//! The encoder: interprets form records to turn an [`Inst`] into bytes.
//!
//! There is no per-instruction code here. For each candidate form of the
//! mnemonic, in table order, the operands are matched against the form's
//! operand kinds and placed into the fields its slots name (ModRM.reg,
//! ModRM.rm/SIB/displacement, the opcode's low bits, the immediate); the
//! first form that fits is emitted. Memory operands always take the shortest
//! ModRM/SIB/displacement encoding, so equal instructions encode equally.

use core::fmt;
use core::ops::Deref;

use super::error::EncodeError;
use super::flags::FlagEffects;
use super::form::{ATTR_NOT_R0, Form, ModRmSpec, OpcodeField, OperandKind, Slot};
use super::inst::Inst;
use super::operand::{Cond, Mem, Operand};
use super::reg::{Reg, Size};
use super::table;

/// The longest x86-64 instruction, in bytes.
pub const MAX_INST_LEN: usize = 15;

/// An encoded instruction: up to 15 bytes on the stack, plus the form that
/// produced them. Dereferences to `[u8]`.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode, Inst, Mnemonic, Reg};
///
/// let enc = encode(&Inst::new(Mnemonic::Push, [Reg::R12.into()]))?;
/// assert_eq!(&*enc, [0x41, 0x54]);
/// assert_eq!(enc.len(), 2);
/// let mut code = Vec::new();
/// code.extend_from_slice(&enc);
/// # Ok::<(), isa_lang::x86_64::EncodeError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encoded {
    bytes: [u8; MAX_INST_LEN],
    flags: FlagEffects,
    form: u16,
    len: u8,
    disp_at: u8,
    imm_at: u8,
}

impl Encoded {
    /// The encoded bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Inst, Mnemonic};
    ///
    /// assert_eq!(encode(&Inst::new(Mnemonic::Ret, []))?.as_bytes(), [0xC3]);
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len).min(MAX_INST_LEN)]
    }

    /// The table row the instruction was encoded with.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Inst, Mnemonic, Reg};
    ///
    /// let enc = encode(&Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Reg::ECX.into()]))?;
    /// assert_eq!(enc.form().syntax(), "MOV r/m32, r32");
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[must_use]
    pub fn form(&self) -> &'static Form {
        // Every `Encoded` is built from a table form, so the lookup succeeds;
        // the fallback keeps this total without a panic path.
        table::form(self.form).unwrap_or(&table::FORMS[0])
    }

    /// What the instruction does to the flags, with a condition code's reads
    /// filled in.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{encode, Cond, Flags, Inst, Mnemonic, Operand};
    ///
    /// let jl = encode(&Inst::new(Mnemonic::Jcc(Cond::L), [Operand::Rel(0)]))?;
    /// assert_eq!(jl.flags().read(), Flags::SF | Flags::OF);
    /// # Ok::<(), isa_lang::x86_64::EncodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> FlagEffects {
        self.flags
    }

    /// Offset of the 32-bit displacement of a RIP-relative operand, if any.
    pub(crate) const fn rip_disp_at(&self) -> Option<usize> {
        if self.disp_at == 0 {
            None
        } else {
            Some(self.disp_at as usize)
        }
    }

    /// Offset of the immediate field, if any.
    pub(crate) const fn imm_at(&self) -> Option<usize> {
        if self.imm_at == 0 {
            None
        } else {
            Some(self.imm_at as usize)
        }
    }
}

impl Deref for Encoded {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<[u8]> for Encoded {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for Encoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Encoded(")?;
        for (i, b) in self.as_bytes().iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02x}")?;
        }
        f.write_str(")")
    }
}

