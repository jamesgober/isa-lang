//! Operands: registers, memory references, immediates, and branch targets,
//! plus the condition codes carried by `Jcc`, `SETcc`, and `CMOVcc`.

use core::fmt;

use super::flags::Flags;
use super::reg::{Reg, Size};

/// The scale factor of a memory operand's index register.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::Scale;
///
/// assert_eq!(Scale::X8.factor(), 8);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scale {
    /// `index*1`.
    X1,
    /// `index*2`.
    X2,
    /// `index*4`.
    X4,
    /// `index*8`.
    X8,
}

impl Scale {
    /// The multiplier: 1, 2, 4, or 8.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Scale;
    ///
    /// assert_eq!(Scale::X2.factor(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub const fn factor(self) -> u8 {
        1 << self.bits()
    }

    /// The two-bit SIB `scale` field.
    pub(crate) const fn bits(self) -> u8 {
        match self {
            Scale::X1 => 0,
            Scale::X2 => 1,
            Scale::X4 => 2,
            Scale::X8 => 3,
        }
    }

    pub(crate) const fn from_bits(bits: u8) -> Scale {
        match bits & 3 {
            0 => Scale::X1,
            1 => Scale::X2,
            2 => Scale::X4,
            _ => Scale::X8,
        }
    }
}

// `Mem::base` holds a packed `Reg` (always below 0x80) or one of these.
const NO_REG: u8 = 0xFF;
const RIP: u8 = 0xFE;

/// A memory operand: `size ptr [base + index*scale + disp]`, `[rip + disp]`,
/// or an absolute `[disp]`.
///
/// The size is the width of the access, written `qword ptr` and so on in
/// Intel syntax. The encoder chooses the shortest displacement field (none,
/// 8-bit, or 32-bit) and whether a SIB byte is needed, so the same `Mem` always
/// encodes the same way; only the address matters, never the bytes it came
/// from.
///
/// Addresses are 64-bit: base and index must be 64-bit registers, and `RSP`
/// cannot be an index. These rules are checked when the operand is encoded
/// ([`EncodeError::BaseNotQword`](crate::x86_64::EncodeError::BaseNotQword) and
/// the variants after it), so building a `Mem` never fails.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Mem, Reg, Scale, Size};
///
/// let local = Mem::new(Size::Qword, Reg::RBP).with_disp(-8);
/// assert_eq!(local.to_string(), "qword ptr [rbp-0x8]");
///
/// let element = Mem::new(Size::Dword, Reg::RDI).with_index(Reg::RCX, Scale::X4);
/// assert_eq!(element.to_string(), "dword ptr [rdi+rcx*4]");
///
/// assert_eq!(Mem::rip(Size::Byte, 16).to_string(), "byte ptr [rip+0x10]");
/// assert_eq!(Mem::absolute(Size::Word, 0x1000).to_string(), "word ptr [0x1000]");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mem {
    disp: i32,
    size: Size,
    base: u8,
    index: u8,
    scale: Scale,
}

impl Mem {
    /// `[base]`, accessing `size` bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// assert_eq!(Mem::new(Size::Qword, Reg::RSP).to_string(), "qword ptr [rsp]");
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(size: Size, base: Reg) -> Mem {
        Mem {
            disp: 0,
            size,
            base: base.bits(),
            index: NO_REG,
            scale: Scale::X1,
        }
    }

    /// `[rip + disp]`: an address relative to the end of the instruction.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Size};
    ///
    /// let m = Mem::rip(Size::Qword, -0x20);
    /// assert!(m.is_rip_relative());
    /// assert_eq!(m.to_string(), "qword ptr [rip-0x20]");
    /// ```
    #[inline]
    #[must_use]
    pub const fn rip(size: Size, disp: i32) -> Mem {
        Mem {
            disp,
            size,
            base: RIP,
            index: NO_REG,
            scale: Scale::X1,
        }
    }

