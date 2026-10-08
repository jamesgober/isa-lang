//! The assembler: a code buffer with labels, fixups, and branch relaxation.
//!
//! Instructions whose bytes do not depend on a label are encoded at once
//! into one flat buffer. Label-relative instructions are recorded beside it:
//! `jmp`/`jcc` to a label as relaxable branch sites (`rel8` or `rel32`, chosen
//! at the end), and `call` or RIP-relative references to a label as 32-bit
//! fields patched once the layout is known.
//!
//! Relaxation starts with every branch short and lengthens only branches
//! that cannot reach, until none is out of range. Lengthening only ever
//! moves code apart, so a branch that has to be long stays long, and the
//! result is the least fixed point: the same set of long branches whatever
//! order they were found in, and none longer than necessary. Each
//! lengthening re-checks only the short branches within 256 bytes of it
//! (a short branch spans at most 129 bytes), with positions kept in a
//! Fenwick tree, so the whole pass is O(n log n) in the number of branches.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use super::encode::{encode, encode_form};
use super::error::EncodeError;
use super::inst::{Inst, Mnemonic, Op};
use super::nop::recommended_nop;
use super::operand::{Cond, Operand};
use super::table;

/// A position in an [`Assembler`]'s code, bound once with
/// [`Assembler::bind`]. Labels are only meaningful to the assembler that
/// made them.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::Assembler;
///
/// let mut asm = Assembler::new();
/// let a = asm.label();
/// let b = asm.label();
/// assert_ne!(a, b);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(u32);

impl Label {
    /// The label's number within its assembler, in creation order.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let mut asm = Assembler::new();
    /// assert_eq!(asm.label().index(), 0);
    /// assert_eq!(asm.label().index(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Label({})", self.0)
    }
}

/// Why the assembler refused an instruction or could not finish.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{AsmError, Assembler};
///
/// let mut asm = Assembler::new();
/// let nowhere = asm.label();
/// asm.jmp(nowhere)?;
/// assert_eq!(asm.finish().unwrap_err(), AsmError::Unbound(nowhere));
/// # Ok::<(), AsmError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AsmError {
    /// The instruction could not be encoded.
    Encode(EncodeError),
    /// The label was not made by this assembler.
    UnknownLabel(Label),
    /// The label is already bound.
    Rebound(Label),
    /// A branch or reference uses a label that was never bound.
    Unbound(Label),
    /// [`Assembler::emit_rip`] was given an instruction without a
    /// RIP-relative memory operand.
    NoRipOperand,
    /// The code would exceed [`Assembler::MAX_CODE`] bytes, beyond which a
    /// 32-bit displacement cannot reach every label.
    TooLarge,
}

impl From<EncodeError> for AsmError {
    fn from(e: EncodeError) -> AsmError {
        AsmError::Encode(e)
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::Encode(e) => fmt::Display::fmt(e, f),
            AsmError::UnknownLabel(l) => write!(f, "label {} was not made by this assembler", l.0),
            AsmError::Rebound(l) => write!(f, "label {} is already bound", l.0),
            AsmError::Unbound(l) => write!(f, "label {} is used but never bound", l.0),
            AsmError::NoRipOperand => {
                f.write_str("the instruction has no rip-relative memory operand to resolve")
            }
            AsmError::TooLarge => write!(f, "code exceeds {} bytes", Assembler::MAX_CODE),
        }
    }
}

impl core::error::Error for AsmError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            AsmError::Encode(e) => Some(e),
            _ => None,
        }
    }
}

/// Where a label is: an offset in the fixed-size code plus the number of
/// relaxable branches recorded before it.
#[derive(Clone, Copy)]
struct Anchor {
    offset: u32,
    branches: u32,
    bound: bool,
}

/// A `jmp`/`jcc` to a label, relaxed at `finish`.
#[derive(Clone, Copy)]
struct Branch {
    at: u32,
    label: u32,
    cond: Option<Cond>,
}

/// A 32-bit field to resolve against a label once the layout is final.
#[derive(Clone, Copy)]
struct Patch {
    field: u32,
    end: u32,
    branches: u32,
    label: u32,
    addend: i32,
}

