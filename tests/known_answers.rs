//! Known-answer tests: encodings checked byte for byte against the Intel SDM.
//!
//! Each case names the SDM opcode-table row it uses (Intel 64 and IA-32
//! Architectures Software Developer's Manual, Volume 2, the instruction's own
//! page) and spells out the bytes: REX = `0100WRXB`, ModRM = `mod reg rm`,
//! SIB = `scale index base`, then displacement and immediate little-endian.
//! Every case is also decoded, and its Intel-syntax text checked.
//!
//! The same instruction texts were assembled with LLVM's integrated
//! assembler and the same bytes disassembled with llvm-objdump 18.1.8 while
//! the crate was built (see `docs/release/v0.2.0.md`): the bytes agreed for
//! every case except `xchg eax, ebx` and `xchg cl, dl`, where LLVM swaps the
//! commutative operands (`93`, `86 ca`); the bytes here follow the SDM's
//! `XCHG r/m, r` operand order. That check is not part of this suite, which
//! depends on nothing outside the crate.

use isa_lang::x86_64::{
    Cond, Inst, Mem, Mnemonic, Mnemonic::*, Operand, Reg, Scale, Size, decode, encode,
};

fn r(reg: Reg) -> Operand {
    Operand::Reg(reg)
}
fn imm(v: i64) -> Operand {
    Operand::Imm(v)
}
fn rel(d: i64) -> Operand {
    Operand::Rel(d)
}
fn mem(size: Size, base: Reg, disp: i32) -> Operand {
    Operand::Mem(Mem::new(size, base).with_disp(disp))
}
fn sib(size: Size, base: Reg, index: Reg, scale: Scale, disp: i32) -> Operand {
    Operand::Mem(
        Mem::new(size, base)
            .with_index(index, scale)
            .with_disp(disp),
    )
}
fn rip(size: Size, disp: i32) -> Operand {
    Operand::Mem(Mem::rip(size, disp))
}

/// Encodes, decodes, and prints one instruction.
#[track_caller]
fn kat(m: Mnemonic, ops: &[Operand], bytes: &[u8], text: &str) {
    let inst = Inst::from_slice(m, ops).unwrap();
    let enc = encode(&inst).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(enc.as_bytes(), bytes, "{text}");
    let dec = decode(bytes).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(dec.inst(), inst, "{text}");
    assert_eq!(dec.len(), bytes.len(), "{text}");
    assert_eq!(inst.to_string(), text);
}

/// A non-canonical encoding: decodes to `text`, and re-encodes to `canonical`.
#[track_caller]
fn decodes_as(bytes: &[u8], text: &str, canonical: &[u8]) {
    let dec = decode(bytes).unwrap_or_else(|e| panic!("{bytes:02x?}: {e}"));
    assert_eq!(dec.len(), bytes.len(), "{text}");
    assert_eq!(dec.inst().to_string(), text);
    assert_eq!(encode(&dec.inst()).unwrap().as_bytes(), canonical, "{text}");
}

use Reg as R;
use Size::{Byte as B8, Dword as D32, Qword as Q64, Word as W16};