    /// `[addr]`: an absolute address, sign-extended from 32 bits.
    ///
    /// Add an index with [`with_index`](Mem::with_index) for `[index*scale + disp]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Scale, Size};
    ///
    /// let table = Mem::absolute(Size::Qword, 0x4000).with_index(Reg::RAX, Scale::X8);
    /// assert_eq!(table.to_string(), "qword ptr [rax*8+0x4000]");
    /// ```
    #[inline]
    #[must_use]
    pub const fn absolute(size: Size, addr: i32) -> Mem {
        Mem {
            disp: addr,
            size,
            base: NO_REG,
            index: NO_REG,
            scale: Scale::X1,
        }
    }

    /// The same operand with displacement `disp`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// assert_eq!(Mem::new(Size::Byte, Reg::RAX).with_disp(1).disp(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_disp(self, disp: i32) -> Mem {
        Mem { disp, ..self }
    }

    /// The same operand with index register `index` scaled by `scale`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Scale, Size};
    ///
    /// let m = Mem::new(Size::Qword, Reg::RBX).with_index(Reg::R12, Scale::X2);
    /// assert_eq!(m.index(), Some(Reg::R12));
    /// assert_eq!(m.scale(), Scale::X2);
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_index(self, index: Reg, scale: Scale) -> Mem {
        Mem {
            index: index.bits(),
            scale,
            ..self
        }
    }

    /// The same address with access size `size`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// let m = Mem::new(Size::Qword, Reg::RAX).with_size(Size::Byte);
    /// assert_eq!(m.size(), Size::Byte);
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_size(self, size: Size) -> Mem {
        Mem { size, ..self }
    }

    /// The width of the access.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Size};
    ///
    /// assert_eq!(Mem::rip(Size::Dword, 0).size(), Size::Dword);
    /// ```
    #[inline]
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// The base register, if any. RIP-relative and absolute operands have none.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// assert_eq!(Mem::new(Size::Qword, Reg::R13).base(), Some(Reg::R13));
    /// assert_eq!(Mem::rip(Size::Qword, 0).base(), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn base(&self) -> Option<Reg> {
        if self.base & 0x80 == 0 {
            Some(Reg::from_bits(self.base))
        } else {
            None
        }
    }

    /// The index register, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// assert_eq!(Mem::new(Size::Qword, Reg::RAX).index(), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn index(&self) -> Option<Reg> {
        if self.index & 0x80 == 0 {
            Some(Reg::from_bits(self.index))
        } else {
            None
        }
    }

    /// The index scale; [`Scale::X1`] when there is no index.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Scale, Size};
    ///
    /// assert_eq!(Mem::new(Size::Qword, Reg::RAX).scale(), Scale::X1);
    /// ```
    #[inline]
    #[must_use]
    pub const fn scale(&self) -> Scale {
        self.scale
    }

    /// The displacement (or, for an absolute operand, the address).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Size};
    ///
    /// assert_eq!(Mem::absolute(Size::Qword, -4).disp(), -4);
    /// ```
    #[inline]
    #[must_use]
    pub const fn disp(&self) -> i32 {
        self.disp
    }

    /// Whether the address is `[rip + disp]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Mem, Reg, Size};
    ///
    /// assert!(Mem::rip(Size::Qword, 0).is_rip_relative());
    /// assert!(!Mem::new(Size::Qword, Reg::RAX).is_rip_relative());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_rip_relative(&self) -> bool {
        self.base == RIP
    }

    /// A decoded memory operand. `base` is `None` for "no base", and RIP is
    /// built with [`Mem::rip`] instead.
    pub(crate) const fn decoded(
        size: Size,
        base: Option<Reg>,
        index: Option<Reg>,
        scale: Scale,
        disp: i32,
    ) -> Mem {
        let (index, scale) = match index {
            Some(r) => (r.bits(), scale),
            None => (NO_REG, Scale::X1),
        };
        let base = match base {
            Some(r) => r.bits(),
            None => NO_REG,
        };
        Mem {
            disp,
            size,
            base,
            index,
            scale,
        }
    }
}