/// Encodes one instruction, choosing the shortest form that fits.
///
/// Forms are tried in table order, which puts shorter forms first: a small
/// immediate takes the sign-extended 8-bit form, the accumulator forms are
/// used where they are shorter, a near branch takes `rel8`.
///
/// # Errors
///
/// [`EncodeError`] when no form takes the operands, an immediate or branch
/// target is out of range, a memory operand is malformed, or a high-byte
/// register meets a REX prefix.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode, Inst, Mem, Mnemonic, Operand, Reg, Size};
///
/// let i = Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(8)]);
/// assert_eq!(&*encode(&i)?, [0x48, 0x83, 0xC0, 0x08]);
///
/// let store = Inst::new(Mnemonic::Mov, [Mem::new(Size::Dword, Reg::RSP).with_disp(4).into(), Reg::R9D.into()]);
/// assert_eq!(&*encode(&store)?, [0x44, 0x89, 0x4C, 0x24, 0x04]);
/// # Ok::<(), isa_lang::x86_64::EncodeError>(())
/// ```
pub fn encode(inst: &Inst) -> Result<Encoded, EncodeError> {
    check_memory(inst)?;
    let (op, cond) = inst.mnemonic().parts();
    let mut best = Miss::Kind;
    let ops = inst.operands();
    for &id in table::by_op(op) {
        let Some(form) = table::form(id) else {
            continue;
        };
        // Most candidates fail on operand count or kinds; skip those before
        // any layout work.
        if ops.len() != usize::from(form.count)
            || !ops
                .iter()
                .zip(form.kinds.iter())
                .all(|(&o, &k)| kind_fits(k, o))
        {
            continue;
        }
        match try_form(form, inst, cond) {
            Ok(encoded) => return Ok(encoded),
            Err(miss) => best = best.closer(miss),
        }
    }
    Err(best.into_error(inst))
}

/// Encodes one instruction with exactly the given form.
///
/// This is the instruction selector's door: a pattern that names a form gets
/// that form's bytes, or an error if the operands do not fit it. The
/// `rel8`/`rel32` and register-or-memory choices are the caller's.
///
/// # Errors
///
/// [`EncodeError::NoForm`] if the form belongs to another mnemonic or does
/// not take these operands; the other variants as for [`encode`].
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{encode_form, forms, Inst, Mnemonic, Operand, Reg};
///
/// // Force the 32-bit immediate form even though 8 bits would do.
/// let wide = forms().iter().find(|f| f.syntax() == "ADD r/m32, imm32").unwrap();
/// let i = Inst::new(Mnemonic::Add, [Reg::ECX.into(), Operand::Imm(1)]);
/// assert_eq!(&*encode_form(wide, &i)?, [0x81, 0xC1, 0x01, 0x00, 0x00, 0x00]);
/// # Ok::<(), isa_lang::x86_64::EncodeError>(())
/// ```
pub fn encode_form(form: &Form, inst: &Inst) -> Result<Encoded, EncodeError> {
    check_memory(inst)?;
    let (op, cond) = inst.mnemonic().parts();
    // Use the table's own record: `form` may be a copy, but its id is real.
    let form = table::form(form.id).unwrap_or(form);
    if form.op != op {
        return Err(EncodeError::NoForm {
            mnemonic: inst.mnemonic(),
        });
    }
    try_form(form, inst, cond).map_err(|miss| miss.into_error(inst))
}

/// Why one form did not fit, ordered from farthest to closest.
#[derive(Clone, Copy, Debug)]
enum Miss {
    Kind,
    HighByte,
    Imm(i64),
    Target(i64),
}

impl Miss {
    const fn rank(self) -> u8 {
        match self {
            Miss::Kind => 0,
            Miss::HighByte => 1,
            Miss::Imm(_) => 2,
            Miss::Target(_) => 3,
        }
    }

