//! The general-purpose register file: sixteen registers, each addressable at
//! four widths, plus the four legacy high-byte registers.
//!
//! This is the register section of the x86-64 target description. Names and
//! hardware numbers are data here; the encoder and decoder only ever look at
//! a register's width, its 4-bit number, and whether it is a high-byte
//! register (which constrains REX, see `docs/API.md`).

use core::fmt;

/// The width of an operand: a register, a memory access, or an operation.
///
/// Sizes order from narrowest to widest, so `Size::Byte < Size::Qword`.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Reg, Size};
///
/// assert_eq!(Reg::EAX.size(), Size::Dword);
/// assert_eq!(Size::Qword.bits(), 64);
/// assert!(Size::Byte < Size::Qword);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Size {
    /// 8 bits.
    Byte,
    /// 16 bits.
    Word,
    /// 32 bits.
    Dword,
    /// 64 bits.
    Qword,
}

impl Size {
    /// The width in bits: 8, 16, 32, or 64.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Size;
    ///
    /// assert_eq!(Size::Word.bits(), 16);
    /// ```
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            Size::Byte => 8,
            Size::Word => 16,
            Size::Dword => 32,
            Size::Qword => 64,
        }
    }

    /// The width in bytes: 1, 2, 4, or 8.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Size;
    ///
    /// assert_eq!(Size::Dword.bytes(), 4);
    /// ```
    #[inline]
    #[must_use]
    pub const fn bytes(self) -> u32 {
        self.bits() / 8
    }

    /// The Intel-syntax memory size keyword (`byte`, `word`, `dword`, `qword`).
    pub(crate) const fn keyword(self) -> &'static str {
        match self {
            Size::Byte => "byte",
            Size::Word => "word",
            Size::Dword => "dword",
            Size::Qword => "qword",
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Size::Byte => 0,
            Size::Word => 1,
            Size::Dword => 2,
            Size::Qword => 3,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Size {
        match code & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            2 => Size::Dword,
            _ => Size::Qword,
        }
    }
}

/// A general-purpose register at a particular width.
///
/// A register is its hardware number (0–15), its width, and, for `AH`, `CH`,
/// `DH`, and `BH`, a high-byte flag. `RAX`, `EAX`, `AX`, and `AL` are the same
/// physical register at four widths and are four distinct `Reg` values.
///
/// Every register has a constant (`Reg::RAX`, `Reg::R10D`, `Reg::SIL`, ...),
/// and [`Reg::new`] builds one from a width and a number, which is what an
/// allocator that works in register numbers needs.
///
/// The high-byte registers cannot appear in an instruction that needs a REX
/// prefix; [`encode`](crate::x86_64::encode) reports
/// [`EncodeError::HighByteWithRex`](crate::x86_64::EncodeError::HighByteWithRex).
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Reg, Size};
///
/// let r = Reg::new(Size::Dword, 10).unwrap();
/// assert_eq!(r, Reg::R10D);
/// assert_eq!(r.num(), 10);
/// assert_eq!(r.to_string(), "r10d");
/// assert!(Reg::new(Size::Qword, 16).is_none());
/// assert!(Reg::AH.is_high_byte());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Reg(u8);

// Packing: bits 0-3 hardware number, bit 4 high-byte, bits 5-6 size code.
// One byte keeps `Mem` at eight bytes and `Operand` at sixteen.
const HIGH: u8 = 0x10;

impl Reg {
    const fn pack(size: Size, num: u8, high: bool) -> Reg {
        Reg((size.code() << 5) | (num & 15) | if high { HIGH } else { 0 })
    }