/// Short branches are two bytes (`EB cb`, `7x cb`).
const SHORT: u64 = 2;
/// Lengthening adds 3 bytes to `jmp` (`E9 cd`) and 4 to `jcc` (`0F 8x cd`).
const fn growth(cond: Option<Cond>) -> u64 {
    if cond.is_some() { 4 } else { 3 }
}
/// Short branches whose start is farther than this from a lengthened branch
/// cannot span it.
const WINDOW: i64 = 256;

const JMP_REL8: u16 = table::find_form(Op::Jmp, 1, 0);
const JMP_REL32: u16 = table::find_form(Op::Jmp, 4, 0);
const JCC_REL8: u16 = table::find_form(Op::Jcc, 1, 0);
const JCC_REL32: u16 = table::find_form(Op::Jcc, 4, 1);

/// An x86-64 assembler: emit instructions, branch to labels, bind them, and
/// `finish` into machine code.
///
/// `jmp` and `jcc` to a label are relaxed: each is 2 bytes (`rel8`) when the
/// final layout lets it reach, and 5 or 6 bytes (`rel32`) otherwise, with
/// the fewest long branches possible. `call` and RIP-relative references to
/// labels always use 32-bit displacements.
///
/// # Examples
///
/// A loop summing `1..=n`, with `n` in `edi`:
///
/// ```
/// use isa_lang::x86_64::{decode_all, Assembler, Cond, Inst, Mnemonic, Operand, Reg};
///
/// let mut asm = Assembler::new();
/// let top = asm.label();
/// let done = asm.label();
/// asm.emit(&Inst::new(Mnemonic::Xor, [Reg::EAX.into(), Reg::EAX.into()]))?;
/// asm.emit(&Inst::new(Mnemonic::Test, [Reg::EDI.into(), Reg::EDI.into()]))?;
/// asm.jcc(Cond::E, done)?;
/// asm.bind(top)?;
/// asm.emit(&Inst::new(Mnemonic::Add, [Reg::EAX.into(), Reg::EDI.into()]))?;
/// asm.emit(&Inst::new(Mnemonic::Dec, [Reg::EDI.into()]))?;
/// asm.jcc(Cond::Ne, top)?;
/// asm.bind(done)?;
/// asm.emit(&Inst::new(Mnemonic::Ret, []))?;
/// let code = asm.finish()?;
///
/// assert_eq!(code.bytes().len(), 13); // both branches are short
/// assert_eq!(code.label_offset(top), Some(6));
/// let text: Vec<String> = decode_all(code.bytes())
///     .map(|r| r.map(|(at, d)| d.inst().display_at(at as u64).to_string()))
///     .collect::<Result<_, _>>()
///     .unwrap();
/// assert_eq!(text[2], "je 0xc");
/// assert_eq!(text[5], "jne 0x6");
/// # Ok::<(), isa_lang::x86_64::AsmError>(())
/// ```
#[derive(Clone, Default)]
pub struct Assembler {
    code: Vec<u8>,
    branches: Vec<Branch>,
    patches: Vec<Patch>,
    labels: Vec<Anchor>,
}

impl fmt::Debug for Assembler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assembler")
            .field("fixed_bytes", &self.code.len())
            .field("branches", &self.branches.len())
            .field("patches", &self.patches.len())
            .field("labels", &self.labels.len())
            .finish()
    }
}

impl Assembler {
    /// The most code one assembler produces: `i32::MAX` bytes, so every
    /// 32-bit displacement between two points of it fits.
    pub const MAX_CODE: usize = i32::MAX as usize;

    /// An empty assembler.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// assert!(Assembler::new().finish()?.bytes().is_empty());
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    #[must_use]
    pub fn new() -> Assembler {
        Assembler::default()
    }

    /// An empty assembler with room for `bytes` bytes of code.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let asm = Assembler::with_capacity(4096);
    /// # let _ = asm;
    /// ```
    #[must_use]
    pub fn with_capacity(bytes: usize) -> Assembler {
        Assembler {
            code: Vec::with_capacity(bytes),
            ..Assembler::default()
        }
    }