/// One operand of an [`Inst`](crate::x86_64::Inst).
///
/// - `Imm(v)` is the operand's value as a signed integer of the operation's
///   width: for a 32-bit operation write `-1`, not `0xFFFF_FFFF`. Unsigned
///   fields (shift counts, `bt` bit numbers, the `ret` pop count) take
///   `0..=255` or `0..=65535`. Values outside the field's range are refused
///   by the encoder, never truncated.
/// - `Rel(d)` is a branch target, `d` bytes from the **start** of the
///   branch instruction (NASM's `$+d`). The encoder picks the 8-bit or 32-bit
///   form and subtracts the instruction's own length. Use the
///   [`Assembler`](crate::x86_64::Assembler) and labels to branch to code
///   whose position is not known yet.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Mem, Operand, Reg, Size};
///
/// let ops: [Operand; 4] = [
///     Reg::RAX.into(),
///     Mem::new(Size::Qword, Reg::RSP).into(),
///     Operand::Imm(-1),
///     Operand::Rel(16),
/// ];
/// assert_eq!(ops[0].to_string(), "rax");
/// assert_eq!(ops[2].to_string(), "-0x1");
/// assert_eq!(ops[3].to_string(), "$+0x10");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operand {
    /// A register.
    Reg(Reg),
    /// A memory reference.
    Mem(Mem),
    /// An immediate value.
    Imm(i64),
    /// A branch target relative to the start of the instruction.
    Rel(i64),
}

impl From<Reg> for Operand {
    #[inline]
    fn from(reg: Reg) -> Operand {
        Operand::Reg(reg)
    }
}

impl From<Mem> for Operand {
    #[inline]
    fn from(mem: Mem) -> Operand {
        Operand::Mem(mem)
    }
}

/// A condition code: the `cc` in `Jcc`, `SETcc`, and `CMOVcc`.
///
/// The discriminants are the hardware encodings (the low four opcode bits),
/// and each condition's negation differs only in bit 0. Names follow the
/// common Intel spellings; aliases such as `z` for `e` are the same value.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Cond, Flags};
///
/// assert_eq!(Cond::L.negate(), Cond::Ge);
/// assert_eq!(Cond::E.code(), 4);
/// assert_eq!(Cond::from_code(15), Some(Cond::G));
/// assert_eq!(Cond::Le.reads(), Flags::ZF | Flags::SF | Flags::OF);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cond {
    /// Overflow (`OF = 1`).
    O,
    /// No overflow (`OF = 0`).
    No,
    /// Below, unsigned `<` (`CF = 1`).
    B,
    /// Above or equal, unsigned `>=` (`CF = 0`).
    Ae,
    /// Equal (`ZF = 1`).
    E,
    /// Not equal (`ZF = 0`).
    Ne,
    /// Below or equal, unsigned `<=` (`CF = 1 or ZF = 1`).
    Be,
    /// Above, unsigned `>` (`CF = 0 and ZF = 0`).
    A,
    /// Sign (`SF = 1`).
    S,
    /// No sign (`SF = 0`).
    Ns,
    /// Parity even (`PF = 1`).
    P,
    /// Parity odd (`PF = 0`).
    Np,
    /// Less, signed `<` (`SF != OF`).
    L,
    /// Greater or equal, signed `>=` (`SF = OF`).
    Ge,
    /// Less or equal, signed `<=` (`ZF = 1 or SF != OF`).
    Le,
    /// Greater, signed `>` (`ZF = 0 and SF = OF`).
    G,
}

const CONDS: [Cond; 16] = [
    Cond::O,
    Cond::No,
    Cond::B,
    Cond::Ae,
    Cond::E,
    Cond::Ne,
    Cond::Be,
    Cond::A,
    Cond::S,
    Cond::Ns,
    Cond::P,
    Cond::Np,
    Cond::L,
    Cond::Ge,
    Cond::Le,
    Cond::G,
];