    /// The register with hardware number `num` (0–15) at `size`, or `None`
    /// when `num` is out of range.
    ///
    /// At `Size::Byte`, numbers 4–7 are `SPL`, `BPL`, `SIL`, and `DIL`; the
    /// high-byte registers have their own constants ([`Reg::AH`] and so on).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Reg, Size};
    ///
    /// assert_eq!(Reg::new(Size::Qword, 0), Some(Reg::RAX));
    /// assert_eq!(Reg::new(Size::Byte, 6), Some(Reg::SIL));
    /// assert_eq!(Reg::new(Size::Word, 99), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(size: Size, num: u8) -> Option<Reg> {
        if num < 16 {
            Some(Reg::pack(size, num, false))
        } else {
            None
        }
    }

    /// The register's width.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Reg, Size};
    ///
    /// assert_eq!(Reg::R8W.size(), Size::Word);
    /// ```
    #[inline]
    #[must_use]
    pub const fn size(self) -> Size {
        Size::from_code(self.0 >> 5)
    }

    /// The 4-bit hardware number that goes into ModRM, SIB, or the opcode
    /// byte (with REX supplying bit 3). The high-byte registers report the
    /// number they are encoded with: `AH` is 4, `BH` is 7.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Reg;
    ///
    /// assert_eq!(Reg::R15.num(), 15);
    /// assert_eq!(Reg::AH.num(), 4);
    /// ```
    #[inline]
    #[must_use]
    pub const fn num(self) -> u8 {
        self.0 & 15
    }

    /// Whether this is `AH`, `CH`, `DH`, or `BH`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Reg;
    ///
    /// assert!(Reg::BH.is_high_byte());
    /// assert!(!Reg::DIL.is_high_byte());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_high_byte(self) -> bool {
        self.0 & HIGH != 0
    }

    /// The register's lowercase Intel name, such as `"rax"` or `"r9b"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Reg;
    ///
    /// assert_eq!(Reg::SPL.name(), "spl");
    /// assert_eq!(Reg::CH.name(), "ch");
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        let num = self.num() as usize;
        if self.is_high_byte() {
            return HIGH_NAMES[num & 3];
        }
        match self.size() {
            Size::Byte => NAMES8[num],
            Size::Word => NAMES16[num],
            Size::Dword => NAMES32[num],
            Size::Qword => NAMES64[num],
        }
    }

    /// Whether this register can only be encoded with a REX prefix
    /// (`SPL`, `BPL`, `SIL`, `DIL`, and every `R8`–`R15` form).
    pub(crate) const fn needs_rex(self) -> bool {
        let num = self.num();
        num >= 8 || (matches!(self.size(), Size::Byte) && !self.is_high_byte() && num >= 4)
    }

    /// The decoder's view of an 8-bit register field: without any REX prefix,
    /// numbers 4-7 name the high-byte registers.
    pub(crate) const fn decode(size: Size, num: u8, rex: bool) -> Reg {
        let high = matches!(size, Size::Byte) && !rex && num >= 4 && num < 8;
        Reg::pack(size, num, high)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) const fn from_bits(bits: u8) -> Reg {
        Reg(bits & 0x7F)
    }
}

const NAMES64: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];
const NAMES32: [&str; 16] = [
    "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "r12d",
    "r13d", "r14d", "r15d",
];
const NAMES16: [&str; 16] = [
    "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w",
    "r14w", "r15w",
];
const NAMES8: [&str; 16] = [
    "al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b", "r12b",
    "r13b", "r14b", "r15b",
];
const HIGH_NAMES: [&str; 4] = ["ah", "ch", "dh", "bh"];

macro_rules! regs {
    ($size:ident: $($name:ident = $num:expr),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($name), "` register.")]
            pub const $name: Reg = Reg::pack(Size::$size, $num, false);
        )*
    };
}