    /// A new, unbound label.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let mut asm = Assembler::new();
    /// let l = asm.label();
    /// asm.bind(l)?;
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn label(&mut self) -> Label {
        // Past 2^32 labels the id saturates to one that never binds.
        let id = u32::try_from(self.labels.len()).unwrap_or(u32::MAX);
        if id != u32::MAX {
            self.labels.push(Anchor {
                offset: 0,
                branches: 0,
                bound: false,
            });
        }
        Label(id)
    }

    /// Binds `label` to the current position.
    ///
    /// # Errors
    ///
    /// [`AsmError::Rebound`] if it is already bound, [`AsmError::UnknownLabel`]
    /// if another assembler made it.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{AsmError, Assembler};
    ///
    /// let mut asm = Assembler::new();
    /// let l = asm.label();
    /// asm.bind(l)?;
    /// assert_eq!(asm.bind(l), Err(AsmError::Rebound(l)));
    /// # Ok::<(), AsmError>(())
    /// ```
    pub fn bind(&mut self, label: Label) -> Result<(), AsmError> {
        let offset = self.code.len() as u32;
        let branches = self.branches.len() as u32;
        let anchor = self
            .labels
            .get_mut(label.index())
            .ok_or(AsmError::UnknownLabel(label))?;
        if anchor.bound {
            return Err(AsmError::Rebound(label));
        }
        *anchor = Anchor {
            offset,
            branches,
            bound: true,
        };
        Ok(())
    }