#[test]
fn arithmetic_and_logic() {
    // ADD r/m64, r64 = REX.W 01 /r. ModRM 11 011(rbx) 000(rax) = D8.
    kat(
        Add,
        &[r(R::RAX), r(R::RBX)],
        &[0x48, 0x01, 0xD8],
        "add rax, rbx",
    );
    // ADD r/m32, r32 = 01 /r.
    kat(Add, &[r(R::EAX), r(R::EBX)], &[0x01, 0xD8], "add eax, ebx");
    // REX 0100 1101: W, R (r9 in reg), B (r8 in rm). ModRM 11 001 000 = C8.
    kat(
        Add,
        &[r(R::R8), r(R::R9)],
        &[0x4D, 0x01, 0xC8],
        "add r8, r9",
    );
    // ADD AL, imm8 = 04 ib.
    kat(Add, &[r(R::AL), imm(5)], &[0x04, 0x05], "add al, 0x5");
    // ADD AX, imm16 = 66 05 iw (the accumulator form beats 66 81 /0 iw).
    kat(
        Add,
        &[r(R::AX), imm(0x1234)],
        &[0x66, 0x05, 0x34, 0x12],
        "add ax, 0x1234",
    );
    // ADD EAX, imm32 = 05 id.
    kat(
        Add,
        &[r(R::EAX), imm(0x1234_5678)],
        &[0x05, 0x78, 0x56, 0x34, 0x12],
        "add eax, 0x12345678",
    );
    // ADD r/m64, imm8 = REX.W 83 /0 ib. ModRM 11 000 000 = C0.
    kat(
        Add,
        &[r(R::RAX), imm(8)],
        &[0x48, 0x83, 0xC0, 0x08],
        "add rax, 0x8",
    );
    kat(
        Add,
        &[r(R::RAX), imm(-1)],
        &[0x48, 0x83, 0xC0, 0xFF],
        "add rax, -0x1",
    );
    // 0x80 does not fit a signed imm8: ADD RAX, imm32 = REX.W 05 id (6 bytes)
    // beats REX.W 81 /0 id (7 bytes).
    kat(
        Add,
        &[r(R::RAX), imm(0x80)],
        &[0x48, 0x05, 0x80, 0, 0, 0],
        "add rax, 0x80",
    );
    // Not the accumulator: REX.W 81 /0 id, ModRM 11 000 001 = C1.
    kat(
        Add,
        &[r(R::RCX), imm(0x80)],
        &[0x48, 0x81, 0xC1, 0x80, 0, 0, 0],
        "add rcx, 0x80",
    );
    // ADD r/m32, imm8 = 83 /0 ib, ModRM 00 000 000.
    kat(
        Add,
        &[mem(D32, R::RAX, 0), imm(1)],
        &[0x83, 0x00, 0x01],
        "add dword ptr [rax], 0x1",
    );
    // ADD r/m8, imm8 = 80 /0 ib.
    kat(
        Add,
        &[mem(B8, R::RAX, 0), imm(1)],
        &[0x80, 0x00, 0x01],
        "add byte ptr [rax], 0x1",
    );
    // RSP base needs SIB: ModRM 01 000 100 = 44, SIB 00 100(none) 100(rsp) = 24, disp8.
    kat(
        Add,
        &[mem(Q64, R::RSP, 8), r(R::RAX)],
        &[0x48, 0x01, 0x44, 0x24, 0x08],
        "add qword ptr [rsp+0x8], rax",
    );
    // ADD r64, r/m64 = REX.W 03 /r. ModRM 01 000 101(rbp) = 45, disp8 F8.
    kat(
        Add,
        &[r(R::RAX), mem(Q64, R::RBP, -8)],
        &[0x48, 0x03, 0x45, 0xF8],
        "add rax, qword ptr [rbp-0x8]",
    );
    // OR r/m32, r32 = 09 /r. ModRM 11 010(edx) 001(ecx) = D1.
    kat(Or, &[r(R::ECX), r(R::EDX)], &[0x09, 0xD1], "or ecx, edx");
    // ADC r/m32, r32 = 11 /r. ModRM 11 001 000 = C8.
    kat(Adc, &[r(R::EAX), r(R::ECX)], &[0x11, 0xC8], "adc eax, ecx");
    // SBB r/m64, imm8 = REX.W 83 /3 ib. ModRM 11 011 010 = DA.
    kat(
        Sbb,
        &[r(R::RDX), imm(1)],
        &[0x48, 0x83, 0xDA, 0x01],
        "sbb rdx, 0x1",
    );
    // 255 needs imm32: AND r/m32, imm32 = 81 /4 id. ModRM 11 100 110 = E6.
    kat(
        And,
        &[r(R::ESI), imm(0xFF)],
        &[0x81, 0xE6, 0xFF, 0, 0, 0],
        "and esi, 0xff",
    );
    // AND AL, imm8 = 24 ib.
    kat(And, &[r(R::AL), imm(0x0F)], &[0x24, 0x0F], "and al, 0xf");
    // SUB r/m64, imm8 = REX.W 83 /5 ib. ModRM 11 101 100 = EC.
    kat(
        Sub,
        &[r(R::RSP), imm(40)],
        &[0x48, 0x83, 0xEC, 0x28],
        "sub rsp, 0x28",
    );
    // XOR r/m32, r32 = 31 /r.
    kat(Xor, &[r(R::EAX), r(R::EAX)], &[0x31, 0xC0], "xor eax, eax");
    // REX 0100 0101 (R, B). ModRM 11 111 111 = FF.
    kat(
        Xor,
        &[r(R::R15D), r(R::R15D)],
        &[0x45, 0x31, 0xFF],
        "xor r15d, r15d",
    );
    // CMP r/m64, r64 = REX.W 39 /r. ModRM 11 110(rsi) 111(rdi) = F7.
    kat(
        Cmp,
        &[r(R::RDI), r(R::RSI)],
        &[0x48, 0x39, 0xF7],
        "cmp rdi, rsi",
    );
    // CMP r/m8, imm8 = 80 /7 ib. ModRM 00 111 111 = 3F.
    kat(
        Cmp,
        &[mem(B8, R::RDI, 0), imm(0)],
        &[0x80, 0x3F, 0x00],
        "cmp byte ptr [rdi], 0x0",
    );
    // CMP AL, imm8 = 3C ib.
    kat(Cmp, &[r(R::AL), imm(-128)], &[0x3C, 0x80], "cmp al, -0x80");
    // TEST r/m32, r32 = 85 /r.
    kat(
        Test,
        &[r(R::EAX), r(R::EAX)],
        &[0x85, 0xC0],
        "test eax, eax",
    );
    // TEST AL, imm8 = A8 ib.
    kat(Test, &[r(R::AL), imm(1)], &[0xA8, 0x01], "test al, 0x1");
    // TEST RAX, imm32 = REX.W A9 id.
    kat(
        Test,
        &[r(R::RAX), imm(0x100)],
        &[0x48, 0xA9, 0x00, 0x01, 0, 0],
        "test rax, 0x100",
    );
    // TEST r/m32, imm32 = F7 /0 id. ModRM 11 000 001 = C1.
    kat(
        Test,
        &[r(R::ECX), imm(0x100)],
        &[0xF7, 0xC1, 0x00, 0x01, 0, 0],
        "test ecx, 0x100",
    );
    // TEST r/m8, imm8 = F6 /0 ib. ModRM 01 000 000 = 40, disp8 1.
    kat(
        Test,
        &[mem(B8, R::RAX, 1), imm(1)],
        &[0xF6, 0x40, 0x01, 0x01],
        "test byte ptr [rax+0x1], 0x1",
    );
    // TEST r/m8, r8 = 84 /r with REX 0100 0101.
    kat(
        Test,
        &[r(R::R8B), r(R::R8B)],
        &[0x45, 0x84, 0xC0],
        "test r8b, r8b",
    );
}

