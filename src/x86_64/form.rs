//! Instruction forms: one row of the target description, compiled.
//!
//! A form is one line of the architecture manual's opcode table: an encoding
//! recipe (`REX.W 83 /0 ib`) and an operand signature (`ADD r/m64, imm8`),
//! plus the flags it touches. The encoder and decoder never contain
//! per-instruction code; they interpret these records. See
//! `_lexersketch/specs/TARGET-DESC.md` for the model.

use core::fmt;

use super::flags::FlagEffects;
use super::inst::Op;
use super::reg::{Reg, Size};

/// The kind of operand a form accepts in one position.
///
/// This is the operand-signature half of the target description, with the
/// architecture manual's names: `r64` is `Reg(Qword)`, `r/m32` is
/// `RegMem(Dword)`, `imm8` is `Imm { bits: 8, signed: true }`.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{forms, OperandKind, Size};
///
/// let add = forms().iter().find(|f| f.syntax() == "ADD r/m64, imm8").unwrap();
/// assert_eq!(
///     add.operands(),
///     [OperandKind::RegMem(Size::Qword), OperandKind::Imm { bits: 8, signed: true }],
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperandKind {
    /// A register of this width (`r8` ... `r64`).
    Reg(Size),
    /// A register or a memory operand of this width (`r/m8` ... `r/m64`).
    RegMem(Size),
    /// A memory operand of this width. `LEA`'s `m` uses the operation's width.
    Mem(Size),
    /// This exact register, implied by the opcode (`AL`, `EAX`, `CL`, ...).
    Fixed(Reg),
    /// The constant 1, implied by the opcode (the shift-by-one forms).
    One,
    /// An immediate field of `bits` bits. A signed field is sign-extended to
    /// the operation's width; an unsigned one is a count or index.
    Imm {
        /// Field width: 8, 16, 32, or 64.
        bits: u8,
        /// Whether the field is sign-extended.
        signed: bool,
    },
    /// A relative branch displacement field of `bits` bits.
    Rel {
        /// Field width: 8 or 32.
        bits: u8,
    },
}

/// Where an operand lives in the encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Not encoded: fixed registers and the constant 1.
    Implied,
    /// ModRM.reg (with REX.R).
    Reg,
    /// ModRM.rm, SIB, and displacement (with REX.B and REX.X).
    Rm,
    /// The low three bits of the opcode byte (with REX.B).
    Opcode,
    /// The immediate or relative-displacement field.
    Imm,
}

/// What the ModRM byte carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModRmSpec {
    /// No ModRM byte.
    Absent,
    /// `/r`: ModRM.reg is a register operand.
    Reg,
    /// `/digit`: ModRM.reg is an opcode extension.
    Ext(u8),
}

/// What the opcode byte's low bits carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpcodeField {
    /// Nothing: the opcode is one value.
    Fixed,
    /// `+rb`/`+rw`/`+rd`/`+ro`: a register in bits 0-2.
    Reg,
    /// `+cc`: a condition code in bits 0-3.
    Cond,
}

/// Attribute: the operation is 64-bit without REX.W (push, pop, indirect
/// near jmp/call). A redundant REX.W is accepted when decoding.
pub(crate) const ATTR_D64: u8 = 1;
/// Attribute: the `+r` register must not be number 0, because `90` is `NOP`.
pub(crate) const ATTR_NOT_R0: u8 = 2;

