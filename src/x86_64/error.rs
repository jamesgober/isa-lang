//! Encoding and decoding errors.

use core::fmt;

use super::inst::Mnemonic;
use super::reg::Reg;

/// Why an instruction could not be encoded.
///
/// The encoder tries every form of the mnemonic and reports the failure of
/// the form that came closest: an operand-kind mismatch only when no form
/// took the operand kinds at all.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode, EncodeError, Inst, Mem, Mnemonic, Operand, Reg, Size};
///
/// // Memory to memory has no form.
/// let m = Mem::new(Size::Qword, Reg::RAX);
/// let err = encode(&Inst::new(Mnemonic::Mov, [m.into(), m.into()])).unwrap_err();
/// assert_eq!(err, EncodeError::NoForm { mnemonic: Mnemonic::Mov });
///
/// // A 64-bit add takes a sign-extended 32-bit immediate at most.
/// let err = encode(&Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(1 << 40)])).unwrap_err();
/// assert_eq!(err, EncodeError::ImmediateOutOfRange { value: 1 << 40 });
/// assert_eq!(err.to_string(), "immediate 0x10000000000 does not fit any form for these operands");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodeError {
    /// No form of the mnemonic takes operands of these kinds, sizes, and
    /// count. Check the operand order (destination first) and widths.
    NoForm {
        /// The mnemonic that was asked for.
        mnemonic: Mnemonic,
    },
    /// Forms take these operand kinds, but the immediate is outside every
    /// one's range. Immediates are signed values of the operation's width;
    /// write `-1`, not `0xFFFF_FFFF`, for a 32-bit all-ones value, and load
    /// wider constants into a register first.
    ImmediateOutOfRange {
        /// The immediate as given.
        value: i64,
    },
    /// The branch target is beyond the reach of a 32-bit displacement.
    TargetOutOfRange {
        /// The target, relative to the start of the instruction.
        offset: i64,
    },
    /// `AH`, `CH`, `DH`, or `BH` appears in an instruction that needs a REX
    /// prefix (64-bit operation, `R8`-`R15`, or `SPL`/`BPL`/`SIL`/`DIL`),
    /// where those encodings mean `SPL`-`DIL` instead. Use another register.
    HighByteWithRex,
    /// A memory operand's base register is not a 64-bit register.
    BaseNotQword {
        /// The offending base.
        base: Reg,
    },
    /// A memory operand's index register is not a 64-bit register.
    IndexNotQword {
        /// The offending index.
        index: Reg,
    },
    /// `RSP` is used as an index register, which the SIB byte cannot express.
    /// Swap base and index if the scale is 1.
    RspIndex,
    /// A RIP-relative memory operand has an index register.
    RipWithIndex,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::NoForm { mnemonic } => {
                write!(f, "no `{mnemonic}` form takes these operands")
            }
            EncodeError::ImmediateOutOfRange { value } => {
                f.write_str("immediate ")?;
                super::fmt::hex(f, *value)?;
                f.write_str(" does not fit any form for these operands")
            }
            EncodeError::TargetOutOfRange { offset } => {
                f.write_str("branch target $")?;
                super::fmt::signed_hex(f, *offset)?;
                f.write_str(" is out of reach of a 32-bit displacement")
            }
            EncodeError::HighByteWithRex => f.write_str(
                "ah, ch, dh, and bh cannot be used in an instruction that needs a REX prefix",
            ),
            EncodeError::BaseNotQword { base } => {
                write!(f, "memory base `{base}` is not a 64-bit register")
            }
            EncodeError::IndexNotQword { index } => {
                write!(f, "memory index `{index}` is not a 64-bit register")
            }
            EncodeError::RspIndex => f.write_str("rsp cannot be an index register"),
            EncodeError::RipWithIndex => {
                f.write_str("a rip-relative memory operand cannot have an index register")
            }
        }
    }
}

impl core::error::Error for EncodeError {}