#[test]
fn unary_multiply_divide() {
    // INC r/m64 = REX.W FF /0. ModRM 11 000 000.
    kat(Inc, &[r(R::RAX)], &[0x48, 0xFF, 0xC0], "inc rax");
    // DEC r/m32 = FF /1. ModRM 11 001 001 = C9.
    kat(Dec, &[r(R::ECX)], &[0xFF, 0xC9], "dec ecx");
    // INC r/m8 = FE /0. ModRM 00 000 011 = 03.
    kat(
        Inc,
        &[mem(B8, R::RBX, 0)],
        &[0xFE, 0x03],
        "inc byte ptr [rbx]",
    );
    // NEG r/m64 = REX.W F7 /3. ModRM 11 011 000 = D8.
    kat(Neg, &[r(R::RAX)], &[0x48, 0xF7, 0xD8], "neg rax");
    // NOT r/m32 = F7 /2, REX.B. ModRM 11 010 010 = D2.
    kat(Not, &[r(R::R10D)], &[0x41, 0xF7, 0xD2], "not r10d");
    // NEG r/m16 = 66 F7 /3. ModRM 00 011 000 = 18.
    kat(
        Neg,
        &[mem(W16, R::RAX, 0)],
        &[0x66, 0xF7, 0x18],
        "neg word ptr [rax]",
    );
    // MUL r/m64 = REX.W F7 /4. ModRM 11 100 001 = E1.
    kat(Mul, &[r(R::RCX)], &[0x48, 0xF7, 0xE1], "mul rcx");
    // IMUL r/m64 = REX.W F7 /5. ModRM 11 101 001 = E9.
    kat(Imul, &[r(R::RCX)], &[0x48, 0xF7, 0xE9], "imul rcx");
    // DIV r/m64 = REX.W F7 /6, REX.B. ModRM 11 110 000 = F0.
    kat(Div, &[r(R::R8)], &[0x49, 0xF7, 0xF0], "div r8");
    // IDIV r/m32 = F7 /7. ModRM 00 111 100 = 3C, SIB 24.
    kat(
        Idiv,
        &[mem(D32, R::RSP, 0)],
        &[0xF7, 0x3C, 0x24],
        "idiv dword ptr [rsp]",
    );
    // DIV r/m8 = F6 /6. ModRM 11 110 011 = F3.
    kat(Div, &[r(R::BL)], &[0xF6, 0xF3], "div bl");
    // IMUL r64, r/m64 = REX.W 0F AF /r. ModRM 11 000(rax) 011(rbx) = C3.
    kat(
        Imul,
        &[r(R::RAX), r(R::RBX)],
        &[0x48, 0x0F, 0xAF, 0xC3],
        "imul rax, rbx",
    );
    // IMUL r32, r/m32, imm8 = 6B /r ib. ModRM 00 000 111 = 07.
    kat(
        Imul,
        &[r(R::EAX), mem(D32, R::RDI, 0), imm(10)],
        &[0x6B, 0x07, 0x0A],
        "imul eax, dword ptr [rdi], 0xa",
    );
    // IMUL r64, r/m64, imm32 = REX.W 69 /r id. ModRM 11 010 001 = D1.
    kat(
        Imul,
        &[r(R::RDX), r(R::RCX), imm(1000)],
        &[0x48, 0x69, 0xD1, 0xE8, 0x03, 0, 0],
        "imul rdx, rcx, 0x3e8",
    );
    // IMUL r16, r/m16, imm16 = 66 69 /r iw. ModRM 11 000 011 = C3.
    kat(
        Imul,
        &[r(R::AX), r(R::BX), imm(300)],
        &[0x66, 0x69, 0xC3, 0x2C, 0x01],
        "imul ax, bx, 0x12c",
    );
    // CBW/CWDE/CDQE = 98 at 16/32/64; CWD/CDQ/CQO = 99.
    kat(Cbw, &[], &[0x66, 0x98], "cbw");
    kat(Cwde, &[], &[0x98], "cwde");
    kat(Cdqe, &[], &[0x48, 0x98], "cdqe");
    kat(Cwd, &[], &[0x66, 0x99], "cwd");
    kat(Cdq, &[], &[0x99], "cdq");
    kat(Cqo, &[], &[0x48, 0x99], "cqo");
}

