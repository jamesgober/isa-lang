//! The x86-64 target description: every instruction form as one row of
//! data, compiled at build time.
//!
//! Each row is written the way the Intel SDM's opcode tables write it: an
//! encoding recipe (`REX.W 83 /0 ib`), an operand signature
//! (`ADD r/m64, imm8`), and a flag-effect column. [`row`] parses the two
//! strings in a `const fn`, so a malformed row is a compile error, and the
//! encoder and decoder consume the compiled [`Form`] records and the two
//! indexes built from them here. Adding a form is adding a row.
//!
//! Notation, beyond the SDM's:
//! - The operand-size prefix is written (`66 81 /0 iw` for `ADD r/m16, imm16`).
//! - `+cc` marks a condition code in the low four opcode bits, so one row
//!   covers all sixteen `Jcc`, `SETcc`, or `CMOVcc` encodings.
//! - `uimm8` and `uimm16` mark unsigned immediates (counts, bit numbers).
//!
//! Row order matters twice. Within a mnemonic it is the encoder's preference:
//! the first form that fits wins, so shorter forms come first. Within an
//! opcode it is the decoder's order (only `90`, where `NOP` and `XCHG` meet,
//! depends on it, and the `XCHG` rows exclude register 0 explicitly).

use super::flags::{FlagEffects, Flags};
use super::form::{ATTR_D64, ATTR_NOT_R0, Form, ModRmSpec, OpcodeField, OperandKind, Slot};
use super::inst::{OP_COUNT, OP_TABLE_NAMES, OPS, Op};
use super::reg::{Reg, Size};

// ---------------------------------------------------------------------------
// Flag-effect column
// ---------------------------------------------------------------------------

/// The flag-effect column of a row.
#[derive(Clone, Copy)]
pub(crate) struct Fx {
    effects: FlagEffects,
    reads_cond: bool,
}

const fn fx(read: Flags, written: Flags, undefined: Flags) -> Fx {
    Fx {
        effects: FlagEffects::new(read, written, undefined),
        reads_cond: false,
    }
}

const NO_FLAGS: Flags = Flags::NONE;
const OSZPC: Flags = Flags::OF
    .union(Flags::SF)
    .union(Flags::ZF)
    .union(Flags::PF)
    .union(Flags::CF);
const SZPC: Flags = Flags::SF.union(Flags::ZF).union(Flags::PF).union(Flags::CF);

/// No flags read or written.
const NONE: Fx = fx(NO_FLAGS, NO_FLAGS, NO_FLAGS);
/// ADD, SUB, CMP, NEG: all six written.
const ARITH: Fx = fx(NO_FLAGS, Flags::ALL, NO_FLAGS);
/// ADC, SBB: read CF, all six written.
const CARRY: Fx = fx(Flags::CF, Flags::ALL, NO_FLAGS);
/// AND, OR, XOR, TEST: OF and CF cleared, SF ZF PF set, AF undefined.
const LOGIC: Fx = fx(NO_FLAGS, OSZPC, Flags::AF);
/// INC, DEC: CF untouched.
const INCDEC: Fx = fx(NO_FLAGS, Flags::ALL.without(Flags::CF), NO_FLAGS);
/// MUL, IMUL: CF and OF written, the rest undefined.
const MULF: Fx = fx(
    NO_FLAGS,
    Flags::CF.union(Flags::OF),
    SZPC.without(Flags::CF).union(Flags::AF),
);
/// DIV, IDIV: all six undefined.
const DIVF: Fx = fx(NO_FLAGS, NO_FLAGS, Flags::ALL);
/// Shifts by one: OF defined.
const SHIFT1: Fx = fx(NO_FLAGS, OSZPC, Flags::AF);
/// Shifts by CL or imm8: OF undefined for counts above one.
const SHIFTN: Fx = fx(NO_FLAGS, SZPC, Flags::OF.union(Flags::AF));
/// Rotates by one: CF and OF.
const ROT1: Fx = fx(NO_FLAGS, Flags::CF.union(Flags::OF), NO_FLAGS);
/// Rotates by CL or imm8: CF, OF undefined.
const ROTN: Fx = fx(NO_FLAGS, Flags::CF, Flags::OF);
/// BT: CF is the bit, ZF untouched, the rest undefined.
const BTF: Fx = fx(
    NO_FLAGS,
    Flags::CF,
    Flags::OF.union(Flags::SF).union(Flags::AF).union(Flags::PF),
);
/// Jcc, SETcc, CMOVcc: read the flags their condition names.
const COND: Fx = Fx {
    effects: FlagEffects::new(NO_FLAGS, NO_FLAGS, NO_FLAGS),
    reads_cond: true,
};

// ---------------------------------------------------------------------------
// The row compiler (evaluated only at compile time)
// ---------------------------------------------------------------------------

/// A row with no attributes.
const fn row(encoding: &'static str, syntax: &'static str, fx: Fx) -> Form {
    compile(encoding, syntax, fx, 0)
}

/// A row with attributes (`ATTR_D64`, `ATTR_NOT_R0`).
const fn row_a(encoding: &'static str, syntax: &'static str, fx: Fx, attrs: u8) -> Form {
    compile(encoding, syntax, fx, attrs)
}

/// The parsed encoding recipe.
#[derive(Clone, Copy)]
struct Recipe {
    p66: bool,
    rex_w: bool,
    map: u8,
    opcode: u8,
    has_opcode: bool,
    field: OpcodeField,
    modrm: ModRmSpec,
    imm_bytes: u8,
    rel: bool,
}

