//! # isa_lang
//!
//! Machine instructions to bytes and back, driven by target-description
//! tables. Each instruction form is one row of data — an encoding recipe and
//! an operand signature written as the architecture manual writes them — and
//! one table-driven encoder, decoder, and Intel-syntax disassembler interpret
//! the rows. Adding a form is adding a row.
//!
//! This release covers the x86-64 integer, control-flow, and memory core in
//! [`x86_64`]. AArch64 follows in the same model (see the roadmap).
//!
//! ## Quick start
//!
//! ```
//! use isa_lang::x86_64::{decode, encode, Inst, Mem, Mnemonic, Reg, Size};
//!
//! // mov rax, qword ptr [rdi+0x10]
//! let load = Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Mem::new(Size::Qword, Reg::RDI).with_disp(16).into()]);
//! let bytes = encode(&load)?;
//! assert_eq!(&*bytes, [0x48, 0x8B, 0x47, 0x10]);
//!
//! let decoded = decode(&bytes).unwrap();
//! assert_eq!(decoded.inst(), load);
//! assert_eq!(decoded.inst().to_string(), "mov rax, qword ptr [rdi+0x10]");
//! # Ok::<(), isa_lang::x86_64::EncodeError>(())
//! ```
//!
//! With labels:
//!
//! ```
//! use isa_lang::x86_64::{Assembler, Cond, Inst, Mnemonic, Operand, Reg};
//!
//! let mut asm = Assembler::new();
//! let positive = asm.label();
//! asm.emit(&Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Reg::EDI.into()]))?;
//! asm.emit(&Inst::new(Mnemonic::Test, [Reg::EAX.into(), Reg::EAX.into()]))?;
//! asm.jcc(Cond::Ns, positive)?;
//! asm.emit(&Inst::new(Mnemonic::Neg, [Reg::EAX.into()]))?;
//! asm.bind(positive)?;
//! asm.emit(&Inst::new(Mnemonic::Ret, []))?;
//! let code = asm.finish()?;
//! assert_eq!(code.bytes(), [0x89, 0xF8, 0x85, 0xC0, 0x79, 0x02, 0xF7, 0xD8, 0xC3]);
//! # Ok::<(), isa_lang::x86_64::AsmError>(())
//! ```
//!
//! ## Guarantees
//!
//! - `decode(encode(i)) == i` for every instruction the encoder accepts,
//!   checked over every form's whole operand space.
//! - Anything the decoder accepts, the encoder can encode back to the same
//!   instruction.
//! - The decoder never panics, on any input, and every error names the byte
//!   at fault.
//! - Encodings match the Intel SDM byte for byte (known-answer tests).
//!
//! ## Features
//!
//! - `std` (default): no effect today; the crate is `no_std` and needs only
//!   `alloc` (for [`x86_64::Assembler`]).

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]

extern crate alloc;

pub mod x86_64;

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;
