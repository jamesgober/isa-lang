//! Arithmetic status flags and the effect each instruction form has on them.
//!
//! Flag effects are part of the target description: every form records which
//! flags it reads, which it writes with a defined value, and which it leaves
//! undefined. Instruction selection and scheduling need exactly this to know
//! whether a compare can be reused or a flag-setting instruction moved.

use core::fmt;
use core::ops::BitOr;

/// A set of arithmetic status flags (`CF`, `PF`, `AF`, `ZF`, `SF`, `OF`).
///
/// The bits are the flags' positions in `RFLAGS`, so `Flags::ZF.bits()` is
/// `1 << 6`.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::Flags;
///
/// let f = Flags::CF | Flags::ZF;
/// assert!(f.contains(Flags::ZF));
/// assert!(!f.contains(Flags::SF));
/// assert_eq!(f.to_string(), "CF|ZF");
/// assert_eq!(Flags::NONE.to_string(), "none");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(u16);

impl Flags {
    /// No flags.
    pub const NONE: Flags = Flags(0);
    /// Carry flag.
    pub const CF: Flags = Flags(1 << 0);
    /// Parity flag.
    pub const PF: Flags = Flags(1 << 2);
    /// Auxiliary-carry flag.
    pub const AF: Flags = Flags(1 << 4);
    /// Zero flag.
    pub const ZF: Flags = Flags(1 << 6);
    /// Sign flag.
    pub const SF: Flags = Flags(1 << 7);
    /// Overflow flag.
    pub const OF: Flags = Flags(1 << 11);
    /// All six status flags.
    pub const ALL: Flags = Flags(1 | (1 << 2) | (1 << 4) | (1 << 6) | (1 << 7) | (1 << 11));

    /// The flags as `RFLAGS` bit positions.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Flags;
    ///
    /// assert_eq!(Flags::OF.bits(), 0x800);
    /// ```
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Every flag in either set.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Flags;
    ///
    /// assert_eq!(Flags::CF.union(Flags::OF), Flags::CF | Flags::OF);
    /// ```
    #[inline]
    #[must_use]
    pub const fn union(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }

    /// The flags in `self` but not in `other`.
    pub(crate) const fn without(self, other: Flags) -> Flags {
        Flags(self.0 & !other.0)
    }

    /// Whether every flag in `other` is in `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Flags;
    ///
    /// assert!(Flags::ALL.contains(Flags::AF | Flags::PF));
    /// ```
    #[inline]
    #[must_use]
    pub const fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Flags;
    ///
    /// assert!(Flags::NONE.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for Flags {
    type Output = Flags;

    #[inline]
    fn bitor(self, rhs: Flags) -> Flags {
        self.union(rhs)
    }
}

const FLAG_NAMES: [(Flags, &str); 6] = [
    (Flags::CF, "CF"),
    (Flags::PF, "PF"),
    (Flags::AF, "AF"),
    (Flags::ZF, "ZF"),
    (Flags::SF, "SF"),
    (Flags::OF, "OF"),
];

impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("none");
        }
        let mut first = true;
        for (flag, name) in FLAG_NAMES {
            if self.contains(flag) {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// What an instruction does to the status flags.
///
/// - `read`: flags whose value the instruction depends on.
/// - `written`: flags set to a defined value.
/// - `undefined`: flags left with an unspecified value.
///
/// Shifts and rotates leave every flag unchanged when the masked count is 0;
/// their `written` set describes a non-zero count. A consumer that cannot
/// prove the count is non-zero must treat those flags as possibly unchanged.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Reg};
///
/// let adc = encode(&Inst::new(Mnemonic::Adc, [Reg::RAX.into(), Reg::RBX.into()]))?;
/// assert_eq!(adc.flags().read(), Flags::CF);
/// assert_eq!(adc.flags().written(), Flags::ALL);
///
/// let and = encode(&Inst::new(Mnemonic::And, [Reg::EAX.into(), Reg::EBX.into()]))?;
/// assert_eq!(and.flags().undefined(), Flags::AF);
/// assert_eq!(and.flags().clobbered(), Flags::ALL);
/// # Ok::<(), isa_lang::x86_64::EncodeError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FlagEffects {
    read: Flags,
    written: Flags,
    undefined: Flags,
}

impl FlagEffects {
    pub(crate) const fn new(read: Flags, written: Flags, undefined: Flags) -> FlagEffects {
        FlagEffects {
            read,
            written,
            undefined,
        }
    }

    pub(crate) const fn with_read(self, read: Flags) -> FlagEffects {
        FlagEffects {
            read: self.read.union(read),
            ..self
        }
    }

    /// Flags the instruction reads.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Cond, Flags, Inst, Mnemonic, Reg};
    ///
    /// let set = encode(&Inst::new(Mnemonic::Setcc(Cond::B), [Reg::AL.into()]))?;
    /// assert_eq!(set.flags().read(), Flags::CF);
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn read(&self) -> Flags {
        self.read
    }

    /// Flags the instruction sets to a defined value.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Reg};
    ///
    /// let inc = encode(&Inst::new(Mnemonic::Inc, [Reg::ECX.into()]))?;
    /// assert!(!inc.flags().written().contains(Flags::CF));
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn written(&self) -> Flags {
        self.written
    }

    /// Flags the instruction leaves undefined.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Reg};
    ///
    /// let div = encode(&Inst::new(Mnemonic::Div, [Reg::RCX.into()]))?;
    /// assert_eq!(div.flags().undefined(), Flags::ALL);
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn undefined(&self) -> Flags {
        self.undefined
    }

    /// Every flag whose value may differ afterwards: written or undefined.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Reg};
    ///
    /// let mov = encode(&Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Reg::EBX.into()]))?;
    /// assert_eq!(mov.flags().clobbered(), Flags::NONE);
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn clobbered(&self) -> Flags {
        self.written.union(self.undefined)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_flags_set_operations() {
        let f = Flags::CF | Flags::OF;
        assert!(f.contains(Flags::CF) && f.contains(Flags::OF) && !f.contains(Flags::ZF));
        assert!(Flags::ALL.contains(f));
        assert!(!f.is_empty() && Flags::NONE.is_empty() && Flags::default().is_empty());
        assert_eq!(Flags::ALL.bits(), 0x8D5);
    }

    #[test]
    fn test_flags_display_order() {
        assert_eq!(Flags::ALL.to_string(), "CF|PF|AF|ZF|SF|OF");
        assert_eq!((Flags::OF | Flags::CF).to_string(), "CF|OF");
        assert_eq!(alloc::format!("{:?}", Flags::NONE), "none");
    }

    #[test]
    fn test_flag_effects_clobbered() {
        let e = FlagEffects::new(Flags::CF, Flags::ZF, Flags::AF).with_read(Flags::OF);
        assert_eq!(e.read(), Flags::CF | Flags::OF);
        assert_eq!(e.clobbered(), Flags::ZF | Flags::AF);
    }
}