const fn token_is(tok: &[u8], lit: &[u8]) -> bool {
    if tok.len() != lit.len() {
        return false;
    }
    let mut i = 0;
    while i < tok.len() {
        if tok[i] != lit[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const fn recipe_token(mut r: Recipe, tok: &[u8]) -> Recipe {
    if token_is(tok, b"+") {
        return r;
    }
    if token_is(tok, b"REX.W") {
        assert!(
            !r.has_opcode && !r.rex_w,
            "REX.W must come once, before the opcode"
        );
        r.rex_w = true;
        return r;
    }
    if token_is(tok, b"66") && !r.has_opcode && r.map == 0 {
        assert!(!r.p66 && !r.rex_w, "66 must come once, first");
        r.p66 = true;
        return r;
    }
    if tok.len() == 2 && tok[0] == b'/' {
        assert!(r.has_opcode, "ModRM spec before the opcode");
        assert!(matches!(r.modrm, ModRmSpec::Absent), "two ModRM specs");
        r.modrm = match tok[1] {
            b'r' => ModRmSpec::Reg,
            b'0'..=b'7' => ModRmSpec::Ext(tok[1] - b'0'),
            _ => panic!("unknown ModRM spec"),
        };
        return r;
    }
    let imm = match tok {
        b"ib" => Some((1, false)),
        b"iw" => Some((2, false)),
        b"id" => Some((4, false)),
        b"io" => Some((8, false)),
        b"cb" => Some((1, true)),
        b"cd" => Some((4, true)),
        _ => None,
    };
    if let Some((bytes, rel)) = imm {
        assert!(
            r.has_opcode && r.imm_bytes == 0,
            "immediate before the opcode, or twice"
        );
        r.imm_bytes = bytes;
        r.rel = rel;
        return r;
    }
    let (Some(hi), Some(lo)) = (
        if tok.len() >= 2 {
            hex_digit(tok[0])
        } else {
            None
        },
        if tok.len() >= 2 {
            hex_digit(tok[1])
        } else {
            None
        },
    ) else {
        panic!("unknown token in an encoding recipe")
    };
    let byte = hi * 16 + lo;
    let suffix = tok.split_at(2).1;
    if !r.has_opcode && r.map == 0 && byte == 0x0F && suffix.is_empty() {
        r.map = 1;
        return r;
    }
    assert!(!r.has_opcode, "two opcode bytes after the escape");
    r.opcode = byte;
    r.has_opcode = true;
    r.field = match suffix {
        b"" => OpcodeField::Fixed,
        b"+rb" | b"+rw" | b"+rd" | b"+ro" => OpcodeField::Reg,
        b"+cc" => OpcodeField::Cond,
        _ => panic!("unknown opcode suffix"),
    };
    match r.field {
        OpcodeField::Fixed => {}
        OpcodeField::Reg => assert!(byte & 7 == 0, "+r opcode must have its low 3 bits clear"),
        OpcodeField::Cond => assert!(byte & 15 == 0, "+cc opcode must have its low 4 bits clear"),
    }
    r
}

const fn parse_recipe(s: &[u8]) -> Recipe {
    let mut r = Recipe {
        p66: false,
        rex_w: false,
        map: 0,
        opcode: 0,
        has_opcode: false,
        field: OpcodeField::Fixed,
        modrm: ModRmSpec::Absent,
        imm_bytes: 0,
        rel: false,
    };
    let mut i = 0;
    while i < s.len() {
        if s[i] == b' ' {
            i += 1;
            continue;
        }
        let start = i;
        while i < s.len() && s[i] != b' ' {
            i += 1;
        }
        let tok = s.split_at(i).0.split_at(start).1;
        r = recipe_token(r, tok);
    }
    assert!(r.has_opcode, "encoding recipe has no opcode");
    r
}

const fn find_op(name: &[u8]) -> Op {
    let mut i = 0;
    while i < OP_COUNT {
        if token_is(name, OP_TABLE_NAMES[i].as_bytes()) {
            return OPS[i];
        }
        i += 1;
    }
    panic!("unknown mnemonic in a form row")
}

const fn operand_kind(tok: &[u8], op_size: Size) -> OperandKind {
    match tok {
        b"r8" => OperandKind::Reg(Size::Byte),
        b"r16" => OperandKind::Reg(Size::Word),
        b"r32" => OperandKind::Reg(Size::Dword),
        b"r64" => OperandKind::Reg(Size::Qword),
        b"r/m8" => OperandKind::RegMem(Size::Byte),
        b"r/m16" => OperandKind::RegMem(Size::Word),
        b"r/m32" => OperandKind::RegMem(Size::Dword),
        b"r/m64" => OperandKind::RegMem(Size::Qword),
        b"m" => OperandKind::Mem(op_size),
        b"m8" => OperandKind::Mem(Size::Byte),
        b"m16" => OperandKind::Mem(Size::Word),
        b"m32" => OperandKind::Mem(Size::Dword),
        b"m64" => OperandKind::Mem(Size::Qword),
        b"imm8" => OperandKind::Imm {
            bits: 8,
            signed: true,
        },
        b"imm16" => OperandKind::Imm {
            bits: 16,
            signed: true,
        },
        b"imm32" => OperandKind::Imm {
            bits: 32,
            signed: true,
        },
        b"imm64" => OperandKind::Imm {
            bits: 64,
            signed: true,
        },
        b"uimm8" => OperandKind::Imm {
            bits: 8,
            signed: false,
        },
        b"uimm16" => OperandKind::Imm {
            bits: 16,
            signed: false,
        },
        b"rel8" => OperandKind::Rel { bits: 8 },
        b"rel32" => OperandKind::Rel { bits: 32 },
        b"AL" => OperandKind::Fixed(Reg::AL),
        b"AX" => OperandKind::Fixed(Reg::AX),
        b"EAX" => OperandKind::Fixed(Reg::EAX),
        b"RAX" => OperandKind::Fixed(Reg::RAX),
        b"CL" => OperandKind::Fixed(Reg::CL),
        b"1" => OperandKind::One,
        _ => panic!("unknown operand kind in a form row"),
    }
}

/// Splits `ADD r/m64, imm8` into its mnemonic and operand kinds.
const fn parse_syntax(s: &[u8], op_size: Size) -> (Op, [OperandKind; 3], u8) {
    let mut i = 0;
    while i < s.len() && s[i] != b' ' {
        i += 1;
    }
    let op = find_op(s.split_at(i).0);
    let mut kinds = [OperandKind::One; 3];
    let mut count = 0usize;
    while i < s.len() {
        while i < s.len() && (s[i] == b' ' || s[i] == b',') {
            i += 1;
        }
        if i == s.len() {
            break;
        }
        let start = i;
        while i < s.len() && s[i] != b',' && s[i] != b' ' {
            i += 1;
        }
        assert!(count < 3, "more than three operands in a form row");
        kinds[count] = operand_kind(s.split_at(i).0.split_at(start).1, op_size);
        count += 1;
    }
    (op, kinds, count as u8)
}

/// Compiles one row, checking that the operand signature and the encoding
/// recipe describe the same fields. Any inconsistency is a build failure.
const fn compile(encoding: &'static str, syntax: &'static str, fx: Fx, attrs: u8) -> Form {
    let r = parse_recipe(encoding.as_bytes());
    let op_size = if r.p66 {
        Size::Word
    } else if r.rex_w || attrs & ATTR_D64 != 0 {
        Size::Qword
    } else {
        Size::Dword
    };
    let (op, kinds, count) = parse_syntax(syntax.as_bytes(), op_size);
    let mut slots = [Slot::Implied; 3];
    let (mut reg, mut rm, mut opreg, mut imm) = (false, false, false, false);
    let mut rm_mem_only = false;
    let mut i = 0;
    while i < count as usize {
        slots[i] = match kinds[i] {
            OperandKind::Reg(_) => {
                if matches!(r.field, OpcodeField::Reg) && !opreg {
                    opreg = true;
                    Slot::Opcode
                } else {
                    assert!(
                        matches!(r.modrm, ModRmSpec::Reg) && !reg,
                        "register has no field"
                    );
                    reg = true;
                    Slot::Reg
                }
            }
            OperandKind::RegMem(_) | OperandKind::Mem(_) => {
                assert!(
                    !matches!(r.modrm, ModRmSpec::Absent) && !rm,
                    "r/m operand has no field"
                );
                rm = true;
                rm_mem_only = matches!(kinds[i], OperandKind::Mem(_));
                Slot::Rm
            }
            OperandKind::Imm { bits, .. } => {
                assert!(
                    !r.rel && r.imm_bytes * 8 == bits && !imm,
                    "imm kind and field differ"
                );
                imm = true;
                Slot::Imm
            }
            OperandKind::Rel { bits } => {
                assert!(
                    r.rel && r.imm_bytes * 8 == bits && !imm,
                    "rel kind and field differ"
                );
                imm = true;
                Slot::Imm
            }
            OperandKind::Fixed(_) | OperandKind::One => Slot::Implied,
        };
        i += 1;
    }
    assert!(
        reg == matches!(r.modrm, ModRmSpec::Reg),
        "/r without a register operand"
    );
    assert!(
        rm != matches!(r.modrm, ModRmSpec::Absent),
        "ModRM without an r/m operand"
    );
    assert!(
        opreg == matches!(r.field, OpcodeField::Reg),
        "+r without a register operand"
    );
    assert!(
        imm == (r.imm_bytes != 0),
        "immediate field without an operand"
    );
    assert!(
        op.has_cond() == matches!(r.field, OpcodeField::Cond),
        "+cc and mnemonic differ"
    );
    assert!(
        op.has_cond() == fx.reads_cond,
        "condition forms must read their condition"
    );
    Form {
        kinds,
        slots,
        flags: fx.effects,
        id: 0,
        op,
        count,
        map: r.map,
        opcode: r.opcode,
        field: r.field,
        modrm: r.modrm,
        imm_bytes: r.imm_bytes,
        p66: r.p66,
        rex_w: r.rex_w,
        attrs,
        reads_cond: fx.reads_cond,
        rm_mem_only,
        encoding,
        syntax,
    }
}

// ---------------------------------------------------------------------------
// The rows
// ---------------------------------------------------------------------------

#[rustfmt::skip]
const TABLE: &[Form] = &[
    // ===== Integer arithmetic and logic (SDM Vol. 2: ADD, OR, ADC, SBB, AND, SUB, XOR, CMP) =====
    // ADD
    row("04 ib",                 "ADD AL, imm8",          ARITH),
    row("80 /0 ib",              "ADD r/m8, imm8",        ARITH),
    row("66 83 /0 ib",           "ADD r/m16, imm8",       ARITH),
    row("83 /0 ib",              "ADD r/m32, imm8",       ARITH),
    row("REX.W 83 /0 ib",        "ADD r/m64, imm8",       ARITH),
    row("66 05 iw",              "ADD AX, imm16",         ARITH),
    row("05 id",                 "ADD EAX, imm32",        ARITH),
    row("REX.W 05 id",           "ADD RAX, imm32",        ARITH),
    row("66 81 /0 iw",           "ADD r/m16, imm16",      ARITH),
    row("81 /0 id",              "ADD r/m32, imm32",      ARITH),
    row("REX.W 81 /0 id",        "ADD r/m64, imm32",      ARITH),
    row("00 /r",                 "ADD r/m8, r8",          ARITH),
    row("66 01 /r",              "ADD r/m16, r16",        ARITH),
    row("01 /r",                 "ADD r/m32, r32",        ARITH),
    row("REX.W 01 /r",           "ADD r/m64, r64",        ARITH),
    row("02 /r",                 "ADD r8, r/m8",          ARITH),
    row("66 03 /r",              "ADD r16, r/m16",        ARITH),
    row("03 /r",                 "ADD r32, r/m32",        ARITH),
    row("REX.W 03 /r",           "ADD r64, r/m64",        ARITH),
    // OR
    row("0C ib",                 "OR AL, imm8",           LOGIC),
    row("80 /1 ib",              "OR r/m8, imm8",         LOGIC),
    row("66 83 /1 ib",           "OR r/m16, imm8",        LOGIC),
    row("83 /1 ib",              "OR r/m32, imm8",        LOGIC),
    row("REX.W 83 /1 ib",        "OR r/m64, imm8",        LOGIC),
    row("66 0D iw",              "OR AX, imm16",          LOGIC),
    row("0D id",                 "OR EAX, imm32",         LOGIC),
    row("REX.W 0D id",           "OR RAX, imm32",         LOGIC),
    row("66 81 /1 iw",           "OR r/m16, imm16",       LOGIC),
    row("81 /1 id",              "OR r/m32, imm32",       LOGIC),
    row("REX.W 81 /1 id",        "OR r/m64, imm32",       LOGIC),
    row("08 /r",                 "OR r/m8, r8",           LOGIC),
    row("66 09 /r",              "OR r/m16, r16",         LOGIC),
    row("09 /r",                 "OR r/m32, r32",         LOGIC),
    row("REX.W 09 /r",           "OR r/m64, r64",         LOGIC),
    row("0A /r",                 "OR r8, r/m8",           LOGIC),
    row("66 0B /r",              "OR r16, r/m16",         LOGIC),
    row("0B /r",                 "OR r32, r/m32",         LOGIC),
    row("REX.W 0B /r",           "OR r64, r/m64",         LOGIC),
    // ADC
    row("14 ib",                 "ADC AL, imm8",          CARRY),
    row("80 /2 ib",              "ADC r/m8, imm8",        CARRY),
    row("66 83 /2 ib",           "ADC r/m16, imm8",       CARRY),
    row("83 /2 ib",              "ADC r/m32, imm8",       CARRY),
    row("REX.W 83 /2 ib",        "ADC r/m64, imm8",       CARRY),
    row("66 15 iw",              "ADC AX, imm16",         CARRY),
    row("15 id",                 "ADC EAX, imm32",        CARRY),
    row("REX.W 15 id",           "ADC RAX, imm32",        CARRY),
    row("66 81 /2 iw",           "ADC r/m16, imm16",      CARRY),
    row("81 /2 id",              "ADC r/m32, imm32",      CARRY),
    row("REX.W 81 /2 id",        "ADC r/m64, imm32",      CARRY),
    row("10 /r",                 "ADC r/m8, r8",          CARRY),
    row("66 11 /r",              "ADC r/m16, r16",        CARRY),
    row("11 /r",                 "ADC r/m32, r32",        CARRY),
    row("REX.W 11 /r",           "ADC r/m64, r64",        CARRY),
    row("12 /r",                 "ADC r8, r/m8",          CARRY),
    row("66 13 /r",              "ADC r16, r/m16",        CARRY),
    row("13 /r",                 "ADC r32, r/m32",        CARRY),
    row("REX.W 13 /r",           "ADC r64, r/m64",        CARRY),
    // SBB
    row("1C ib",                 "SBB AL, imm8",          CARRY),
    row("80 /3 ib",              "SBB r/m8, imm8",        CARRY),
    row("66 83 /3 ib",           "SBB r/m16, imm8",       CARRY),
    row("83 /3 ib",              "SBB r/m32, imm8",       CARRY),
    row("REX.W 83 /3 ib",        "SBB r/m64, imm8",       CARRY),
    row("66 1D iw",              "SBB AX, imm16",         CARRY),
    row("1D id",                 "SBB EAX, imm32",        CARRY),
    row("REX.W 1D id",           "SBB RAX, imm32",        CARRY),
    row("66 81 /3 iw",           "SBB r/m16, imm16",      CARRY),
    row("81 /3 id",              "SBB r/m32, imm32",      CARRY),
    row("REX.W 81 /3 id",        "SBB r/m64, imm32",      CARRY),
    row("18 /r",                 "SBB r/m8, r8",          CARRY),
    row("66 19 /r",              "SBB r/m16, r16",        CARRY),
    row("19 /r",                 "SBB r/m32, r32",        CARRY),
    row("REX.W 19 /r",           "SBB r/m64, r64",        CARRY),
    row("1A /r",                 "SBB r8, r/m8",          CARRY),
    row("66 1B /r",              "SBB r16, r/m16",        CARRY),
    row("1B /r",                 "SBB r32, r/m32",        CARRY),
    row("REX.W 1B /r",           "SBB r64, r/m64",        CARRY),
    // AND
    row("24 ib",                 "AND AL, imm8",          LOGIC),
    row("80 /4 ib",              "AND r/m8, imm8",        LOGIC),
    row("66 83 /4 ib",           "AND r/m16, imm8",       LOGIC),
    row("83 /4 ib",              "AND r/m32, imm8",       LOGIC),
    row("REX.W 83 /4 ib",        "AND r/m64, imm8",       LOGIC),
    row("66 25 iw",              "AND AX, imm16",         LOGIC),
    row("25 id",                 "AND EAX, imm32",        LOGIC),
    row("REX.W 25 id",           "AND RAX, imm32",        LOGIC),
    row("66 81 /4 iw",           "AND r/m16, imm16",      LOGIC),
    row("81 /4 id",              "AND r/m32, imm32",      LOGIC),
    row("REX.W 81 /4 id",        "AND r/m64, imm32",      LOGIC),
    row("20 /r",                 "AND r/m8, r8",          LOGIC),
    row("66 21 /r",              "AND r/m16, r16",        LOGIC),
    row("21 /r",                 "AND r/m32, r32",        LOGIC),
    row("REX.W 21 /r",           "AND r/m64, r64",        LOGIC),
    row("22 /r",                 "AND r8, r/m8",          LOGIC),
    row("66 23 /r",              "AND r16, r/m16",        LOGIC),
    row("23 /r",                 "AND r32, r/m32",        LOGIC),
    row("REX.W 23 /r",           "AND r64, r/m64",        LOGIC),
    // SUB
    row("2C ib",                 "SUB AL, imm8",          ARITH),
    row("80 /5 ib",              "SUB r/m8, imm8",        ARITH),
    row("66 83 /5 ib",           "SUB r/m16, imm8",       ARITH),
    row("83 /5 ib",              "SUB r/m32, imm8",       ARITH),
    row("REX.W 83 /5 ib",        "SUB r/m64, imm8",       ARITH),
    row("66 2D iw",              "SUB AX, imm16",         ARITH),
    row("2D id",                 "SUB EAX, imm32",        ARITH),
    row("REX.W 2D id",           "SUB RAX, imm32",        ARITH),
    row("66 81 /5 iw",           "SUB r/m16, imm16",      ARITH),
    row("81 /5 id",              "SUB r/m32, imm32",      ARITH),
    row("REX.W 81 /5 id",        "SUB r/m64, imm32",      ARITH),
    row("28 /r",                 "SUB r/m8, r8",          ARITH),
    row("66 29 /r",              "SUB r/m16, r16",        ARITH),
    row("29 /r",                 "SUB r/m32, r32",        ARITH),
    row("REX.W 29 /r",           "SUB r/m64, r64",        ARITH),
    row("2A /r",                 "SUB r8, r/m8",          ARITH),
    row("66 2B /r",              "SUB r16, r/m16",        ARITH),
    row("2B /r",                 "SUB r32, r/m32",        ARITH),
    row("REX.W 2B /r",           "SUB r64, r/m64",        ARITH),
    // XOR
    row("34 ib",                 "XOR AL, imm8",          LOGIC),
    row("80 /6 ib",              "XOR r/m8, imm8",        LOGIC),
    row("66 83 /6 ib",           "XOR r/m16, imm8",       LOGIC),
    row("83 /6 ib",              "XOR r/m32, imm8",       LOGIC),
    row("REX.W 83 /6 ib",        "XOR r/m64, imm8",       LOGIC),
    row("66 35 iw",              "XOR AX, imm16",         LOGIC),
    row("35 id",                 "XOR EAX, imm32",        LOGIC),
    row("REX.W 35 id",           "XOR RAX, imm32",        LOGIC),
    row("66 81 /6 iw",           "XOR r/m16, imm16",      LOGIC),
    row("81 /6 id",              "XOR r/m32, imm32",      LOGIC),
    row("REX.W 81 /6 id",        "XOR r/m64, imm32",      LOGIC),
    row("30 /r",                 "XOR r/m8, r8",          LOGIC),
    row("66 31 /r",              "XOR r/m16, r16",        LOGIC),
    row("31 /r",                 "XOR r/m32, r32",        LOGIC),
    row("REX.W 31 /r",           "XOR r/m64, r64",        LOGIC),
    row("32 /r",                 "XOR r8, r/m8",          LOGIC),
    row("66 33 /r",              "XOR r16, r/m16",        LOGIC),
    row("33 /r",                 "XOR r32, r/m32",        LOGIC),
    row("REX.W 33 /r",           "XOR r64, r/m64",        LOGIC),
    // CMP
    row("3C ib",                 "CMP AL, imm8",          ARITH),
    row("80 /7 ib",              "CMP r/m8, imm8",        ARITH),
    row("66 83 /7 ib",           "CMP r/m16, imm8",       ARITH),
    row("83 /7 ib",              "CMP r/m32, imm8",       ARITH),
    row("REX.W 83 /7 ib",        "CMP r/m64, imm8",       ARITH),
    row("66 3D iw",              "CMP AX, imm16",         ARITH),
    row("3D id",                 "CMP EAX, imm32",        ARITH),
    row("REX.W 3D id",           "CMP RAX, imm32",        ARITH),
    row("66 81 /7 iw",           "CMP r/m16, imm16",      ARITH),
    row("81 /7 id",              "CMP r/m32, imm32",      ARITH),
    row("REX.W 81 /7 id",        "CMP r/m64, imm32",      ARITH),
    row("38 /r",                 "CMP r/m8, r8",          ARITH),
    row("66 39 /r",              "CMP r/m16, r16",        ARITH),
    row("39 /r",                 "CMP r/m32, r32",        ARITH),
    row("REX.W 39 /r",           "CMP r/m64, r64",        ARITH),
    row("3A /r",                 "CMP r8, r/m8",          ARITH),
    row("66 3B /r",              "CMP r16, r/m16",        ARITH),
    row("3B /r",                 "CMP r32, r/m32",        ARITH),
    row("REX.W 3B /r",           "CMP r64, r/m64",        ARITH),
    // ===== One-operand groups F6/F7 (NOT, NEG, MUL, IMUL, DIV, IDIV) =====
    // NOT
    row("F6 /2",                 "NOT r/m8",              NONE),
    row("66 F7 /2",              "NOT r/m16",             NONE),
    row("F7 /2",                 "NOT r/m32",             NONE),
    row("REX.W F7 /2",           "NOT r/m64",             NONE),
    // NEG
    row("F6 /3",                 "NEG r/m8",              ARITH),
    row("66 F7 /3",              "NEG r/m16",             ARITH),
    row("F7 /3",                 "NEG r/m32",             ARITH),
    row("REX.W F7 /3",           "NEG r/m64",             ARITH),
    // MUL
    row("F6 /4",                 "MUL r/m8",              MULF),
    row("66 F7 /4",              "MUL r/m16",             MULF),
    row("F7 /4",                 "MUL r/m32",             MULF),
    row("REX.W F7 /4",           "MUL r/m64",             MULF),
    // IMUL
    row("F6 /5",                 "IMUL r/m8",             MULF),
    row("66 F7 /5",              "IMUL r/m16",            MULF),
    row("F7 /5",                 "IMUL r/m32",            MULF),
    row("REX.W F7 /5",           "IMUL r/m64",            MULF),
    // DIV
    row("F6 /6",                 "DIV r/m8",              DIVF),
    row("66 F7 /6",              "DIV r/m16",             DIVF),
    row("F7 /6",                 "DIV r/m32",             DIVF),
    row("REX.W F7 /6",           "DIV r/m64",             DIVF),
    // IDIV
    row("F6 /7",                 "IDIV r/m8",             DIVF),
    row("66 F7 /7",              "IDIV r/m16",            DIVF),
    row("F7 /7",                 "IDIV r/m32",            DIVF),
    row("REX.W F7 /7",           "IDIV r/m64",            DIVF),
    // ===== Shifts and rotates (ROL, ROR, SHL, SHR, SAR) =====
    // ROL
    row("D0 /0",                 "ROL r/m8, 1",           ROT1),
    row("66 D1 /0",              "ROL r/m16, 1",          ROT1),
    row("D1 /0",                 "ROL r/m32, 1",          ROT1),
    row("REX.W D1 /0",           "ROL r/m64, 1",          ROT1),
    row("C0 /0 ib",              "ROL r/m8, uimm8",       ROTN),
    row("66 C1 /0 ib",           "ROL r/m16, uimm8",      ROTN),
    row("C1 /0 ib",              "ROL r/m32, uimm8",      ROTN),
    row("REX.W C1 /0 ib",        "ROL r/m64, uimm8",      ROTN),
    row("D2 /0",                 "ROL r/m8, CL",          ROTN),
    row("66 D3 /0",              "ROL r/m16, CL",         ROTN),
    row("D3 /0",                 "ROL r/m32, CL",         ROTN),
    row("REX.W D3 /0",           "ROL r/m64, CL",         ROTN),
    // ROR
    row("D0 /1",                 "ROR r/m8, 1",           ROT1),
    row("66 D1 /1",              "ROR r/m16, 1",          ROT1),
    row("D1 /1",                 "ROR r/m32, 1",          ROT1),
    row("REX.W D1 /1",           "ROR r/m64, 1",          ROT1),
    row("C0 /1 ib",              "ROR r/m8, uimm8",       ROTN),
    row("66 C1 /1 ib",           "ROR r/m16, uimm8",      ROTN),
    row("C1 /1 ib",              "ROR r/m32, uimm8",      ROTN),
    row("REX.W C1 /1 ib",        "ROR r/m64, uimm8",      ROTN),
    row("D2 /1",                 "ROR r/m8, CL",          ROTN),
    row("66 D3 /1",              "ROR r/m16, CL",         ROTN),
    row("D3 /1",                 "ROR r/m32, CL",         ROTN),
    row("REX.W D3 /1",           "ROR r/m64, CL",         ROTN),
    // SHL
    row("D0 /4",                 "SHL r/m8, 1",           SHIFT1),
    row("66 D1 /4",              "SHL r/m16, 1",          SHIFT1),
    row("D1 /4",                 "SHL r/m32, 1",          SHIFT1),
    row("REX.W D1 /4",           "SHL r/m64, 1",          SHIFT1),
    row("C0 /4 ib",              "SHL r/m8, uimm8",       SHIFTN),
    row("66 C1 /4 ib",           "SHL r/m16, uimm8",      SHIFTN),
    row("C1 /4 ib",              "SHL r/m32, uimm8",      SHIFTN),
    row("REX.W C1 /4 ib",        "SHL r/m64, uimm8",      SHIFTN),
    row("D2 /4",                 "SHL r/m8, CL",          SHIFTN),
    row("66 D3 /4",              "SHL r/m16, CL",         SHIFTN),
    row("D3 /4",                 "SHL r/m32, CL",         SHIFTN),
    row("REX.W D3 /4",           "SHL r/m64, CL",         SHIFTN),
    // SHR
    row("D0 /5",                 "SHR r/m8, 1",           SHIFT1),
    row("66 D1 /5",              "SHR r/m16, 1",          SHIFT1),
    row("D1 /5",                 "SHR r/m32, 1",          SHIFT1),
    row("REX.W D1 /5",           "SHR r/m64, 1",          SHIFT1),
    row("C0 /5 ib",              "SHR r/m8, uimm8",       SHIFTN),
    row("66 C1 /5 ib",           "SHR r/m16, uimm8",      SHIFTN),
    row("C1 /5 ib",              "SHR r/m32, uimm8",      SHIFTN),
    row("REX.W C1 /5 ib",        "SHR r/m64, uimm8",      SHIFTN),
    row("D2 /5",                 "SHR r/m8, CL",          SHIFTN),
    row("66 D3 /5",              "SHR r/m16, CL",         SHIFTN),
    row("D3 /5",                 "SHR r/m32, CL",         SHIFTN),
    row("REX.W D3 /5",           "SHR r/m64, CL",         SHIFTN),
    // SAR
    row("D0 /7",                 "SAR r/m8, 1",           SHIFT1),
    row("66 D1 /7",              "SAR r/m16, 1",          SHIFT1),
    row("D1 /7",                 "SAR r/m32, 1",          SHIFT1),
    row("REX.W D1 /7",           "SAR r/m64, 1",          SHIFT1),
    row("C0 /7 ib",              "SAR r/m8, uimm8",       SHIFTN),
    row("66 C1 /7 ib",           "SAR r/m16, uimm8",      SHIFTN),
    row("C1 /7 ib",              "SAR r/m32, uimm8",      SHIFTN),
    row("REX.W C1 /7 ib",        "SAR r/m64, uimm8",      SHIFTN),
    row("D2 /7",                 "SAR r/m8, CL",          SHIFTN),
    row("66 D3 /7",              "SAR r/m16, CL",         SHIFTN),
    row("D3 /7",                 "SAR r/m32, CL",         SHIFTN),
    row("REX.W D3 /7",           "SAR r/m64, CL",         SHIFTN),
    // ===== Data movement, multiply, compare, control flow =====
    // MOV
    row("88 /r",                 "MOV r/m8, r8",          NONE),
    row("66 89 /r",              "MOV r/m16, r16",        NONE),
    row("89 /r",                 "MOV r/m32, r32",        NONE),
    row("REX.W 89 /r",           "MOV r/m64, r64",        NONE),
    row("8A /r",                 "MOV r8, r/m8",          NONE),
    row("66 8B /r",              "MOV r16, r/m16",        NONE),
    row("8B /r",                 "MOV r32, r/m32",        NONE),
    row("REX.W 8B /r",           "MOV r64, r/m64",        NONE),
    row("B0+rb ib",              "MOV r8, imm8",          NONE),
    row("C6 /0 ib",              "MOV r/m8, imm8",        NONE),
    row("66 B8+rw iw",           "MOV r16, imm16",        NONE),
    row("66 C7 /0 iw",           "MOV r/m16, imm16",      NONE),
    row("B8+rd id",              "MOV r32, imm32",        NONE),
    row("C7 /0 id",              "MOV r/m32, imm32",      NONE),
    row("REX.W C7 /0 id",        "MOV r/m64, imm32",      NONE),
    row("REX.W B8+rd io",        "MOV r64, imm64",        NONE),
    // MOVSX, MOVSXD, MOVZX
    row("66 0F BE /r",           "MOVSX r16, r/m8",       NONE),
    row("0F BE /r",              "MOVSX r32, r/m8",       NONE),
    row("REX.W 0F BE /r",        "MOVSX r64, r/m8",       NONE),
    row("0F BF /r",              "MOVSX r32, r/m16",      NONE),
    row("REX.W 0F BF /r",        "MOVSX r64, r/m16",      NONE),
    row("REX.W 63 /r",           "MOVSXD r64, r/m32",     NONE),
    row("66 0F B6 /r",           "MOVZX r16, r/m8",       NONE),
    row("0F B6 /r",              "MOVZX r32, r/m8",       NONE),
    row("REX.W 0F B6 /r",        "MOVZX r64, r/m8",       NONE),
    row("0F B7 /r",              "MOVZX r32, r/m16",      NONE),
    row("REX.W 0F B7 /r",        "MOVZX r64, r/m16",      NONE),
    // LEA
    row("66 8D /r",              "LEA r16, m",            NONE),
    row("8D /r",                 "LEA r32, m",            NONE),
    row("REX.W 8D /r",           "LEA r64, m",            NONE),
    // TEST
    row("A8 ib",                 "TEST AL, imm8",         LOGIC),
    row("66 A9 iw",              "TEST AX, imm16",        LOGIC),
    row("A9 id",                 "TEST EAX, imm32",       LOGIC),
    row("REX.W A9 id",           "TEST RAX, imm32",       LOGIC),
    row("F6 /0 ib",              "TEST r/m8, imm8",       LOGIC),
    row("66 F7 /0 iw",           "TEST r/m16, imm16",     LOGIC),
    row("F7 /0 id",              "TEST r/m32, imm32",     LOGIC),
    row("REX.W F7 /0 id",        "TEST r/m64, imm32",     LOGIC),
    row("84 /r",                 "TEST r/m8, r8",         LOGIC),
    row("66 85 /r",              "TEST r/m16, r16",       LOGIC),
    row("85 /r",                 "TEST r/m32, r32",       LOGIC),
    row("REX.W 85 /r",           "TEST r/m64, r64",       LOGIC),
    // INC, DEC
    row("FE /0",                 "INC r/m8",              INCDEC),
    row("66 FF /0",              "INC r/m16",             INCDEC),
    row("FF /0",                 "INC r/m32",             INCDEC),
    row("REX.W FF /0",           "INC r/m64",             INCDEC),
    row("FE /1",                 "DEC r/m8",              INCDEC),
    row("66 FF /1",              "DEC r/m16",             INCDEC),
    row("FF /1",                 "DEC r/m32",             INCDEC),
    row("REX.W FF /1",           "DEC r/m64",             INCDEC),
    // IMUL, two and three operands
    row("66 0F AF /r",           "IMUL r16, r/m16",       MULF),
    row("0F AF /r",              "IMUL r32, r/m32",       MULF),
    row("REX.W 0F AF /r",        "IMUL r64, r/m64",       MULF),
    row("66 6B /r ib",           "IMUL r16, r/m16, imm8", MULF),
    row("6B /r ib",              "IMUL r32, r/m32, imm8", MULF),
    row("REX.W 6B /r ib",        "IMUL r64, r/m64, imm8", MULF),
    row("66 69 /r iw",           "IMUL r16, r/m16, imm16", MULF),
    row("69 /r id",              "IMUL r32, r/m32, imm32", MULF),
    row("REX.W 69 /r id",        "IMUL r64, r/m64, imm32", MULF),
    // CBW, CWDE, CDQE, CWD, CDQ, CQO
    row("66 98",                 "CBW",                   NONE),
    row("98",                    "CWDE",                  NONE),
    row("REX.W 98",              "CDQE",                  NONE),
    row("66 99",                 "CWD",                   NONE),
    row("99",                    "CDQ",                   NONE),
    row("REX.W 99",              "CQO",                   NONE),
    // BT
    row("66 0F A3 /r",           "BT r/m16, r16",         BTF),
    row("0F A3 /r",              "BT r/m32, r32",         BTF),
    row("REX.W 0F A3 /r",        "BT r/m64, r64",         BTF),
    row("66 0F BA /4 ib",        "BT r/m16, uimm8",       BTF),
    row("0F BA /4 ib",           "BT r/m32, uimm8",       BTF),
    row("REX.W 0F BA /4 ib",     "BT r/m64, uimm8",       BTF),
    // SETcc, CMOVcc, Jcc
    row("0F 90+cc /0",           "SETcc r/m8",            COND),
    row("66 0F 40+cc /r",        "CMOVcc r16, r/m16",     COND),
    row("0F 40+cc /r",           "CMOVcc r32, r/m32",     COND),
    row("REX.W 0F 40+cc /r",     "CMOVcc r64, r/m64",     COND),
    row("70+cc cb",              "Jcc rel8",              COND),
    row("0F 80+cc cd",           "Jcc rel32",             COND),
    // JMP, CALL, RET
    row("EB cb",                 "JMP rel8",              NONE),
    row("E9 cd",                 "JMP rel32",             NONE),
    row_a("FF /4",               "JMP r/m64",             NONE, ATTR_D64),
    row("E8 cd",                 "CALL rel32",            NONE),
    row_a("FF /2",               "CALL r/m64",            NONE, ATTR_D64),
    row("C3",                    "RET",                   NONE),
    row("C2 iw",                 "RET uimm16",            NONE),
    // PUSH, POP
    row_a("50+rd",               "PUSH r64",              NONE, ATTR_D64),
    row_a("FF /6",               "PUSH r/m64",            NONE, ATTR_D64),
    row_a("6A ib",               "PUSH imm8",             NONE, ATTR_D64),
    row_a("68 id",               "PUSH imm32",            NONE, ATTR_D64),
    row_a("58+rd",               "POP r64",               NONE, ATTR_D64),
    row_a("8F /0",               "POP r/m64",             NONE, ATTR_D64),
    // NOP (90 is NOP whatever the operand size; see XCHG), UD2, INT3
    row("90",                    "NOP",                   NONE),
    row("66 90",                 "NOP",                   NONE),
    row("REX.W 90",              "NOP",                   NONE),
    row("66 0F 1F /0",           "NOP r/m16",             NONE),
    row("0F 1F /0",              "NOP r/m32",             NONE),
    row("0F 0B",                 "UD2",                   NONE),
    row("CC",                    "INT3",                  NONE),
    // XCHG (the +r forms never use register 0: 90 is NOP)
    row_a("66 90+rw",            "XCHG r16, AX",          NONE, ATTR_NOT_R0),
    row_a("90+rd",               "XCHG r32, EAX",         NONE, ATTR_NOT_R0),
    row_a("REX.W 90+rd",         "XCHG r64, RAX",         NONE, ATTR_NOT_R0),
    row("86 /r",                 "XCHG r/m8, r8",         NONE),
    row("66 87 /r",              "XCHG r/m16, r16",       NONE),
    row("87 /r",                 "XCHG r/m32, r32",       NONE),
    row("REX.W 87 /r",           "XCHG r/m64, r64",       NONE),
];

// ---------------------------------------------------------------------------
// The compiled table and its indexes
// ---------------------------------------------------------------------------

/// Number of forms in the table.
pub(crate) const FORM_COUNT: usize = TABLE.len();

/// Every form, numbered by position.
pub(crate) static FORMS: [Form; FORM_COUNT] = number_forms();

const fn number_forms() -> [Form; FORM_COUNT] {
    let mut out = [TABLE[0]; FORM_COUNT];
    let mut i = 0;
    while i < FORM_COUNT {
        out[i] = TABLE[i];
        out[i].id = i as u16;
        i += 1;
    }
    out
}

/// A bucketed index: the form ids for key `k` are `list[start[k]..start[k + 1]]`,
/// in table order.
pub(crate) struct Index<const KEYS: usize, const LEN: usize> {
    start: [u16; KEYS],
    list: [u16; LEN],
}

impl<const KEYS: usize, const LEN: usize> Index<KEYS, LEN> {
    /// The forms filed under `key`, in table order; empty for an unknown key.
    #[inline]
    pub(crate) fn get(&self, key: usize) -> &[u16] {
        let (Some(&lo), Some(&hi)) = (self.start.get(key), self.start.get(key + 1)) else {
            return &[];
        };
        self.list
            .get(usize::from(lo)..usize::from(hi))
            .unwrap_or(&[])
    }
}

/// The encoder's index: forms by mnemonic family, in preference order.
pub(crate) static BY_OP: Index<{ OP_COUNT + 1 }, FORM_COUNT> = build_by_op();

const fn build_by_op() -> Index<{ OP_COUNT + 1 }, FORM_COUNT> {
    let mut start = [0u16; OP_COUNT + 1];
    let mut i = 0;
    while i < FORM_COUNT {
        start[TABLE[i].op.index() + 1] += 1;
        i += 1;
    }
    let mut k = 0;
    while k < OP_COUNT {
        start[k + 1] += start[k];
        k += 1;
    }
    let mut next = start;
    let mut list = [0u16; FORM_COUNT];
    let mut i = 0;
    while i < FORM_COUNT {
        let key = TABLE[i].op.index();
        list[next[key] as usize] = i as u16;
        next[key] += 1;
        i += 1;
    }
    Index { start, list }
}

/// Opcode keys: 256 one-byte opcodes, then 256 behind the `0F` escape.
pub(crate) const OPCODE_KEYS: usize = 512;

/// How many opcode values a form covers: 1, 8 for `+r`, 16 for `+cc`.
const fn span(form: &Form) -> usize {
    match form.field {
        OpcodeField::Fixed => 1,
        OpcodeField::Reg => 8,
        OpcodeField::Cond => 16,
    }
}

const fn count_opcode_entries() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < FORM_COUNT {
        n += span(&TABLE[i]);
        i += 1;
    }
    n
}

const OPCODE_ENTRIES: usize = count_opcode_entries();

const OPCODE_INDEX: Index<{ OPCODE_KEYS + 1 }, OPCODE_ENTRIES> = build_by_opcode();

/// The decoder's index: forms by opcode key, in table order.
pub(crate) static BY_OPCODE: Index<{ OPCODE_KEYS + 1 }, OPCODE_ENTRIES> = OPCODE_INDEX;

const fn build_by_opcode() -> Index<{ OPCODE_KEYS + 1 }, OPCODE_ENTRIES> {
    let mut start = [0u16; OPCODE_KEYS + 1];
    let mut i = 0;
    while i < FORM_COUNT {
        let f = &TABLE[i];
        let base = f.map as usize * 256 + f.opcode as usize;
        let mut k = 0;
        while k < span(f) {
            start[base + k + 1] += 1;
            k += 1;
        }
        i += 1;
    }
    let mut k = 0;
    while k < OPCODE_KEYS {
        start[k + 1] += start[k];
        k += 1;
    }
    let mut next = start;
    let mut list = [0u16; OPCODE_ENTRIES];
    let mut i = 0;
    while i < FORM_COUNT {
        let f = &TABLE[i];
        let base = f.map as usize * 256 + f.opcode as usize;
        let mut k = 0;
        while k < span(f) {
            list[next[base + k] as usize] = i as u16;
            next[base + k] += 1;
            k += 1;
        }
        i += 1;
    }
    Index { start, list }
}

/// Build-time checks on the table as a whole.
const _: () = {
    assert!(FORM_COUNT > 0 && FORM_COUNT < u16::MAX as usize);
    // The hardware decides from the opcode alone whether a ModRM byte follows,
    // so every form filed under one opcode must agree.
    let mut key = 0;
    while key < OPCODE_KEYS {
        let lo = OPCODE_INDEX.start[key] as usize;
        let hi = OPCODE_INDEX.start[key + 1] as usize;
        let mut j = lo;
        while j < hi {
            let a = &TABLE[OPCODE_INDEX.list[lo] as usize];
            let b = &TABLE[OPCODE_INDEX.list[j] as usize];
            assert!(
                matches!(a.modrm, ModRmSpec::Absent) == matches!(b.modrm, ModRmSpec::Absent),
                "forms sharing an opcode disagree on ModRM"
            );
            j += 1;
        }
        key += 1;
    }
};

/// The form with table index `id`.
#[inline]
pub(crate) fn form(id: u16) -> Option<&'static Form> {
    FORMS.get(usize::from(id))
}

/// The forms an opcode key might decode as.
#[inline]
pub(crate) fn by_opcode(map: u8, opcode: u8) -> &'static [u16] {
    BY_OPCODE.get(usize::from(map) * 256 + usize::from(opcode))
}

