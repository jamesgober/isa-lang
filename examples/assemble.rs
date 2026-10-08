//! The assembler: a function with a loop, labels, and a RIP-relative
//! constant, then its disassembly listing.
//!
//! ```text
//! cargo run --example assemble
//! ```
//!
//! The function is `u64 sum_scaled(const u32 *p, u64 n)` (System V: `p` in
//! `rdi`, `n` in `rsi`): the sum of `p[i] * SCALE` for `i < n`.

use isa_lang::x86_64::{
    AsmError, Assembler, Cond, Inst, Mem, Mnemonic, Reg, Scale, Size, disassemble,
};

fn main() -> Result<(), AsmError> {
    use Mnemonic::*;
    let mut asm = Assembler::new();
    let top = asm.label();
    let done = asm.label();
    let scale = asm.label();

    asm.emit(&Inst::new(Xor, [Reg::EAX.into(), Reg::EAX.into()]))?; // sum = 0
    asm.emit(&Inst::new(Xor, [Reg::ECX.into(), Reg::ECX.into()]))?; // i = 0
    asm.emit(&Inst::new(Test, [Reg::RSI.into(), Reg::RSI.into()]))?;
    asm.jcc(Cond::E, done)?;
    // r8 = SCALE, loaded from the constant after the code.
    asm.emit_rip(
        &Inst::new(Mov, [Reg::R8.into(), Mem::rip(Size::Qword, 0).into()]),
        scale,
    )?;
    asm.bind(top)?;
    let element = Mem::new(Size::Dword, Reg::RDI).with_index(Reg::RCX, Scale::X4);
    asm.emit(&Inst::new(Mov, [Reg::EDX.into(), element.into()]))?; // zero-extends into rdx
    asm.emit(&Inst::new(Imul, [Reg::RDX.into(), Reg::R8.into()]))?;
    asm.emit(&Inst::new(Add, [Reg::RAX.into(), Reg::RDX.into()]))?;
    asm.emit(&Inst::new(Inc, [Reg::RCX.into()]))?;
    asm.emit(&Inst::new(Cmp, [Reg::RCX.into(), Reg::RSI.into()]))?;
    asm.jcc(Cond::B, top)?;
    asm.bind(done)?;
    asm.emit(&Inst::new(Ret, []))?;
    asm.bind(scale)?;
    asm.bytes(&1000u64.to_le_bytes())?;

    let code = asm.finish()?;
    let end = code.label_offset(scale).unwrap_or(code.bytes().len());
    print!("{}", disassemble(&code.bytes()[..end], 0x1000));
    println!("{end} bytes of code, then the 8-byte constant");
    Ok(())
}