const COND_NAMES: [&str; 16] = [
    "o", "no", "b", "ae", "e", "ne", "be", "a", "s", "ns", "p", "np", "l", "ge", "le", "g",
];

impl Cond {
    /// The 4-bit hardware encoding.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Cond;
    ///
    /// assert_eq!(Cond::Ne.code(), 5);
    /// ```
    #[inline]
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The condition with hardware encoding `code`, or `None` above 15.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Cond;
    ///
    /// assert_eq!(Cond::from_code(2), Some(Cond::B));
    /// assert_eq!(Cond::from_code(16), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Cond> {
        if code < 16 {
            Some(CONDS[code as usize])
        } else {
            None
        }
    }

    /// The opposite condition.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Cond;
    ///
    /// assert_eq!(Cond::A.negate(), Cond::Be);
    /// assert_eq!(Cond::A.negate().negate(), Cond::A);
    /// ```
    #[inline]
    #[must_use]
    pub const fn negate(self) -> Cond {
        CONDS[(self.code() ^ 1) as usize]
    }

    /// The flags the condition tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Cond, Flags};
    ///
    /// assert_eq!(Cond::B.reads(), Flags::CF);
    /// ```
    #[must_use]
    pub const fn reads(self) -> Flags {
        match self {
            Cond::O | Cond::No => Flags::OF,
            Cond::B | Cond::Ae => Flags::CF,
            Cond::E | Cond::Ne => Flags::ZF,
            Cond::Be | Cond::A => Flags::CF.union(Flags::ZF),
            Cond::S | Cond::Ns => Flags::SF,
            Cond::P | Cond::Np => Flags::PF,
            Cond::L | Cond::Ge => Flags::SF.union(Flags::OF),
            Cond::Le | Cond::G => Flags::ZF.union(Flags::SF).union(Flags::OF),
        }
    }

    /// The mnemonic suffix: `"e"` in `je`, `sete`, `cmove`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Cond;
    ///
    /// assert_eq!(Cond::Ae.suffix(), "ae");
    /// ```
    #[inline]
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        COND_NAMES[self as usize]
    }
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cond_codes_round_trip_and_negate() {
        for code in 0..16u8 {
            let c = Cond::from_code(code);
            assert_eq!(c.map(Cond::code), Some(code));
            let c = c.unwrap_or(Cond::O);
            assert_eq!(c.negate().code(), code ^ 1);
            assert_eq!(c.negate().reads(), c.reads());
        }
        assert_eq!(Cond::from_code(16), None);
    }

    #[test]
    fn test_scale_bits_round_trip() {
        for s in [Scale::X1, Scale::X2, Scale::X4, Scale::X8] {
            assert_eq!(Scale::from_bits(s.bits()), s);
        }
        assert_eq!(Scale::X4.factor(), 4);
    }

    #[test]
    fn test_mem_accessors() {
        let m = Mem::new(Size::Qword, Reg::RBX)
            .with_index(Reg::RSI, Scale::X8)
            .with_disp(-5);
        assert_eq!(m.base(), Some(Reg::RBX));
        assert_eq!(m.index(), Some(Reg::RSI));
        assert_eq!(m.scale(), Scale::X8);
        assert_eq!(m.disp(), -5);
        assert!(!m.is_rip_relative());
        let a = Mem::absolute(Size::Byte, 7);
        assert!(!a.is_rip_relative() && a.base().is_none() && a.index().is_none());
        let r = Mem::rip(Size::Byte, 7);
        assert!(r.is_rip_relative() && r.base().is_none());
    }

    #[test]
    fn test_mem_decoded_canonical_scale_without_index() {
        let m = Mem::decoded(Size::Dword, Some(Reg::RAX), None, Scale::X8, 0);
        assert_eq!(m, Mem::new(Size::Dword, Reg::RAX));
    }
}
