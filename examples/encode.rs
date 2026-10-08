//! The one-call path: encode instructions, decode them back, print both.
//!
//! ```text
//! cargo run --example encode
//! ```

use isa_lang::x86_64::{
    Cond, EncodeError, Inst, Mem, Mnemonic, Operand, Reg, Scale, Size, decode, encode,
};

fn main() -> Result<(), EncodeError> {
    let program = [
        Inst::new(Mnemonic::Push, [Reg::RBP.into()]),
        Inst::new(Mnemonic::Mov, [Reg::RBP.into(), Reg::RSP.into()]),
        Inst::new(Mnemonic::Sub, [Reg::RSP.into(), Operand::Imm(32)]),
        Inst::new(
            Mnemonic::Mov,
            [
                Reg::EAX.into(),
                Mem::new(Size::Dword, Reg::RDI)
                    .with_index(Reg::RCX, Scale::X4)
                    .into(),
            ],
        ),
        Inst::new(
            Mnemonic::Imul,
            [Reg::EAX.into(), Reg::EAX.into(), Operand::Imm(1000)],
        ),
        Inst::new(Mnemonic::Cmp, [Reg::EAX.into(), Operand::Imm(-1)]),
        Inst::new(Mnemonic::Setcc(Cond::G), [Reg::DL.into()]),
        Inst::new(Mnemonic::Movzx, [Reg::EDX.into(), Reg::DL.into()]),
        Inst::new(
            Mnemonic::Mov,
            [Reg::R11.into(), Operand::Imm(0x1122_3344_5566_7788)],
        ),
        Inst::new(Mnemonic::Ret, []),
    ];
    for inst in &program {
        let bytes = encode(inst)?;
        let back = decode(&bytes).map(|d| d.inst());
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        println!("{:<32} {:<34} {}", hex.join(" "), inst, bytes.form());
        assert_eq!(back, Ok(*inst));
    }
    Ok(())
}