    /// Encodes `inst` at the current position.
    ///
    /// `Rel` operands are encoded as given, relative to this instruction's
    /// final address; to reach other code in this assembler, use labels
    /// ([`jmp`](Assembler::jmp), [`jcc`](Assembler::jcc), [`call`](Assembler::call),
    /// [`emit_rip`](Assembler::emit_rip)).
    ///
    /// # Errors
    ///
    /// [`AsmError::Encode`] if the instruction cannot be encoded,
    /// [`AsmError::TooLarge`] past [`MAX_CODE`](Assembler::MAX_CODE).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Inst, Mnemonic};
    ///
    /// let mut asm = Assembler::new();
    /// asm.emit(&Inst::new(Mnemonic::Ud2, []))?;
    /// assert_eq!(asm.finish()?.bytes(), [0x0F, 0x0B]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn emit(&mut self, inst: &Inst) -> Result<(), AsmError> {
        let encoded = encode(inst)?;
        self.append(&encoded)
    }

    /// Emits raw bytes: data, or code from elsewhere.
    ///
    /// # Errors
    ///
    /// [`AsmError::TooLarge`] past [`MAX_CODE`](Assembler::MAX_CODE).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let mut asm = Assembler::new();
    /// asm.bytes(&[0xDE, 0xAD])?;
    /// assert_eq!(asm.finish()?.bytes(), [0xDE, 0xAD]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn bytes(&mut self, data: &[u8]) -> Result<(), AsmError> {
        self.append(data)
    }

    /// Emits `len` bytes of no-ops, using the architecture manual's
    /// recommended multi-byte `NOP` sequences (at most 9 bytes each).
    ///
    /// # Errors
    ///
    /// [`AsmError::TooLarge`] past [`MAX_CODE`](Assembler::MAX_CODE).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{decode_all, Assembler};
    ///
    /// let mut asm = Assembler::new();
    /// asm.nops(12)?;
    /// let code = asm.finish()?;
    /// assert_eq!(code.bytes().len(), 12);
    /// assert_eq!(decode_all(code.bytes()).count(), 2); // 9 + 3
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn nops(&mut self, len: usize) -> Result<(), AsmError> {
        self.reserve(len)?;
        let mut left = len;
        while left > 0 {
            let step = left.min(9);
            self.code
                .extend_from_slice(recommended_nop(step).unwrap_or(&[]));
            left -= step;
        }
        Ok(())
    }

    /// Emits `jmp label`, as `rel8` or `rel32` as the final layout allows.
    ///
    /// # Errors
    ///
    /// [`AsmError::UnknownLabel`] for another assembler's label,
    /// [`AsmError::TooLarge`] past [`MAX_CODE`](Assembler::MAX_CODE).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let mut asm = Assembler::new();
    /// let here = asm.label();
    /// asm.bind(here)?;
    /// asm.jmp(here)?;
    /// assert_eq!(asm.finish()?.bytes(), [0xEB, 0xFE]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn jmp(&mut self, label: Label) -> Result<(), AsmError> {
        self.branch(label, None)
    }

    /// Emits `jcc label`, as `rel8` or `rel32` as the final layout allows.
    ///
    /// # Errors
    ///
    /// As for [`jmp`](Assembler::jmp).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Cond};
    ///
    /// let mut asm = Assembler::new();
    /// let skip = asm.label();
    /// asm.jcc(Cond::Ae, skip)?;
    /// asm.nops(200)?;
    /// asm.bind(skip)?;
    /// assert_eq!(asm.finish()?.bytes()[..6], [0x0F, 0x83, 0xC8, 0x00, 0x00, 0x00]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn jcc(&mut self, cond: Cond, label: Label) -> Result<(), AsmError> {
        self.branch(label, Some(cond))
    }

    /// Emits `call label` (always `rel32`).
    ///
    /// # Errors
    ///
    /// As for [`jmp`](Assembler::jmp).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Inst, Mnemonic};
    ///
    /// let mut asm = Assembler::new();
    /// let f = asm.label();
    /// asm.call(f)?;
    /// asm.emit(&Inst::new(Mnemonic::Ret, []))?;
    /// asm.bind(f)?;
    /// asm.emit(&Inst::new(Mnemonic::Ret, []))?;
    /// assert_eq!(asm.finish()?.bytes(), [0xE8, 0x01, 0x00, 0x00, 0x00, 0xC3, 0xC3]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn call(&mut self, label: Label) -> Result<(), AsmError> {
        self.check_label(label)?;
        let encoded = encode(&Inst::new(Mnemonic::Call, [Operand::Rel(5)]))?;
        let field = encoded.imm_at().unwrap_or(1);
        self.patched(&encoded, field, label, 0)
    }

    /// Emits `inst`, whose RIP-relative memory operand is resolved to point
    /// at `label`. The operand's displacement is added to the label's
    /// address, so `Mem::rip(size, 8)` addresses `label + 8`.
    ///
    /// # Errors
    ///
    /// [`AsmError::NoRipOperand`] if `inst` has no RIP-relative operand; the
    /// errors of [`emit`](Assembler::emit) and [`jmp`](Assembler::jmp).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Inst, Mem, Mnemonic, Reg, Size};
    ///
    /// let mut asm = Assembler::new();
    /// let data = asm.label();
    /// asm.emit_rip(&Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Mem::rip(Size::Qword, 0).into()]), data)?;
    /// asm.emit(&Inst::new(Mnemonic::Ret, []))?;
    /// asm.bind(data)?;
    /// asm.bytes(&42u64.to_le_bytes())?;
    /// let code = asm.finish()?;
    /// // mov rax, [rip+1] skips the one-byte ret.
    /// assert_eq!(code.bytes()[..7], [0x48, 0x8B, 0x05, 0x01, 0x00, 0x00, 0x00]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn emit_rip(&mut self, inst: &Inst, label: Label) -> Result<(), AsmError> {
        self.check_label(label)?;
        let addend = inst
            .operands()
            .iter()
            .find_map(|op| match op {
                Operand::Mem(m) if m.is_rip_relative() => Some(m.disp()),
                _ => None,
            })
            .ok_or(AsmError::NoRipOperand)?;
        let encoded = encode(inst)?;
        let field = encoded.rip_disp_at().ok_or(AsmError::NoRipOperand)?;
        self.patched(&encoded, field, label, addend)
    }

    /// Resolves every label and returns the machine code.
    ///
    /// # Errors
    ///
    /// [`AsmError::Unbound`] if a branch or reference names a label that was
    /// never bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Inst, Mnemonic};
    ///
    /// let mut asm = Assembler::new();
    /// let end = asm.label();
    /// asm.jmp(end)?;
    /// asm.emit(&Inst::new(Mnemonic::Int3, []))?;
    /// asm.bind(end)?;
    /// let code = asm.finish()?;
    /// assert_eq!(code.bytes(), [0xEB, 0x01, 0xCC]);
    /// assert_eq!(code.label_offset(end), Some(3));
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    pub fn finish(self) -> Result<Code, AsmError> {
        let layout = Layout::relax(&self)?;
        let mut out = Vec::with_capacity(layout.total);
        let mut prev = 0usize;
        for (i, b) in self.branches.iter().enumerate() {
            let at = b.at as usize;
            out.extend_from_slice(self.code.get(prev..at).unwrap_or(&[]));
            let start = layout.branch_start(i, b.at);
            let target = layout.label_pos(self.anchor(b.label)?);
            let mnemonic = match b.cond {
                Some(c) => Mnemonic::Jcc(c),
                None => Mnemonic::Jmp,
            };
            let form = match (b.cond.is_some(), layout.long[i]) {
                (false, false) => JMP_REL8,
                (false, true) => JMP_REL32,
                (true, false) => JCC_REL8,
                (true, true) => JCC_REL32,
            };
            let inst = Inst::new(mnemonic, [Operand::Rel(target - start)]);
            let form = table::form(form).ok_or(AsmError::TooLarge)?;
            out.extend_from_slice(&encode_form(form, &inst)?);
            prev = at;
        }
        out.extend_from_slice(self.code.get(prev..).unwrap_or(&[]));
        for p in &self.patches {
            let shift = layout.shift(p.branches);
            let field = (i64::from(p.field) + shift) as usize;
            let end = i64::from(p.end) + shift;
            let disp = layout.label_pos(self.anchor(p.label)?) + i64::from(p.addend) - end;
            let disp = i32::try_from(disp).map_err(|_| AsmError::TooLarge)?;
            if let Some(slot) = out.get_mut(field..field + 4) {
                slot.copy_from_slice(&disp.to_le_bytes());
            }
        }
        let labels = self
            .labels
            .iter()
            .map(|&a| {
                if a.bound {
                    layout.label_pos(a) as u32
                } else {
                    u32::MAX
                }
            })
            .collect();
        Ok(Code { bytes: out, labels })
    }

    /// The anchor of a bound label.
    fn anchor(&self, label: u32) -> Result<Anchor, AsmError> {
        match self.labels.get(label as usize) {
            Some(&a) if a.bound => Ok(a),
            _ => Err(AsmError::Unbound(Label(label))),
        }
    }

    fn check_label(&self, label: Label) -> Result<(), AsmError> {
        if label.index() < self.labels.len() {
            Ok(())
        } else {
            Err(AsmError::UnknownLabel(label))
        }
    }

    /// Checks that `extra` more bytes, plus the worst-case growth of every
    /// branch, stay within `MAX_CODE`.
    fn reserve(&self, extra: usize) -> Result<(), AsmError> {
        let worst = self
            .code
            .len()
            .checked_add(extra)
            .and_then(|n| n.checked_add(self.branches.len().checked_mul(6)?));
        match worst {
            Some(n) if n <= Assembler::MAX_CODE => Ok(()),
            _ => Err(AsmError::TooLarge),
        }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), AsmError> {
        self.reserve(bytes.len())?;
        self.code.extend_from_slice(bytes);
        Ok(())
    }

    fn branch(&mut self, label: Label, cond: Option<Cond>) -> Result<(), AsmError> {
        self.check_label(label)?;
        self.reserve(6)?;
        self.branches.push(Branch {
            at: self.code.len() as u32,
            label: label.0,
            cond,
        });
        Ok(())
    }

    fn patched(
        &mut self,
        bytes: &[u8],
        field: usize,
        label: Label,
        addend: i32,
    ) -> Result<(), AsmError> {
        let start = self.code.len();
        self.append(bytes)?;
        self.patches.push(Patch {
            field: (start + field) as u32,
            end: (start + bytes.len()) as u32,
            branches: self.branches.len() as u32,
            label: label.0,
            addend,
        });
        Ok(())
    }
}