#[test]
fn shifts_and_rotates() {
    // SHL r/m64, 1 = REX.W D1 /4. ModRM 11 100 000 = E0.
    kat(
        Shl,
        &[r(R::RAX), imm(1)],
        &[0x48, 0xD1, 0xE0],
        "shl rax, 0x1",
    );
    // SHL r/m64, imm8 = REX.W C1 /4 ib.
    kat(
        Shl,
        &[r(R::RAX), imm(4)],
        &[0x48, 0xC1, 0xE0, 0x04],
        "shl rax, 0x4",
    );
    // SHR r/m32, CL = D3 /5. ModRM 11 101 001 = E9.
    kat(Shr, &[r(R::ECX), r(R::CL)], &[0xD3, 0xE9], "shr ecx, cl");
    // SAR r/m64, imm8 = REX.W C1 /7 ib. ModRM 11 111 010 = FA.
    kat(
        Sar,
        &[r(R::RDX), imm(63)],
        &[0x48, 0xC1, 0xFA, 0x3F],
        "sar rdx, 0x3f",
    );
    // ROL r/m8, 1 = D0 /0. ModRM 11 000 000.
    kat(Rol, &[r(R::AL), imm(1)], &[0xD0, 0xC0], "rol al, 0x1");
    // ROR r/m16, imm8 = 66 C1 /1 ib, REX.B. ModRM 11 001 001 = C9.
    kat(
        Ror,
        &[r(R::R9W), imm(8)],
        &[0x66, 0x41, 0xC1, 0xC9, 0x08],
        "ror r9w, 0x8",
    );
    // SHL r/m8, CL = D2 /4. ModRM 00 100 000 = 20.
    kat(
        Shl,
        &[mem(B8, R::RAX, 0), r(R::CL)],
        &[0xD2, 0x20],
        "shl byte ptr [rax], cl",
    );
    // Unsigned count: 255 is a valid imm8 count.
    kat(
        Shr,
        &[r(R::EAX), imm(255)],
        &[0xC1, 0xE8, 0xFF],
        "shr eax, 0xff",
    );
}