    fn closer(self, other: Miss) -> Miss {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    fn into_error(self, inst: &Inst) -> EncodeError {
        match self {
            Miss::Kind => EncodeError::NoForm {
                mnemonic: inst.mnemonic(),
            },
            Miss::HighByte => EncodeError::HighByteWithRex,
            Miss::Imm(value) => EncodeError::ImmediateOutOfRange { value },
            Miss::Target(offset) => EncodeError::TargetOutOfRange { offset },
        }
    }
}

/// Rejects memory operands no form can encode, before any form is tried.
fn check_memory(inst: &Inst) -> Result<(), EncodeError> {
    for op in inst.operands() {
        let Operand::Mem(m) = op else { continue };
        if let Some(base) = m.base() {
            if base.size() != Size::Qword {
                return Err(EncodeError::BaseNotQword { base });
            }
        }
        if let Some(index) = m.index() {
            if m.is_rip_relative() {
                return Err(EncodeError::RipWithIndex);
            }
            if index.size() != Size::Qword {
                return Err(EncodeError::IndexNotQword { index });
            }
            if index.num() == 4 {
                return Err(EncodeError::RspIndex);
            }
        }
    }
    Ok(())
}

/// Whether `op` has the kind and width `kind` asks for. Immediate and branch
/// ranges are checked separately, because a range miss is a closer miss.
fn kind_fits(kind: OperandKind, op: Operand) -> bool {
    match (kind, op) {
        (OperandKind::Reg(size), Operand::Reg(r)) => r.size() == size,
        (OperandKind::RegMem(size), Operand::Reg(r)) => r.size() == size,
        (OperandKind::RegMem(size) | OperandKind::Mem(size), Operand::Mem(m)) => m.size() == size,
        (OperandKind::Fixed(reg), Operand::Reg(r)) => r == reg,
        (OperandKind::One, Operand::Imm(v)) => v == 1,
        (OperandKind::Imm { .. }, Operand::Imm(_)) => true,
        (OperandKind::Rel { .. }, Operand::Rel(_)) => true,
        _ => false,
    }
}

/// Whether `value` fits an immediate field of `bits` bits.
pub(crate) const fn imm_fits(value: i64, bits: u8, signed: bool) -> bool {
    if bits >= 64 {
        return signed || value >= 0;
    }
    if signed {
        let half = 1i64 << (bits - 1);
        value >= -half && value < half
    } else {
        value >= 0 && value < (1i64 << bits)
    }
}

/// The ModRM/SIB/displacement part of an encoding.
#[derive(Clone, Copy)]
enum Disp {
    None,
    Byte(i8),
    Dword(i32),
}

impl Disp {
    const fn len(self) -> usize {
        match self {
            Disp::None => 0,
            Disp::Byte(_) => 1,
            Disp::Dword(_) => 4,
        }
    }
}

/// An instruction being laid out against one form.
struct Layout<'f> {
    form: &'f Form,
    rex: u8,
    force_rex: bool,
    high_byte: bool,
    opcode: u8,
    reg_field: u8,
    md: u8,
    rm_field: u8,
    sib: Option<u8>,
    disp: Disp,
    rip: bool,
}