/// The relaxed layout: which branches are long, and positions through a
/// Fenwick tree of how much each long branch grew.
struct Layout {
    long: Vec<bool>,
    growth: Fenwick,
    total: usize,
}

impl Layout {
    fn relax(asm: &Assembler) -> Result<Layout, AsmError> {
        // Every referenced label must be bound before positions mean anything.
        let unbound = |label: u32| match asm.labels.get(label as usize) {
            Some(a) if a.bound => None,
            _ => Some(AsmError::Unbound(Label(label))),
        };
        for b in &asm.branches {
            if let Some(e) = unbound(b.label) {
                return Err(e);
            }
        }
        for p in &asm.patches {
            if let Some(e) = unbound(p.label) {
                return Err(e);
            }
        }
        let n = asm.branches.len();
        let mut layout = Layout {
            long: vec![false; n],
            growth: Fenwick::new(n),
            total: 0,
        };
        let mut work = Worklist {
            queued: vec![false; n],
            stack: Vec::new(),
        };
        for (i, b) in asm.branches.iter().enumerate() {
            if !layout.reaches(asm, i, b) {
                work.push(i);
            }
        }
        while let Some(i) = work.stack.pop() {
            let Some(&b) = asm.branches.get(i) else {
                continue;
            };
            layout.long[i] = true;
            layout.growth.add(i, growth(b.cond));
            let here = layout.branch_start(i, b.at);
            // Only short branches near `i` can span it; check them again.
            // Branch starts strictly increase, so each scan stops at the
            // first branch outside the window.
            let before = asm
                .branches
                .get(..i)
                .unwrap_or(&[])
                .iter()
                .enumerate()
                .rev();
            for (j, bj) in before {
                if layout.branch_start(j, bj.at) < here - WINDOW {
                    break;
                }
                if !work.is_queued(j) && !layout.reaches(asm, j, bj) {
                    work.push(j);
                }
            }
            let after = asm.branches.iter().enumerate().skip(i + 1);
            for (j, bj) in after {
                if layout.branch_start(j, bj.at) > here + WINDOW {
                    break;
                }
                if !work.is_queued(j) && !layout.reaches(asm, j, bj) {
                    work.push(j);
                }
            }
        }
        let grown = layout.growth.prefix(n);
        layout.total = asm.code.len() + (SHORT as usize) * n + grown as usize;
        Ok(layout)
    }