#[test]
fn moves_and_addressing() {
    // MOV r/m64, r64 = REX.W 89 /r.
    kat(
        Mov,
        &[r(R::RAX), r(R::RBX)],
        &[0x48, 0x89, 0xD8],
        "mov rax, rbx",
    );
    // MOV r32, imm32 = B8+rd id.
    kat(
        Mov,
        &[r(R::EAX), imm(1)],
        &[0xB8, 0x01, 0, 0, 0],
        "mov eax, 0x1",
    );
    // MOV r/m64, imm32 = REX.W C7 /0 id (sign-extended), shorter than B8+rd io.
    kat(
        Mov,
        &[r(R::RAX), imm(1)],
        &[0x48, 0xC7, 0xC0, 0x01, 0, 0, 0],
        "mov rax, 0x1",
    );
    // MOV r64, imm64 = REX.W B8+rd io.
    kat(
        Mov,
        &[r(R::RAX), imm(0x1122_3344_5566_7788)],
        &[0x48, 0xB8, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11],
        "mov rax, 0x1122334455667788",
    );
    // i32::MIN still sign-extends: REX.WB C7 /0, ModRM 11 000 111 = C7.
    kat(
        Mov,
        &[r(R::R15), imm(-0x8000_0000)],
        &[0x49, 0xC7, 0xC7, 0, 0, 0, 0x80],
        "mov r15, -0x80000000",
    );
    // 0x80000000 does not: REX.WB B8+7 io.
    kat(
        Mov,
        &[r(R::R15), imm(0x8000_0000)],
        &[0x49, 0xBF, 0, 0, 0, 0x80, 0, 0, 0, 0],
        "mov r15, 0x80000000",
    );
    // MOV r8, imm8 = B0+rb ib; R12B = REX.B + B0+4.
    kat(Mov, &[r(R::AL), imm(0x7F)], &[0xB0, 0x7F], "mov al, 0x7f");
    kat(
        Mov,
        &[r(R::R12B), imm(1)],
        &[0x41, 0xB4, 0x01],
        "mov r12b, 0x1",
    );
    // SIL needs an empty REX (0x40) to mean SIL and not DH.
    kat(
        Mov,
        &[r(R::SIL), imm(1)],
        &[0x40, 0xB6, 0x01],
        "mov sil, 0x1",
    );
    // MOV r16, imm16 = 66 B8+rw iw.
    kat(
        Mov,
        &[r(R::AX), imm(0x1234)],
        &[0x66, 0xB8, 0x34, 0x12],
        "mov ax, 0x1234",
    );
    // MOV r/m64, r64: ModRM 01 111(rdi) 100 = 7C, SIB 24, disp8 8.
    kat(
        Mov,
        &[mem(Q64, R::RSP, 8), r(R::RDI)],
        &[0x48, 0x89, 0x7C, 0x24, 0x08],
        "mov qword ptr [rsp+0x8], rdi",
    );
    // MOV r/m32, imm32 = C7 /0 id. ModRM 01 000 101 = 45, disp8 FC.
    kat(
        Mov,
        &[mem(D32, R::RBP, -4), imm(0)],
        &[0xC7, 0x45, 0xFC, 0, 0, 0, 0],
        "mov dword ptr [rbp-0x4], 0x0",
    );
    // MOV r/m8, imm8 = C6 /0 ib.
    kat(
        Mov,
        &[mem(B8, R::RAX, 0), imm(0x41)],
        &[0xC6, 0x00, 0x41],
        "mov byte ptr [rax], 0x41",
    );
    // MOV r/m16, imm16 = 66 C7 /0 iw.
    kat(
        Mov,
        &[mem(W16, R::RAX, 0), imm(0x1234)],
        &[0x66, 0xC7, 0x00, 0x34, 0x12],
        "mov word ptr [rax], 0x1234",
    );
    // RIP-relative: ModRM 00 000 101 = 05, disp32.
    kat(
        Mov,
        &[r(R::RAX), rip(Q64, 0x100)],
        &[0x48, 0x8B, 0x05, 0x00, 0x01, 0, 0],
        "mov rax, qword ptr [rip+0x100]",
    );
    // Absolute disp32 needs SIB with no base and no index: ModRM 00 000 100,
    // SIB 00 100 101 = 25 (mod=00 rm=101 alone would be RIP-relative).
    kat(
        Mov,
        &[r(R::EAX), Operand::Mem(Mem::absolute(D32, 0x1000))],
        &[0x8B, 0x04, 0x25, 0x00, 0x10, 0, 0],
        "mov eax, dword ptr [0x1000]",
    );
    // SIB 11 001(rcx) 000(rax) = C8.
    kat(
        Mov,
        &[r(R::RAX), sib(Q64, R::RAX, R::RCX, Scale::X8, 0)],
        &[0x48, 0x8B, 0x04, 0xC8],
        "mov rax, qword ptr [rax+rcx*8]",
    );
    // R12 base also needs SIB (low bits 100): REX.WB, SIB 24.
    kat(
        Mov,
        &[r(R::RAX), mem(Q64, R::R12, 0)],
        &[0x49, 0x8B, 0x04, 0x24],
        "mov rax, qword ptr [r12]",
    );
    // R13 base has no mod=00 form (low bits 101): disp8 0.
    kat(
        Mov,
        &[r(R::RAX), mem(Q64, R::R13, 0)],
        &[0x49, 0x8B, 0x45, 0x00],
        "mov rax, qword ptr [r13]",
    );
    // REX 0100 1010 (W, X); ModRM 10 000 100 = 84; SIB 10 100(r12) 101(rbp) = A5.
    kat(
        Mov,
        &[r(R::RAX), sib(Q64, R::RBP, R::R12, Scale::X4, 0x1234_5678)],
        &[0x4A, 0x8B, 0x84, 0xA5, 0x78, 0x56, 0x34, 0x12],
        "mov rax, qword ptr [rbp+r12*4+0x12345678]",
    );
    // Index without base: ModRM 00 010 100 = 14; SIB 01 001 101 = 4D; disp32 0.
    kat(
        Mov,
        &[
            r(R::RDX),
            Operand::Mem(Mem::absolute(Q64, 0).with_index(R::RCX, Scale::X2)),
        ],
        &[0x48, 0x8B, 0x14, 0x4D, 0, 0, 0, 0],
        "mov rdx, qword ptr [rcx*2]",
    );
    // High-byte register without REX: MOV r/m8, r8 = 88 /r, ModRM 11 011(bl) 100(ah) = DC.
    kat(Mov, &[r(R::AH), r(R::BL)], &[0x88, 0xDC], "mov ah, bl");
    // MOVZX r32, r/m8 = 0F B6 /r.
    kat(
        Movzx,
        &[r(R::EAX), r(R::AL)],
        &[0x0F, 0xB6, 0xC0],
        "movzx eax, al",
    );
    kat(
        Movzx,
        &[r(R::EAX), mem(B8, R::RDI, 0)],
        &[0x0F, 0xB6, 0x07],
        "movzx eax, byte ptr [rdi]",
    );
    // MOVZX r64, r/m16 = REX.W 0F B7 /r. ModRM 00 000 110 = 06.
    kat(
        Movzx,
        &[r(R::RAX), mem(W16, R::RSI, 0)],
        &[0x48, 0x0F, 0xB7, 0x06],
        "movzx rax, word ptr [rsi]",
    );
    // SIL source needs an empty REX. ModRM 11 001 110 = CE.
    kat(
        Movzx,
        &[r(R::ECX), r(R::SIL)],
        &[0x40, 0x0F, 0xB6, 0xCE],
        "movzx ecx, sil",
    );
    // MOVZX r16, r/m8 = 66 0F B6 /r.
    kat(
        Movzx,
        &[r(R::AX), r(R::CL)],
        &[0x66, 0x0F, 0xB6, 0xC1],
        "movzx ax, cl",
    );
    // MOVSX r64, r/m8 = REX.W 0F BE /r.
    kat(
        Movsx,
        &[r(R::RAX), r(R::AL)],
        &[0x48, 0x0F, 0xBE, 0xC0],
        "movsx rax, al",
    );
    // MOVSX r32, r/m16 = 0F BF /r.
    kat(
        Movsx,
        &[r(R::EAX), r(R::CX)],
        &[0x0F, 0xBF, 0xC1],
        "movsx eax, cx",
    );
    // MOVSXD r64, r/m32 = REX.W 63 /r.
    kat(
        Movsxd,
        &[r(R::RAX), r(R::ECX)],
        &[0x48, 0x63, 0xC1],
        "movsxd rax, ecx",
    );
    kat(
        Movsxd,
        &[r(R::RDX), mem(D32, R::RDI, 4)],
        &[0x48, 0x63, 0x57, 0x04],
        "movsxd rdx, dword ptr [rdi+0x4]",
    );
    // LEA r64, m = REX.W 8D /r.
    kat(
        Lea,
        &[r(R::RAX), rip(Q64, 0x10)],
        &[0x48, 0x8D, 0x05, 0x10, 0, 0, 0],
        "lea rax, [rip+0x10]",
    );
    // SIB 01 000 000 = 40.
    kat(
        Lea,
        &[r(R::RAX), sib(Q64, R::RAX, R::RAX, Scale::X2, 0)],
        &[0x48, 0x8D, 0x04, 0x40],
        "lea rax, [rax+rax*2]",
    );
    // LEA r32, m: the address is still 64-bit. ModRM 01 001 111 = 4F.
    kat(
        Lea,
        &[r(R::ECX), mem(D32, R::RDI, 8)],
        &[0x8D, 0x4F, 0x08],
        "lea ecx, [rdi+0x8]",
    );
    // REX.WR, ModRM 10 000 100 = 84, SIB 24, disp32.
    kat(
        Lea,
        &[r(R::R8), mem(Q64, R::RSP, 0x100)],
        &[0x4C, 0x8D, 0x84, 0x24, 0x00, 0x01, 0, 0],
        "lea r8, [rsp+0x100]",
    );
}