#[rustfmt::skip]
impl Reg {
    regs!(Qword: RAX = 0, RCX = 1, RDX = 2, RBX = 3, RSP = 4, RBP = 5, RSI = 6, RDI = 7,
        R8 = 8, R9 = 9, R10 = 10, R11 = 11, R12 = 12, R13 = 13, R14 = 14, R15 = 15);
    regs!(Dword: EAX = 0, ECX = 1, EDX = 2, EBX = 3, ESP = 4, EBP = 5, ESI = 6, EDI = 7,
        R8D = 8, R9D = 9, R10D = 10, R11D = 11, R12D = 12, R13D = 13, R14D = 14, R15D = 15);
    regs!(Word: AX = 0, CX = 1, DX = 2, BX = 3, SP = 4, BP = 5, SI = 6, DI = 7,
        R8W = 8, R9W = 9, R10W = 10, R11W = 11, R12W = 12, R13W = 13, R14W = 14, R15W = 15);
    regs!(Byte: AL = 0, CL = 1, DL = 2, BL = 3, SPL = 4, BPL = 5, SIL = 6, DIL = 7,
        R8B = 8, R9B = 9, R10B = 10, R11B = 11, R12B = 12, R13B = 13, R14B = 14, R15B = 15);

    /// The `AH` register: bits 8-15 of `RAX`. Cannot be used with REX.
    pub const AH: Reg = Reg::pack(Size::Byte, 4, true);
    /// The `CH` register: bits 8-15 of `RCX`. Cannot be used with REX.
    pub const CH: Reg = Reg::pack(Size::Byte, 5, true);
    /// The `DH` register: bits 8-15 of `RDX`. Cannot be used with REX.
    pub const DH: Reg = Reg::pack(Size::Byte, 6, true);
    /// The `BH` register: bits 8-15 of `RBX`. Cannot be used with REX.
    pub const BH: Reg = Reg::pack(Size::Byte, 7, true);
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Debug for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_new_every_number_round_trips() {
        for size in [Size::Byte, Size::Word, Size::Dword, Size::Qword] {
            for num in 0..16 {
                let r = Reg::new(size, num);
                assert!(r.is_some_and(|r| r.size() == size && r.num() == num && !r.is_high_byte()));
            }
            assert_eq!(Reg::new(size, 16), None);
            assert_eq!(Reg::new(size, 255), None);
        }
    }

    #[test]
    fn test_reg_names_are_distinct() {
        let mut names = alloc::vec::Vec::new();
        for size in [Size::Byte, Size::Word, Size::Dword, Size::Qword] {
            for num in 0..16 {
                names.extend(Reg::new(size, num).map(Reg::name));
            }
        }
        names.extend([Reg::AH, Reg::CH, Reg::DH, Reg::BH].map(Reg::name));
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn test_reg_high_byte_numbers_and_names() {
        assert_eq!((Reg::AH.num(), Reg::AH.name()), (4, "ah"));
        assert_eq!((Reg::CH.num(), Reg::CH.name()), (5, "ch"));
        assert_eq!((Reg::DH.num(), Reg::DH.name()), (6, "dh"));
        assert_eq!((Reg::BH.num(), Reg::BH.name()), (7, "bh"));
        assert_ne!(Reg::AH, Reg::SPL);
    }

    #[test]
    fn test_reg_needs_rex() {
        assert!(!Reg::RAX.needs_rex());
        assert!(Reg::R8.needs_rex());
        assert!(Reg::SPL.needs_rex());
        assert!(Reg::DIL.needs_rex());
        assert!(!Reg::AH.needs_rex());
        assert!(!Reg::ESP.needs_rex());
        assert!(Reg::R15B.needs_rex());
    }

    #[test]
    fn test_reg_decode_high_byte_depends_on_rex() {
        assert_eq!(Reg::decode(Size::Byte, 4, false), Reg::AH);
        assert_eq!(Reg::decode(Size::Byte, 4, true), Reg::SPL);
        assert_eq!(Reg::decode(Size::Dword, 4, false), Reg::ESP);
        assert_eq!(Reg::decode(Size::Byte, 12, false), Reg::R12B);
    }

    #[test]
    fn test_reg_bits_round_trip() {
        for r in [Reg::RAX, Reg::AH, Reg::R15B, Reg::SP, Reg::R9D] {
            assert_eq!(Reg::from_bits(r.bits()), r);
        }
    }
}