    /// Bytes inserted before a point that follows `branches` branch sites.
    fn shift(&self, branches: u32) -> i64 {
        (SHORT * u64::from(branches) + self.growth.prefix(branches as usize)) as i64
    }

    fn label_pos(&self, a: Anchor) -> i64 {
        i64::from(a.offset) + self.shift(a.branches)
    }

    fn branch_start(&self, i: usize, at: u32) -> i64 {
        i64::from(at) + self.shift(i as u32)
    }

    /// Whether short branch `i` reaches its label in the current layout.
    fn reaches(&self, asm: &Assembler, i: usize, b: &Branch) -> bool {
        let Some(&anchor) = asm.labels.get(b.label as usize) else {
            return false;
        };
        let end = self.branch_start(i, b.at) + SHORT as i64;
        let disp = self.label_pos(anchor) - end;
        (-128..=127).contains(&disp)
    }
}

/// Branches found out of range and not yet lengthened. Each is queued at
/// most once: a branch that cannot reach never reaches again.
struct Worklist {
    queued: Vec<bool>,
    stack: Vec<usize>,
}

impl Worklist {
    fn is_queued(&self, i: usize) -> bool {
        self.queued.get(i).copied().unwrap_or(true)
    }

    fn push(&mut self, i: usize) {
        if let Some(q) = self.queued.get_mut(i) {
            *q = true;
            self.stack.push(i);
        }
    }
}

/// Prefix sums over branch growth, updated one branch at a time.
struct Fenwick {
    tree: Vec<u64>,
}

impl Fenwick {
    fn new(n: usize) -> Fenwick {
        Fenwick { tree: vec![0; n] }
    }

    fn add(&mut self, index: usize, value: u64) {
        let mut i = index + 1;
        while let Some(slot) = self.tree.get_mut(i - 1) {
            *slot += value;
            i += i & i.wrapping_neg();
        }
    }

