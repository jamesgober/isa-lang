//! Operand-space generators shared by the integration tests.
//!
//! Everything here is built from the public table (`forms()`) and written
//! independently of the crate's internals: the mnemonic of a form is read
//! from its syntax string, and operand candidates are enumerated from its
//! operand kinds.

#![allow(dead_code, clippy::unwrap_used)]

use isa_lang::x86_64::{
    Cond, Form, Inst, Mem, Mnemonic, Operand, OperandKind, Reg, Scale, Size, decode, encode,
    encode_form,
};

pub const SIZES: [Size; 4] = [Size::Byte, Size::Word, Size::Dword, Size::Qword];
pub const SCALES: [Scale; 4] = [Scale::X1, Scale::X2, Scale::X4, Scale::X8];
pub const HIGH: [Reg; 4] = [Reg::AH, Reg::CH, Reg::DH, Reg::BH];

/// Displacements at every field boundary: none, disp8, disp32.
pub const DISPS: [i32; 9] = [0, 1, -1, 127, -128, 128, -129, i32::MAX, i32::MIN];

/// The mnemonic a form's syntax names, with `cond` for the `cc` families.
pub fn mnemonic(form: &Form, cond: Cond) -> Mnemonic {
    let name = form.syntax().split(' ').next().unwrap();
    match name {
        "ADC" => Mnemonic::Adc,
        "ADD" => Mnemonic::Add,
        "AND" => Mnemonic::And,
        "BT" => Mnemonic::Bt,
        "CALL" => Mnemonic::Call,
        "CBW" => Mnemonic::Cbw,
        "CDQ" => Mnemonic::Cdq,
        "CDQE" => Mnemonic::Cdqe,
        "CMOVcc" => Mnemonic::Cmovcc(cond),
        "CMP" => Mnemonic::Cmp,
        "CQO" => Mnemonic::Cqo,
        "CWD" => Mnemonic::Cwd,
        "CWDE" => Mnemonic::Cwde,
        "DEC" => Mnemonic::Dec,
        "DIV" => Mnemonic::Div,
        "IDIV" => Mnemonic::Idiv,
        "IMUL" => Mnemonic::Imul,
        "INC" => Mnemonic::Inc,
        "INT3" => Mnemonic::Int3,
        "Jcc" => Mnemonic::Jcc(cond),
        "JMP" => Mnemonic::Jmp,
        "LEA" => Mnemonic::Lea,
        "MOV" => Mnemonic::Mov,
        "MOVSX" => Mnemonic::Movsx,
        "MOVSXD" => Mnemonic::Movsxd,
        "MOVZX" => Mnemonic::Movzx,
        "MUL" => Mnemonic::Mul,
        "NEG" => Mnemonic::Neg,
        "NOP" => Mnemonic::Nop,
        "NOT" => Mnemonic::Not,
        "OR" => Mnemonic::Or,
        "POP" => Mnemonic::Pop,
        "PUSH" => Mnemonic::Push,
        "RET" => Mnemonic::Ret,
        "ROL" => Mnemonic::Rol,
        "ROR" => Mnemonic::Ror,
        "SAR" => Mnemonic::Sar,
        "SBB" => Mnemonic::Sbb,
        "SETcc" => Mnemonic::Setcc(cond),
        "SHL" => Mnemonic::Shl,
        "SHR" => Mnemonic::Shr,
        "SUB" => Mnemonic::Sub,
        "TEST" => Mnemonic::Test,
        "UD2" => Mnemonic::Ud2,
        "XCHG" => Mnemonic::Xchg,
        "XOR" => Mnemonic::Xor,
        other => panic!("unknown mnemonic {other}"),
    }
}

pub fn is_cc(form: &Form) -> bool {
    form.reads_condition()
}

pub fn conds() -> impl Iterator<Item = Cond> {
    (0..16).map(|c| Cond::from_code(c).unwrap())
}

/// Every register of a width, including the high-byte registers at 8 bits.
pub fn regs(size: Size) -> Vec<Reg> {
    let mut v: Vec<Reg> = (0..16).map(|n| Reg::new(size, n).unwrap()).collect();
    if size == Size::Byte {
        v.extend(HIGH);
    }
    v
}

fn q(n: u8) -> Reg {
    Reg::new(Size::Qword, n).unwrap()
}

/// Every addressing shape: each base (none, RIP, all sixteen) with every
/// displacement class, and each base with every index and scale.
pub fn mem_shapes(size: Size) -> Vec<Mem> {
    let mut v = Vec::new();
    for disp in DISPS {
        v.push(Mem::absolute(size, disp));
        v.push(Mem::rip(size, disp));
        for b in 0..16 {
            v.push(Mem::new(size, q(b)).with_disp(disp));
        }
    }
    let mut k = 0;
    for index in (0..16).filter(|&n| n != 4) {
        for scale in SCALES {
            k += 1;
            let disp = DISPS[k % DISPS.len()];
            v.push(Mem::absolute(size, disp).with_index(q(index), scale));
            for b in 0..16 {
                let disp = DISPS[(k + usize::from(b)) % DISPS.len()];
                v.push(
                    Mem::new(size, q(b))
                        .with_index(q(index), scale)
                        .with_disp(disp),
                );
            }
        }
    }
    v
}