/// Why bytes could not be decoded.
///
/// Every variant carries `at`, the offset in the input of the byte at fault.
/// The decoder accepts exactly the encodings the form table describes, plus
/// a redundant empty REX prefix (`40`) and a redundant REX.W on 64-bit-default
/// forms; anything else is an error, never a guess.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{decode, DecodeError};
///
/// assert_eq!(decode(&[0x48, 0x83]), Err(DecodeError::Truncated { at: 2 }));
/// assert_eq!(decode(&[0xF0, 0x01, 0xC0]), Err(DecodeError::UnsupportedPrefix { at: 0, byte: 0xF0 }));
/// assert_eq!(decode(&[0x0F, 0x05]), Err(DecodeError::UnknownOpcode { at: 1, escaped: true, opcode: 0x05 }));
/// assert_eq!(
///     decode(&[0x0F, 0x05]).unwrap_err().to_string(),
///     "unknown opcode 0f 05 at byte 1",
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecodeError {
    /// The input ends before the instruction does.
    Truncated {
        /// The offset at which another byte was needed.
        at: usize,
    },
    /// A legacy prefix outside the supported set (only `66` is supported:
    /// no LOCK, REP, segment, or address-size prefixes).
    UnsupportedPrefix {
        /// Offset of the prefix.
        at: usize,
        /// The prefix byte.
        byte: u8,
    },
    /// The operand-size prefix `66` appears twice.
    RepeatedPrefix {
        /// Offset of the second `66`.
        at: usize,
    },
    /// A REX prefix is followed by another prefix instead of the opcode.
    MisplacedRex {
        /// Offset of the REX prefix.
        at: usize,
    },
    /// No form has this opcode.
    UnknownOpcode {
        /// Offset of the opcode byte.
        at: usize,
        /// Whether the opcode follows the `0F` escape.
        escaped: bool,
        /// The opcode byte.
        opcode: u8,
    },
    /// The opcode exists, but not with this operand size (`66` / REX.W).
    InvalidOperandSize {
        /// Offset of the opcode byte.
        at: usize,
    },
    /// The opcode exists, but not with this ModRM `reg` extension.
    UnknownExtension {
        /// Offset of the ModRM byte.
        at: usize,
        /// The `/digit` found.
        ext: u8,
    },
    /// The form needs a memory operand but ModRM names a register.
    RegisterOperand {
        /// Offset of the ModRM byte.
        at: usize,
    },
    /// The REX prefix sets R, X, or B, but no operand of the form uses it.
    UnusedRex {
        /// Offset of the REX prefix.
        at: usize,
    },
}

impl DecodeError {
    /// The offset in the input of the byte at fault.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::decode;
    ///
    /// assert_eq!(decode(&[0x66, 0x66, 0x90]).unwrap_err().at(), 1);
    /// ```
    #[must_use]
    pub const fn at(&self) -> usize {
        match *self {
            DecodeError::Truncated { at }
            | DecodeError::UnsupportedPrefix { at, .. }
            | DecodeError::RepeatedPrefix { at }
            | DecodeError::MisplacedRex { at }
            | DecodeError::UnknownOpcode { at, .. }
            | DecodeError::InvalidOperandSize { at }
            | DecodeError::UnknownExtension { at, .. }
            | DecodeError::RegisterOperand { at }
            | DecodeError::UnusedRex { at } => at,
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            DecodeError::Truncated { at } => {
                write!(
                    f,
                    "instruction is cut off: byte {at} is past the end of the input"
                )
            }
            DecodeError::UnsupportedPrefix { at, byte } => {
                write!(f, "unsupported prefix {byte:02x} at byte {at}")
            }
            DecodeError::RepeatedPrefix { at } => {
                write!(f, "repeated operand-size prefix at byte {at}")
            }
            DecodeError::MisplacedRex { at } => {
                write!(
                    f,
                    "REX prefix at byte {at} is not immediately before the opcode"
                )
            }
            DecodeError::UnknownOpcode {
                at,
                escaped,
                opcode,
            } => {
                let escape = if escaped { "0f " } else { "" };
                write!(f, "unknown opcode {escape}{opcode:02x} at byte {at}")
            }
            DecodeError::InvalidOperandSize { at } => {
                write!(f, "opcode at byte {at} has no form with this operand size")
            }
            DecodeError::UnknownExtension { at, ext } => {
                write!(
                    f,
                    "opcode extension /{ext} in the ModRM byte at {at} is not defined"
                )
            }
            DecodeError::RegisterOperand { at } => {
                write!(
                    f,
                    "ModRM byte at {at} names a register where memory is required"
                )
            }
            DecodeError::UnusedRex { at } => {
                write!(f, "REX prefix at byte {at} sets a bit no operand uses")
            }
        }
    }
}

impl core::error::Error for DecodeError {}