/// One compiled row of the x86-64 target description: an encoding recipe,
/// an operand signature, and its flag effects.
///
/// Forms are obtained from [`forms`](crate::x86_64::forms), and from
/// [`Encoded::form`](crate::x86_64::Encoded::form) and
/// [`Decoded::form`](crate::x86_64::Decoded::form), which say which row an
/// instruction was encoded with or decoded from. They are the hook for the
/// instruction selector: a pattern names a form, and
/// [`encode_form`](crate::x86_64::encode_form) emits exactly that form.
///
/// `Display` prints the row as the table spells it.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode, Inst, Mnemonic, Operand, Reg};
///
/// let enc = encode(&Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(8)]))?;
/// let form = enc.form();
/// assert_eq!(form.encoding(), "REX.W 83 /0 ib");
/// assert_eq!(form.syntax(), "ADD r/m64, imm8");
/// assert_eq!(form.to_string(), "REX.W 83 /0 ib | ADD r/m64, imm8");
/// # Ok::<(), isa_lang::x86_64::EncodeError>(())
/// ```
#[derive(Clone, Copy)]
pub struct Form {
    pub(crate) kinds: [OperandKind; 3],
    pub(crate) slots: [Slot; 3],
    pub(crate) flags: FlagEffects,
    pub(crate) id: u16,
    pub(crate) op: Op,
    pub(crate) count: u8,
    pub(crate) map: u8,
    pub(crate) opcode: u8,
    pub(crate) field: OpcodeField,
    pub(crate) modrm: ModRmSpec,
    pub(crate) imm_bytes: u8,
    pub(crate) p66: bool,
    pub(crate) rex_w: bool,
    pub(crate) attrs: u8,
    pub(crate) reads_cond: bool,
    /// The ModRM.rm operand must be memory (`LEA`'s `m`).
    pub(crate) rm_mem_only: bool,
    pub(crate) encoding: &'static str,
    pub(crate) syntax: &'static str,
}

impl Form {
    /// The encoding recipe, in the architecture manual's notation
    /// (`66`, `REX.W`, opcode bytes, `+rd`, `+cc`, `/r`, `/digit`, `ib`,
    /// `iw`, `id`, `io`, `cb`, `cd`).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::forms;
    ///
    /// assert!(forms().iter().any(|f| f.encoding() == "0F 80+cc cd"));
    /// ```
    #[inline]
    #[must_use]
    pub const fn encoding(&self) -> &'static str {
        self.encoding
    }

    /// The operand signature, in the architecture manual's notation
    /// (`MOVZX r32, r/m8`), with `uimm8`/`uimm16` marking unsigned fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::forms;
    ///
    /// assert!(forms().iter().any(|f| f.syntax() == "SHL r/m32, CL"));
    /// ```
    #[inline]
    #[must_use]
    pub const fn syntax(&self) -> &'static str {
        self.syntax
    }

    /// The operand kinds, destination first.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{forms, OperandKind, Reg};
    ///
    /// let f = forms().iter().find(|f| f.syntax() == "SHL r/m32, CL").unwrap();
    /// assert_eq!(f.operands()[1], OperandKind::Fixed(Reg::CL));
    /// ```
    #[inline]
    #[must_use]
    pub fn operands(&self) -> &[OperandKind] {
        &self.kinds[..usize::from(self.count).min(3)]
    }

    /// What the form does to the flags. For `Jcc`, `SETcc`, and `CMOVcc` the
    /// flags read depend on the condition; see
    /// [`reads_condition`](Form::reads_condition) and [`Cond::reads`](crate::x86_64::Cond::reads).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{forms, Flags};
    ///
    /// let f = forms().iter().find(|f| f.syntax() == "SUB r/m64, r64").unwrap();
    /// assert_eq!(f.flags().written(), Flags::ALL);
    /// ```
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> FlagEffects {
        self.flags
    }

    /// Whether the form reads the flags named by its condition code.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::forms;
    ///
    /// let f = forms().iter().find(|f| f.syntax() == "Jcc rel8").unwrap();
    /// assert!(f.reads_condition());
    /// ```
    #[inline]
    #[must_use]
    pub const fn reads_condition(&self) -> bool {
        self.reads_cond
    }

    /// The form's position in [`forms`](crate::x86_64::forms). Stable within
    /// a release; a dense id for tables keyed by form.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::forms;
    ///
    /// for (i, f) in forms().iter().enumerate() {
    ///     assert_eq!(f.index(), i);
    /// }
    /// ```
    #[inline]
    #[must_use]
    pub const fn index(&self) -> usize {
        self.id as usize
    }
}

impl PartialEq for Form {
    fn eq(&self, other: &Form) -> bool {
        self.id == other.id
    }
}

impl Eq for Form {}

impl fmt::Display for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} | {}", self.encoding, self.syntax)
    }
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Form({self})")
    }
}
