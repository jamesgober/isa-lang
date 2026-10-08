//! x86-64 (long mode): the integer, control-flow, and memory core.
//!
//! The one-call path is [`encode`] and [`decode`]:
//!
//! ```
//! use isa_lang::x86_64::{decode, encode, Inst, Mnemonic, Operand, Reg};
//!
//! let inst = Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(8)]);
//! let bytes = encode(&inst)?;
//! assert_eq!(&*bytes, [0x48, 0x83, 0xC0, 0x08]);
//!
//! let back = decode(&bytes).unwrap();
//! assert_eq!(back.inst(), inst);
//! assert_eq!(back.inst().to_string(), "add rax, 0x8");
//! # Ok::<(), isa_lang::x86_64::EncodeError>(())
//! ```
//!
//! [`Assembler`] adds labels and branch relaxation on top, [`disassemble`]
//! lists code, and [`forms`] exposes the target-description table itself:
//! every form's encoding recipe, operand kinds, and flag effects, which is
//! what an instruction selector is generated from. [`encode_form`] emits one
//! specific form.
//!
//! # What is covered
//!
//! `mov` (register, memory, and immediate forms including `imm64`), `lea`,
//! `add` `or` `adc` `sbb` `and` `sub` `xor` `cmp` `test`, `inc` `dec` `neg`
//! `not`, `mul` `imul` (one, two, and three operands) `div` `idiv`, `cbw`
//! `cwde` `cdqe` `cwd` `cdq` `cqo`, `shl` `shr` `sar` `rol` `ror` (by 1,
//! `imm8`, `cl`), `movzx` `movsx` `movsxd`, `setcc`, `cmovcc`, `jcc`
//! (`rel8`/`rel32`), `jmp` (`rel8`/`rel32`/indirect), `call` (`rel32`/indirect),
//! `ret`, `push`, `pop`, `nop` (one-byte and `nop r/m`), `ud2`, `int3`, `xchg`,
//! and `bt`: 340 forms over 8-, 16-, 32-, and 64-bit operands where the
//! architecture has them. Floating point, SIMD, and system instructions are
//! not covered yet; the decoder reports them as [`DecodeError::UnknownOpcode`].

mod asm;
mod decode;
mod encode;
mod error;
mod flags;
mod fmt;
mod form;
mod inst;
mod nop;
mod operand;
mod reg;
mod table;

pub use asm::{AsmError, Assembler, Code, Label};
pub use decode::{Decoded, Disassembly, Instructions, decode, decode_all, disassemble};
pub use encode::{Encoded, MAX_INST_LEN, encode, encode_form};
pub use error::{DecodeError, EncodeError};
pub use flags::{FlagEffects, Flags};
pub use fmt::DisplayAt;
pub use form::{Form, OperandKind};
pub use inst::{Inst, MAX_OPERANDS, Mnemonic};
pub use nop::recommended_nop;
pub use operand::{Cond, Mem, Operand, Scale};
pub use reg::{Reg, Size};

/// Every instruction form in the x86-64 target description, in table order.
///
/// Within one mnemonic, table order is the encoder's preference (shorter
/// forms first). [`Form::index`] is a form's position here.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::forms;
///
/// let all = forms();
/// assert_eq!(all.len(), 340);
/// assert!(all.iter().any(|f| f.syntax() == "MOV r64, imm64" && f.encoding() == "REX.W B8+rd io"));
/// ```
#[must_use]
pub fn forms() -> &'static [Form] {
    &table::FORMS
}