#[test]
fn conditions() {
    // SETcc r/m8 = 0F 90+cc; E = 4.
    kat(Setcc(Cond::E), &[r(R::AL)], &[0x0F, 0x94, 0xC0], "sete al");
    kat(
        Setcc(Cond::Ne),
        &[r(R::R8B)],
        &[0x41, 0x0F, 0x95, 0xC0],
        "setne r8b",
    );
    kat(
        Setcc(Cond::L),
        &[mem(B8, R::RDI, 0)],
        &[0x0F, 0x9C, 0x07],
        "setl byte ptr [rdi]",
    );
    kat(
        Setcc(Cond::G),
        &[r(R::SIL)],
        &[0x40, 0x0F, 0x9F, 0xC6],
        "setg sil",
    );
    // CMOVcc r32, r/m32 = 0F 40+cc /r; L = C.
    kat(
        Cmovcc(Cond::L),
        &[r(R::EAX), r(R::ECX)],
        &[0x0F, 0x4C, 0xC1],
        "cmovl eax, ecx",
    );
    kat(
        Cmovcc(Cond::A),
        &[r(R::RAX), mem(Q64, R::RBX, 0)],
        &[0x48, 0x0F, 0x47, 0x03],
        "cmova rax, qword ptr [rbx]",
    );
    // 66, REX.R; ModRM 11 001 000 = C8.
    kat(
        Cmovcc(Cond::Ns),
        &[r(R::R9W), r(R::AX)],
        &[0x66, 0x44, 0x0F, 0x49, 0xC8],
        "cmovns r9w, ax",
    );
    // All sixteen condition codes, in hardware order.
    let names = [
        "o", "no", "b", "ae", "e", "ne", "be", "a", "s", "ns", "p", "np", "l", "ge", "le", "g",
    ];
    for (code, name) in names.iter().enumerate() {
        let c = Cond::from_code(code as u8).unwrap();
        kat(
            Jcc(c),
            &[rel(2)],
            &[0x70 + code as u8, 0x00],
            &format!("j{name} $+0x2"),
        );
        kat(
            Setcc(c),
            &[r(R::CL)],
            &[0x0F, 0x90 + code as u8, 0xC1],
            &format!("set{name} cl"),
        );
    }
}