    /// The sum over indices `0..end`.
    fn prefix(&self, end: usize) -> u64 {
        let mut i = end.min(self.tree.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.tree.get(i - 1).copied().unwrap_or(0);
            i &= i - 1;
        }
        sum
    }
}

/// Finished machine code and the final offsets of its labels.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Assembler, Inst, Mnemonic};
///
/// let mut asm = Assembler::new();
/// let entry = asm.label();
/// asm.bind(entry)?;
/// asm.emit(&Inst::new(Mnemonic::Ret, []))?;
/// let code = asm.finish()?;
/// assert_eq!(code.label_offset(entry), Some(0));
/// assert_eq!(code.into_bytes(), vec![0xC3]);
/// # Ok::<(), isa_lang::x86_64::AsmError>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Code {
    bytes: Vec<u8>,
    labels: Vec<u32>,
}

impl Code {
    /// The machine code.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Assembler, Inst, Mnemonic};
    ///
    /// let mut asm = Assembler::new();
    /// asm.emit(&Inst::new(Mnemonic::Cqo, []))?;
    /// assert_eq!(asm.finish()?.bytes(), [0x48, 0x99]);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The machine code, by value.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// assert!(Assembler::new().finish()?.into_bytes().is_empty());
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Where `label` ended up, or `None` if it was never bound (or belongs
    /// to another assembler).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::Assembler;
    ///
    /// let mut asm = Assembler::new();
    /// let unused = asm.label();
    /// assert_eq!(asm.finish()?.label_offset(unused), None);
    /// # Ok::<(), isa_lang::x86_64::AsmError>(())
    /// ```
    #[must_use]
    pub fn label_offset(&self, label: Label) -> Option<usize> {
        match self.labels.get(label.index()) {
            Some(&off) if off != u32::MAX => Some(off as usize),
            _ => None,
        }
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Code")
            .field("len", &self.bytes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x86_64::Reg;

    #[test]
    fn test_fenwick_prefix_sums() {
        let mut f = Fenwick::new(10);
        for i in 0..10 {
            f.add(i, i as u64 + 1);
        }
        for end in 0..=10 {
            let expect: u64 = (1..=end as u64).sum();
            assert_eq!(f.prefix(end), expect);
        }
        assert_eq!(f.prefix(100), 55);
        let empty = Fenwick::new(0);
        assert_eq!(empty.prefix(5), 0);
    }

    #[test]
    fn test_branch_forms_are_the_table_rows() {
        let syntax = |id| table::form(id).map(|f| f.syntax());
        assert_eq!(syntax(JMP_REL8), Some("JMP rel8"));
        assert_eq!(syntax(JMP_REL32), Some("JMP rel32"));
        assert_eq!(syntax(JCC_REL8), Some("Jcc rel8"));
        assert_eq!(syntax(JCC_REL32), Some("Jcc rel32"));
    }

    #[test]
    fn test_unknown_label_from_another_assembler() {
        let mut other = Assembler::new();
        let _ = other.label();
        let foreign = other.label();
        let mut asm = Assembler::new();
        assert_eq!(asm.jmp(foreign), Err(AsmError::UnknownLabel(foreign)));
        assert_eq!(asm.bind(foreign), Err(AsmError::UnknownLabel(foreign)));
        assert_eq!(asm.call(foreign), Err(AsmError::UnknownLabel(foreign)));
    }

    #[test]
    fn test_emit_rip_requires_rip_operand() {
        let mut asm = Assembler::new();
        let l = asm.label();
        let inst = Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Reg::RBX.into()]);
        assert_eq!(asm.emit_rip(&inst, l), Err(AsmError::NoRipOperand));
    }

    #[test]
    fn test_boundary_127_and_128() {
        // A forward jmp over exactly 127 bytes is short; 128 is long.
        for (gap, expect_len) in [(127usize, 2usize), (128, 5)] {
            let mut asm = Assembler::new();
            let l = asm.label();
            assert!(asm.jmp(l).is_ok());
            assert!(asm.bytes(&vec![0x90; gap]).is_ok());
            assert!(asm.bind(l).is_ok());
            let code = asm.finish();
            assert_eq!(code.map(|c| c.bytes().len() - gap), Ok(expect_len));
        }
    }
}