impl<'f> Layout<'f> {
    fn new(form: &'f Form) -> Layout<'f> {
        Layout {
            form,
            rex: if form.rex_w { 0b1000 } else { 0 },
            force_rex: false,
            high_byte: false,
            opcode: form.opcode,
            reg_field: match form.modrm {
                ModRmSpec::Ext(n) => n,
                ModRmSpec::Reg | ModRmSpec::Absent => 0,
            },
            md: 0,
            rm_field: 0,
            sib: None,
            disp: Disp::None,
            rip: false,
        }
    }

    /// Records REX needs of a register placed in an encoded field.
    fn note_byte_reg(&mut self, r: Reg) {
        if r.is_high_byte() {
            self.high_byte = true;
        } else if r.needs_rex() {
            self.force_rex = true;
        }
    }

    fn reg(&mut self, r: Reg) {
        self.note_byte_reg(r);
        self.reg_field = r.num() & 7;
        self.rex |= (r.num() >> 3) << 2;
    }

    fn opcode_reg(&mut self, r: Reg) {
        self.note_byte_reg(r);
        self.opcode |= r.num() & 7;
        self.rex |= r.num() >> 3;
    }

    fn rm_reg(&mut self, r: Reg) {
        self.note_byte_reg(r);
        self.md = 0b11;
        self.rm_field = r.num() & 7;
        self.rex |= r.num() >> 3;
    }

    fn rm_mem(&mut self, m: Mem) {
        let disp = m.disp();
        if m.is_rip_relative() {
            // mod=00 rm=101 is [rip+disp32] in 64-bit mode.
            self.md = 0b00;
            self.rm_field = 0b101;
            self.disp = Disp::Dword(disp);
            self.rip = true;
            return;
        }
        let index_bits = match m.index() {
            Some(index) => {
                self.rex |= (index.num() >> 3) << 1;
                index.num() & 7
            }
            None => 0b100,
        };
        let scale = m.scale().bits() << 6;
        match m.base() {
            None => {
                // No base: SIB with base=101 and mod=00 means disp32 only.
                self.md = 0b00;
                self.rm_field = 0b100;
                self.sib = Some(scale | (index_bits << 3) | 0b101);
                self.disp = Disp::Dword(disp);
            }
            Some(base) => {
                self.rex |= base.num() >> 3;
                let low = base.num() & 7;
                // RBP/R13 have no mod=00 form (that encoding is RIP or
                // disp32), so they take a zero 8-bit displacement.
                (self.md, self.disp) = if disp == 0 && low != 0b101 {
                    (0b00, Disp::None)
                } else if let Ok(d) = i8::try_from(disp) {
                    (0b01, Disp::Byte(d))
                } else {
                    (0b10, Disp::Dword(disp))
                };
                // RSP/R12 as base, or any index, need a SIB byte.
                if m.index().is_some() || low == 0b100 {
                    self.rm_field = 0b100;
                    self.sib = Some(scale | (index_bits << 3) | low);
                } else {
                    self.rm_field = low;
                }
            }
        }
    }

    fn needs_rex(&self) -> bool {
        self.rex != 0 || self.force_rex
    }

    /// Bytes before the immediate.
    fn prefix_len(&self) -> usize {
        let f = self.form;
        let modrm = if matches!(f.modrm, ModRmSpec::Absent) {
            0
        } else {
            1 + usize::from(self.sib.is_some()) + self.disp.len()
        };
        usize::from(f.p66) + usize::from(self.needs_rex()) + usize::from(f.map) + 1 + modrm
    }

    fn write(&self, imm: i64) -> Encoded {
        let f = self.form;
        let mut out = Encoded {
            bytes: [0; MAX_INST_LEN],
            flags: f.flags,
            form: f.id,
            len: 0,
            disp_at: 0,
            imm_at: 0,
        };
        let mut n = 0usize;
        let mut put = |byte: u8, n: &mut usize| {
            if let Some(slot) = out.bytes.get_mut(*n) {
                *slot = byte;
            }
            *n += 1;
        };
        if f.p66 {
            put(0x66, &mut n);
        }
        if self.needs_rex() {
            put(0x40 | self.rex, &mut n);
        }
        if f.map == 1 {
            put(0x0F, &mut n);
        }
        put(self.opcode, &mut n);
        let mut disp_at = 0;
        if !matches!(f.modrm, ModRmSpec::Absent) {
            put(
                (self.md << 6) | (self.reg_field << 3) | self.rm_field,
                &mut n,
            );
            if let Some(sib) = self.sib {
                put(sib, &mut n);
            }
            if self.rip {
                disp_at = n;
            }
            match self.disp {
                Disp::None => {}
                Disp::Byte(d) => put(d as u8, &mut n),
                Disp::Dword(d) => {
                    for b in d.to_le_bytes() {
                        put(b, &mut n);
                    }
                }
            }
        }
        let imm_at = if f.imm_bytes > 0 { n } else { 0 };
        for b in imm.to_le_bytes().iter().take(usize::from(f.imm_bytes)) {
            put(*b, &mut n);
        }
        // Instruction lengths here never exceed 14 bytes, so these fit.
        out.len = n.min(MAX_INST_LEN) as u8;
        out.disp_at = disp_at as u8;
        out.imm_at = imm_at as u8;
        out
    }
}

/// Lays `inst` out against `form`, or says how close it came.
fn try_form(form: &Form, inst: &Inst, cond: Option<Cond>) -> Result<Encoded, Miss> {
    let ops = inst.operands();
    if ops.len() != usize::from(form.count) {
        return Err(Miss::Kind);
    }
    let mut layout = Layout::new(form);
    if matches!(form.field, OpcodeField::Cond) {
        layout.opcode |= cond.map_or(0, Cond::code);
    }
    let mut imm = 0i64;
    let mut rel: Option<(i64, u8)> = None;
    let mut imm_miss = None;
    for (i, &op) in ops.iter().enumerate() {
        let (Some(&kind), Some(&slot)) = (form.kinds.get(i), form.slots.get(i)) else {
            return Err(Miss::Kind);
        };
        if !kind_fits(kind, op) {
            return Err(Miss::Kind);
        }
        match (slot, op) {
            (Slot::Implied, _) => {}
            (Slot::Reg, Operand::Reg(r)) => layout.reg(r),
            (Slot::Opcode, Operand::Reg(r)) => {
                if form.attrs & ATTR_NOT_R0 != 0 && r.num() == 0 {
                    return Err(Miss::Kind);
                }
                layout.opcode_reg(r);
            }
            (Slot::Rm, Operand::Reg(r)) => layout.rm_reg(r),
            (Slot::Rm, Operand::Mem(m)) => layout.rm_mem(m),
            (Slot::Imm, Operand::Imm(v)) => match kind {
                OperandKind::Imm { bits, signed } if imm_fits(v, bits, signed) => imm = v,
                _ => imm_miss = Some(v),
            },
            (Slot::Imm, Operand::Rel(d)) => match kind {
                OperandKind::Rel { bits } => rel = Some((d, bits)),
                _ => return Err(Miss::Kind),
            },
            _ => return Err(Miss::Kind),
        }
    }
    if layout.needs_rex() && layout.high_byte {
        return Err(Miss::HighByte);
    }
    if let Some(v) = imm_miss {
        return Err(Miss::Imm(v));
    }
    if let Some((target, bits)) = rel {
        let len = (layout.prefix_len() + usize::from(form.imm_bytes)) as i64;
        match target.checked_sub(len) {
            Some(r) if imm_fits(r, bits, true) => imm = r,
            _ => return Err(Miss::Target(target)),
        }
    }
    let mut encoded = layout.write(imm);
    if form.reads_cond {
        if let Some(c) = cond {
            encoded.flags = encoded.flags.with_read(c.reads());
        }
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::x86_64::{Mnemonic, Scale};

    fn enc(m: Mnemonic, ops: &[Operand]) -> Result<alloc::vec::Vec<u8>, EncodeError> {
        let inst = Inst::from_slice(m, ops).ok_or(EncodeError::NoForm { mnemonic: m })?;
        encode(&inst).map(|e| e.to_vec())
    }

    #[test]
    fn test_imm_fits_boundaries() {
        assert!(imm_fits(127, 8, true) && imm_fits(-128, 8, true));
        assert!(!imm_fits(128, 8, true) && !imm_fits(-129, 8, true));
        assert!(imm_fits(255, 8, false) && !imm_fits(256, 8, false) && !imm_fits(-1, 8, false));
        assert!(imm_fits(i64::MIN, 64, true) && imm_fits(i64::MAX, 64, true));
        assert!(imm_fits(i32::MIN.into(), 32, true) && !imm_fits(1 << 31, 32, true));
        assert!(imm_fits(65535, 16, false) && !imm_fits(65536, 16, false));
    }

    #[test]
    fn test_memory_shapes() {
        let mov = |m: Mem| enc(Mnemonic::Mov, &[Reg::EAX.into(), m.into()]);
        let d = Size::Dword;
        // [rax] -> mod=00 rm=000
        assert_eq!(mov(Mem::new(d, Reg::RAX)), Ok(vec![0x8B, 0x00]));
        // [rbp] -> mod=01 rm=101 disp8=0
        assert_eq!(mov(Mem::new(d, Reg::RBP)), Ok(vec![0x8B, 0x45, 0x00]));
        // [r13] -> REX.B, mod=01 rm=101 disp8=0
        assert_eq!(mov(Mem::new(d, Reg::R13)), Ok(vec![0x41, 0x8B, 0x45, 0x00]));
        // [rsp] -> SIB 24
        assert_eq!(mov(Mem::new(d, Reg::RSP)), Ok(vec![0x8B, 0x04, 0x24]));
        // [r12] -> REX.B, SIB 24
        assert_eq!(mov(Mem::new(d, Reg::R12)), Ok(vec![0x41, 0x8B, 0x04, 0x24]));
        // [rip+0x10]
        assert_eq!(mov(Mem::rip(d, 16)), Ok(vec![0x8B, 0x05, 0x10, 0, 0, 0]));
        // [0x10] absolute -> SIB 25 disp32
        assert_eq!(
            mov(Mem::absolute(d, 16)),
            Ok(vec![0x8B, 0x04, 0x25, 0x10, 0, 0, 0])
        );
        // [rcx*4+0x10] -> SIB 8D disp32
        let idx = Mem::absolute(d, 16).with_index(Reg::RCX, Scale::X4);
        assert_eq!(mov(idx), Ok(vec![0x8B, 0x04, 0x8D, 0x10, 0, 0, 0]));
        // [rax+r12*8-0x80] -> REX.X, mod=01, SIB E0, disp8 80
        let m = Mem::new(d, Reg::RAX)
            .with_index(Reg::R12, Scale::X8)
            .with_disp(-128);
        assert_eq!(mov(m), Ok(vec![0x42, 0x8B, 0x44, 0xE0, 0x80]));
        // disp 128 needs disp32
        assert_eq!(
            mov(Mem::new(d, Reg::RAX).with_disp(128)),
            Ok(vec![0x8B, 0x80, 0x80, 0, 0, 0])
        );
    }

    #[test]
    fn test_memory_errors() {
        let mov = |m: Mem| enc(Mnemonic::Mov, &[Reg::EAX.into(), m.into()]);
        let d = Size::Dword;
        assert_eq!(
            mov(Mem::new(d, Reg::EAX)),
            Err(EncodeError::BaseNotQword { base: Reg::EAX })
        );
        let bad_index = Mem::new(d, Reg::RAX).with_index(Reg::ECX, Scale::X1);
        assert_eq!(
            mov(bad_index),
            Err(EncodeError::IndexNotQword { index: Reg::ECX })
        );
        let rsp = Mem::new(d, Reg::RAX).with_index(Reg::RSP, Scale::X1);
        assert_eq!(mov(rsp), Err(EncodeError::RspIndex));
        let rip = Mem::rip(d, 0).with_index(Reg::RAX, Scale::X2);
        assert_eq!(mov(rip), Err(EncodeError::RipWithIndex));
    }

    #[test]
    fn test_high_byte_rules() {
        assert_eq!(
            enc(Mnemonic::Mov, &[Reg::AH.into(), Reg::BL.into()]),
            Ok(vec![0x88, 0xDC])
        );
        assert_eq!(
            enc(Mnemonic::Mov, &[Reg::SIL.into(), Reg::AL.into()]),
            Ok(vec![0x40, 0x88, 0xC6])
        );
        assert_eq!(
            enc(Mnemonic::Mov, &[Reg::AH.into(), Reg::SIL.into()]),
            Err(EncodeError::HighByteWithRex)
        );
        assert_eq!(
            enc(Mnemonic::Movzx, &[Reg::RAX.into(), Reg::AH.into()]),
            Err(EncodeError::HighByteWithRex)
        );
        assert_eq!(
            enc(Mnemonic::Movzx, &[Reg::EAX.into(), Reg::AH.into()]),
            Ok(vec![0x0F, 0xB6, 0xC4])
        );
    }

    #[test]
    fn test_branch_selection() {
        let jmp = |d| enc(Mnemonic::Jmp, &[Operand::Rel(d)]);
        assert_eq!(jmp(2), Ok(vec![0xEB, 0x00]));
        assert_eq!(jmp(129), Ok(vec![0xEB, 0x7F]));
        assert_eq!(jmp(130), Ok(vec![0xE9, 0x7D, 0, 0, 0]));
        assert_eq!(jmp(-126), Ok(vec![0xEB, 0x80]));
        assert_eq!(jmp(-127), Ok(vec![0xE9, 0x7C, 0xFF, 0xFF, 0xFF]));
        let far = i64::from(i32::MAX) + 6;
        assert_eq!(jmp(far), Err(EncodeError::TargetOutOfRange { offset: far }));
        assert_eq!(
            jmp(i64::MIN),
            Err(EncodeError::TargetOutOfRange { offset: i64::MIN })
        );
    }

    #[test]
    fn test_not_r0_avoids_nop() {
        assert_eq!(
            enc(Mnemonic::Xchg, &[Reg::EBX.into(), Reg::EAX.into()]),
            Ok(vec![0x93])
        );
        assert_eq!(
            enc(Mnemonic::Xchg, &[Reg::EAX.into(), Reg::EAX.into()]),
            Ok(vec![0x87, 0xC0])
        );
        assert_eq!(
            enc(Mnemonic::Xchg, &[Reg::RAX.into(), Reg::RAX.into()]),
            Ok(vec![0x48, 0x87, 0xC0])
        );
        assert_eq!(
            enc(Mnemonic::Xchg, &[Reg::R8.into(), Reg::RAX.into()]),
            Ok(vec![0x49, 0x90])
        );
    }

    #[test]
    fn test_closest_miss_wins() {
        assert_eq!(
            enc(Mnemonic::Shl, &[Reg::EAX.into(), Operand::Imm(256)]),
            Err(EncodeError::ImmediateOutOfRange { value: 256 })
        );
        assert_eq!(
            enc(Mnemonic::Ret, &[Reg::RAX.into()]),
            Err(EncodeError::NoForm {
                mnemonic: Mnemonic::Ret
            })
        );
    }

    #[test]
    fn test_encode_form_rejects_other_mnemonic() {
        let add = table::by_op(crate::x86_64::inst::Op::Add)[0];
        let form = table::form(add).copied();
        let inst = Inst::new(Mnemonic::Sub, [Reg::AL.into(), Operand::Imm(1)]);
        assert_eq!(
            form.map(|f| encode_form(&f, &inst)),
            Some(Err(EncodeError::NoForm {
                mnemonic: Mnemonic::Sub
            }))
        );
    }
}