#[test]
fn control_flow() {
    // Jcc rel8 = 70+cc cb; the field is relative to the next instruction.
    kat(Jcc(Cond::E), &[rel(0x10)], &[0x74, 0x0E], "je $+0x10");
    kat(Jcc(Cond::Ne), &[rel(-126)], &[0x75, 0x80], "jne $-0x7e");
    // Jcc rel32 = 0F 80+cc cd; 0x100 - 6 = 0xFA.
    kat(
        Jcc(Cond::L),
        &[rel(0x100)],
        &[0x0F, 0x8C, 0xFA, 0, 0, 0],
        "jl $+0x100",
    );
    // JMP rel8 = EB cb, JMP rel32 = E9 cd.
    kat(Jmp, &[rel(2)], &[0xEB, 0x00], "jmp $+0x2");
    kat(
        Jmp,
        &[rel(0x1000)],
        &[0xE9, 0xFB, 0x0F, 0, 0],
        "jmp $+0x1000",
    );
    // JMP r/m64 = FF /4. ModRM 11 100 000 = E0.
    kat(Jmp, &[r(R::RAX)], &[0xFF, 0xE0], "jmp rax");
    kat(Jmp, &[r(R::R11)], &[0x41, 0xFF, 0xE3], "jmp r11");
    // ModRM 00 100 101 = 25: RIP-relative.
    kat(
        Jmp,
        &[rip(Q64, 0x20)],
        &[0xFF, 0x25, 0x20, 0, 0, 0],
        "jmp qword ptr [rip+0x20]",
    );
    // CALL rel32 = E8 cd.
    kat(
        Call,
        &[rel(0x105)],
        &[0xE8, 0x00, 0x01, 0, 0],
        "call $+0x105",
    );
    // CALL r/m64 = FF /2. ModRM 11 010 000 = D0; 01 010 000 = 50.
    kat(Call, &[r(R::RAX)], &[0xFF, 0xD0], "call rax");
    kat(
        Call,
        &[mem(Q64, R::RAX, 8)],
        &[0xFF, 0x50, 0x08],
        "call qword ptr [rax+0x8]",
    );
    // RET = C3, RET imm16 = C2 iw.
    kat(Ret, &[], &[0xC3], "ret");
    kat(Ret, &[imm(16)], &[0xC2, 0x10, 0x00], "ret 0x10");
    // PUSH r64 = 50+rd, POP r64 = 58+rd; R8-R15 add REX.B.
    kat(Push, &[r(R::RBP)], &[0x55], "push rbp");
    kat(Push, &[r(R::R12)], &[0x41, 0x54], "push r12");
    kat(Pop, &[r(R::R15)], &[0x41, 0x5F], "pop r15");
    kat(Pop, &[r(R::RBX)], &[0x5B], "pop rbx");
    // PUSH r/m64 = FF /6 (ModRM 00 110 000 = 30); POP r/m64 = 8F /0.
    kat(
        Push,
        &[mem(Q64, R::RAX, 0)],
        &[0xFF, 0x30],
        "push qword ptr [rax]",
    );
    kat(
        Pop,
        &[mem(Q64, R::RSP, 8)],
        &[0x8F, 0x44, 0x24, 0x08],
        "pop qword ptr [rsp+0x8]",
    );
    // PUSH imm8 = 6A ib, PUSH imm32 = 68 id (both sign-extended to 64).
    kat(Push, &[imm(1)], &[0x6A, 0x01], "push 0x1");
    kat(Push, &[imm(-1)], &[0x6A, 0xFF], "push -0x1");
    kat(
        Push,
        &[imm(0x1000)],
        &[0x68, 0x00, 0x10, 0, 0],
        "push 0x1000",
    );
}