/// The forms a mnemonic family may encode as, in preference order.
#[inline]
pub(crate) fn by_op(op: Op) -> &'static [u16] {
    BY_OP.get(op.index())
}

/// The first form of `op` whose encoding recipe matches. Used to pin the
/// assembler's branch forms to table rows at build time.
pub(crate) const fn find_form(op: Op, imm_bytes: u8, map: u8) -> u16 {
    let mut i = 0;
    while i < FORM_COUNT {
        let f = &TABLE[i];
        if f.op.index() == op.index() && f.imm_bytes == imm_bytes && f.map == map {
            return i as u16;
        }
        i += 1;
    }
    panic!("no such form")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forms_are_numbered_by_position() {
        for (i, f) in FORMS.iter().enumerate() {
            assert_eq!(f.index(), i);
        }
    }

    #[test]
    fn test_by_op_lists_every_form_once_in_order() {
        let mut seen = [false; FORM_COUNT];
        for op in OPS {
            let ids = by_op(op);
            assert!(ids.windows(2).all(|w| w[0] < w[1]));
            for &id in ids {
                let f = form(id).map(|f| f.op);
                assert_eq!(f, Some(op));
                assert!(!seen[usize::from(id)]);
                seen[usize::from(id)] = true;
            }
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn test_by_opcode_covers_every_opcode_of_every_form() {
        for f in &FORMS {
            for k in 0..span(f) {
                let ids = by_opcode(f.map, f.opcode + k as u8);
                assert!(ids.contains(&(f.index() as u16)), "{f}");
            }
        }
        assert!(by_opcode(0, 0x0F).is_empty());
        assert!(by_opcode(1, 0x38).is_empty());
    }

    #[test]
    fn test_index_get_out_of_range_is_empty() {
        assert!(BY_OP.get(OP_COUNT).is_empty());
        assert!(BY_OP.get(usize::MAX - 1).is_empty());
        assert!(BY_OPCODE.get(OPCODE_KEYS).is_empty());
    }

    #[test]
    fn test_row_parses_sdm_notation() {
        let f = row("REX.W 83 /0 ib", "ADD r/m64, imm8", ARITH);
        assert!(f.rex_w && !f.p66 && f.map == 0 && f.opcode == 0x83);
        assert_eq!(f.modrm, ModRmSpec::Ext(0));
        assert_eq!(f.imm_bytes, 1);
        assert_eq!(f.slots[..2], [Slot::Rm, Slot::Imm]);
        let j = row("0F 80+cc cd", "Jcc rel32", COND);
        assert!(j.map == 1 && j.field == OpcodeField::Cond && j.reads_cond);
        let p = row_a("50+rd", "PUSH r64", NONE, ATTR_D64);
        assert_eq!(p.slots[0], Slot::Opcode);
        let l = row("66 8D /r", "LEA r16, m", NONE);
        assert_eq!(l.kinds[1], OperandKind::Mem(Size::Word));
    }

    #[test]
    fn test_flag_columns() {
        assert_eq!(INCDEC.effects.written(), Flags::ALL.without(Flags::CF));
        assert_eq!(MULF.effects.clobbered(), Flags::ALL);
        assert!(!MULF.effects.written().contains(Flags::ZF));
        assert_eq!(LOGIC.effects.clobbered(), Flags::ALL);
        assert_eq!(SHIFTN.effects.clobbered(), Flags::ALL);
        assert_eq!(BTF.effects.clobbered(), Flags::ALL.without(Flags::ZF));
        assert_eq!(ROTN.effects.clobbered(), Flags::CF | Flags::OF);
        assert!(COND.reads_cond && NONE.effects.clobbered().is_empty());
        assert_eq!(CARRY.effects.read(), Flags::CF);
        assert_eq!(
            (ARITH.effects.written(), DIVF.effects.undefined()),
            (Flags::ALL, Flags::ALL)
        );
        assert_eq!(
            (SHIFT1.effects.written(), ROT1.effects.written()),
            (OSZPC, Flags::CF | Flags::OF)
        );
    }
}