/// Immediate values at every boundary of a field.
pub fn imm_values(bits: u8, signed: bool) -> Vec<i64> {
    let (min, max) = if signed {
        if bits == 64 {
            (i64::MIN, i64::MAX)
        } else {
            (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
        }
    } else {
        (0, (1i64 << bits) - 1)
    };
    let mut v = vec![min, min + 1, max - 1, max, 0, 1, 2];
    if signed {
        v.extend([-1, -2]);
    }
    v.extend([max / 3, min / 3]);
    v.sort_unstable();
    v.dedup();
    v
}

/// Candidate operands for one kind. `rel_len` is the instruction length of
/// a relative form, so the generated targets hit the field's boundaries.
pub fn candidates(kind: OperandKind, rel_len: i64) -> Vec<Operand> {
    match kind {
        OperandKind::Reg(s) => regs(s).into_iter().map(Operand::Reg).collect(),
        OperandKind::RegMem(s) => {
            let mut v: Vec<Operand> = regs(s).into_iter().map(Operand::Reg).collect();
            v.extend(mem_shapes(s).into_iter().map(Operand::Mem));
            v
        }
        OperandKind::Mem(s) => mem_shapes(s).into_iter().map(Operand::Mem).collect(),
        OperandKind::Fixed(r) => vec![Operand::Reg(r)],
        OperandKind::One => vec![Operand::Imm(1)],
        OperandKind::Imm { bits, signed } => imm_values(bits, signed)
            .into_iter()
            .map(Operand::Imm)
            .collect(),
        OperandKind::Rel { bits } => imm_values(bits, true)
            .into_iter()
            .map(|r| Operand::Rel(r + rel_len))
            .collect(),
        _ => panic!("unknown operand kind"),
    }
}

/// A few operands spread across a candidate list, for holding the other
/// positions while one position is enumerated.
pub fn representatives(all: &[Operand]) -> Vec<Operand> {
    let mut v = vec![all[0]];
    if all.len() > 2 {
        v.push(all[all.len() / 2]);
    }
    if all.len() > 1 {
        v.push(all[all.len() - 1]);
    }
    v
}

/// The length of a relative-branch form (its encoding with target 0).
pub fn rel_len(form: &Form) -> i64 {
    if !form
        .operands()
        .iter()
        .any(|k| matches!(k, OperandKind::Rel { .. }))
    {
        return 0;
    }
    let inst = Inst::new(mnemonic(form, Cond::E), [Operand::Rel(0)]);
    encode_form(form, &inst).unwrap().len() as i64
}

/// Whether the operands need a REX prefix (beyond what the form forces).
pub fn operands_need_rex(ops: &[Operand]) -> bool {
    ops.iter().any(|op| match op {
        Operand::Reg(r) => {
            r.num() >= 8 || (r.size() == Size::Byte && !r.is_high_byte() && r.num() >= 4)
        }
        Operand::Mem(m) => {
            m.base().is_some_and(|b| b.num() >= 8) || m.index().is_some_and(|i| i.num() >= 8)
        }
        _ => false,
    })
}

pub fn has_high_byte(ops: &[Operand]) -> bool {
    ops.iter()
        .any(|op| matches!(op, Operand::Reg(r) if r.is_high_byte()))
}

/// Encodes `inst` with `form` and checks every round-trip property.
/// Returns whether the operands were encodable with this form (rejections
/// must be the documented ones).
pub fn check_form(form: &Form, inst: &Inst) -> bool {
    match encode_form(form, inst) {
        Ok(bytes) => {
            let d = decode(&bytes).unwrap_or_else(|e| panic!("{form}: {inst} {bytes:?}: {e}"));
            assert_eq!(d.inst(), *inst, "{form}: {bytes:?}");
            assert_eq!(d.form(), form, "{inst}: {bytes:?}");
            assert_eq!(d.len(), bytes.len(), "{form}: {inst}");
            assert_eq!(d.flags(), bytes.flags(), "{form}: {inst}");
            // The free encoder accepts it too, decodes it the same, and is
            // never longer than this form.
            let best = encode(inst).unwrap_or_else(|e| panic!("{form}: {inst}: {e}"));
            assert!(
                best.len() <= bytes.len(),
                "{inst}: {best:?} longer than {bytes:?}"
            );
            assert_eq!(decode(&best).unwrap().inst(), *inst, "{inst}: {best:?}");
            true
        }
        Err(isa_lang::x86_64::EncodeError::HighByteWithRex) => {
            let forced = form.encoding().contains("REX.W");
            assert!(
                has_high_byte(inst.operands()) && (forced || operands_need_rex(inst.operands())),
                "{form}: {inst} wrongly refused"
            );
            false
        }
        Err(isa_lang::x86_64::EncodeError::NoForm { .. }) => {
            // Only the XCHG `+r` forms refuse a valid-looking operand: register 0.
            assert!(
                form.encoding().contains("90+r"),
                "{form}: {inst} wrongly refused"
            );
            assert!(
                matches!(inst.operands()[0], Operand::Reg(r) if r.num() == 0),
                "{form}: {inst}"
            );
            false
        }
        Err(e) => panic!("{form}: {inst}: unexpected {e}"),
    }
}