#[test]
fn exchange_bits_and_misc() {
    // XCHG r64, RAX = REX.W 90+rd.
    kat(
        Xchg,
        &[r(R::RBX), r(R::RAX)],
        &[0x48, 0x93],
        "xchg rbx, rax",
    );
    kat(
        Xchg,
        &[r(R::R8D), r(R::EAX)],
        &[0x41, 0x90],
        "xchg r8d, eax",
    );
    kat(Xchg, &[r(R::CX), r(R::AX)], &[0x66, 0x91], "xchg cx, ax");
    // XCHG r/m32, r32 = 87 /r. ModRM 11 011(ebx) 000(eax) = D8.
    kat(
        Xchg,
        &[r(R::EAX), r(R::EBX)],
        &[0x87, 0xD8],
        "xchg eax, ebx",
    );
    // 90 would be NOP, so XCHG EAX, EAX takes 87 /r.
    kat(
        Xchg,
        &[r(R::EAX), r(R::EAX)],
        &[0x87, 0xC0],
        "xchg eax, eax",
    );
    kat(
        Xchg,
        &[mem(Q64, R::RDI, 0), r(R::RSI)],
        &[0x48, 0x87, 0x37],
        "xchg qword ptr [rdi], rsi",
    );
    // XCHG r/m8, r8 = 86 /r.
    kat(Xchg, &[r(R::CL), r(R::DL)], &[0x86, 0xD1], "xchg cl, dl");
    // BT r/m32, r32 = 0F A3 /r.
    kat(
        Bt,
        &[r(R::EAX), r(R::ECX)],
        &[0x0F, 0xA3, 0xC8],
        "bt eax, ecx",
    );
    // BT r/m64, imm8 = REX.W 0F BA /4 ib. ModRM 00 100 111 = 27.
    kat(
        Bt,
        &[mem(Q64, R::RDI, 0), imm(63)],
        &[0x48, 0x0F, 0xBA, 0x27, 0x3F],
        "bt qword ptr [rdi], 0x3f",
    );
    // 66, REX.B, ModRM 11 100 001 = E1.
    kat(
        Bt,
        &[r(R::R9W), imm(3)],
        &[0x66, 0x41, 0x0F, 0xBA, 0xE1, 0x03],
        "bt r9w, 0x3",
    );
    // NOP = 90; NOP r/m32 = 0F 1F /0.
    kat(Nop, &[], &[0x90], "nop");
    kat(
        Nop,
        &[mem(D32, R::RAX, 0)],
        &[0x0F, 0x1F, 0x00],
        "nop dword ptr [rax]",
    );
    kat(
        Nop,
        &[sib(W16, R::RAX, R::RAX, Scale::X1, 0)],
        &[0x66, 0x0F, 0x1F, 0x04, 0x00],
        "nop word ptr [rax+rax*1]",
    );
    kat(Ud2, &[], &[0x0F, 0x0B], "ud2");
    kat(Int3, &[], &[0xCC], "int3");
}

#[test]
fn non_canonical_encodings_decode_to_the_same_instruction() {
    // ADD r/m64, imm32 with a value that fits imm8.
    decodes_as(
        &[0x48, 0x81, 0xC0, 0x08, 0, 0, 0],
        "add rax, 0x8",
        &[0x48, 0x83, 0xC0, 0x08],
    );
    // ADD r32, r/m32 (03 /r) instead of 01 /r.
    decodes_as(&[0x03, 0xC3], "add eax, ebx", &[0x01, 0xD8]);
    decodes_as(&[0x8B, 0xC3], "mov eax, ebx", &[0x89, 0xD8]);
    // SHL by an explicit imm8 of 1.
    decodes_as(&[0xC1, 0xE0, 0x01], "shl eax, 0x1", &[0xD1, 0xE0]);
    // MOV r64, imm64 with a small value.
    decodes_as(
        &[0x48, 0xB8, 5, 0, 0, 0, 0, 0, 0, 0],
        "mov rax, 0x5",
        &[0x48, 0xC7, 0xC0, 5, 0, 0, 0],
    );
    // Redundant empty REX; redundant REX.W on a default-64 form.
    decodes_as(&[0x40, 0x01, 0xC3], "add ebx, eax", &[0x01, 0xC3]);
    decodes_as(&[0x48, 0xFF, 0xE0], "jmp rax", &[0xFF, 0xE0]);
    // RSP base through a redundant disp8 of 0, and a SIB with no index.
    decodes_as(
        &[0x8B, 0x44, 0x24, 0x00],
        "mov eax, dword ptr [rsp]",
        &[0x8B, 0x04, 0x24],
    );
    decodes_as(
        &[0x8B, 0x04, 0x20],
        "mov eax, dword ptr [rax]",
        &[0x8B, 0x00],
    );
    // The recommended 5-byte NOP has a zero disp8 the encoder would drop.
    decodes_as(
        &[0x0F, 0x1F, 0x44, 0x00, 0x00],
        "nop dword ptr [rax+rax*1]",
        &[0x0F, 0x1F, 0x04, 0x00],
    );
    // 66 90 and REX.W 90 are NOP, not XCHG.
    decodes_as(&[0x66, 0x90], "nop", &[0x90]);
    decodes_as(&[0x48, 0x90], "nop", &[0x90]);
}
