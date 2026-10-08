//! The decoder: interprets form records to turn bytes back into an [`Inst`].
//!
//! Prefixes are read (`66`, then an optional REX), the opcode selects a
//! bucket of candidate forms from the build-time index, and the first
//! candidate whose prefixes, ModRM extension, register-or-memory constraint,
//! and REX usage all match decodes the operands through its slots. Every read
//! is bounds-checked, so no input can make it panic; anything the table does
//! not describe is a [`DecodeError`] naming the byte at fault.

use core::fmt;
use core::iter::FusedIterator;

use super::error::DecodeError;
use super::flags::FlagEffects;
use super::form::{ATTR_D64, ATTR_NOT_R0, Form, ModRmSpec, OpcodeField, OperandKind, Slot};
use super::inst::{Inst, MAX_OPERANDS, Mnemonic};
use super::operand::{Cond, Mem, Operand, Scale};
use super::reg::{Reg, Size};
use super::table;

/// A decoded instruction: the [`Inst`], how many bytes it took, and the form
/// it matched.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::decode;
///
/// let d = decode(&[0x48, 0x8B, 0x44, 0x24, 0x08, 0xC3])?;
/// assert_eq!(d.len(), 5);
/// assert_eq!(d.inst().to_string(), "mov rax, qword ptr [rsp+0x8]");
/// assert_eq!(d.form().syntax(), "MOV r64, r/m64");
/// # Ok::<(), isa_lang::x86_64::DecodeError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Decoded {
    inst: Inst,
    flags: FlagEffects,
    form: u16,
    len: u8,
}

impl Decoded {
    /// The instruction.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{decode, Inst, Mnemonic};
    ///
    /// assert_eq!(decode(&[0xCC])?.inst(), Inst::new(Mnemonic::Int3, []));
    /// # Ok::<(), isa_lang::x86_64::DecodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn inst(&self) -> Inst {
        self.inst
    }

    /// The number of bytes the instruction occupies (1 to 15).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::decode;
    ///
    /// assert_eq!(decode(&[0x0F, 0x1F, 0x44, 0x00, 0x00])?.len(), 5);
    /// # Ok::<(), isa_lang::x86_64::DecodeError>(())
    /// ```
    #[allow(clippy::len_without_is_empty)] // A decoded instruction is never empty.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// The table row the bytes matched.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::decode;
    ///
    /// assert_eq!(decode(&[0x05, 1, 0, 0, 0])?.form().syntax(), "ADD EAX, imm32");
    /// # Ok::<(), isa_lang::x86_64::DecodeError>(())
    /// ```
    #[must_use]
    pub fn form(&self) -> &'static Form {
        // Every `Decoded` is built from a table form, so the lookup succeeds;
        // the fallback keeps this total without a panic path.
        table::form(self.form).unwrap_or(&table::FORMS[0])
    }

    /// What the instruction does to the flags, with a condition code's reads
    /// filled in.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{decode, Flags};
    ///
    /// // sete al
    /// assert_eq!(decode(&[0x0F, 0x94, 0xC0])?.flags().read(), Flags::ZF);
    /// # Ok::<(), isa_lang::x86_64::DecodeError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> FlagEffects {
        self.flags
    }
}

/// Decodes the instruction at the start of `bytes`.
///
/// Trailing bytes are ignored; [`Decoded::len`] says how many were used.
///
/// # Errors
///
/// A [`DecodeError`] naming the offending byte when the bytes do not start
/// with an instruction the form table describes, or end too soon.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{decode, encode, DecodeError, Inst, Mnemonic, Operand, Reg};
///
/// let inst = Inst::new(Mnemonic::Sub, [Reg::RSP.into(), Operand::Imm(40)]);
/// let bytes = encode(&inst).unwrap();
/// assert_eq!(decode(&bytes)?.inst(), inst);
///
/// assert_eq!(decode(&[]), Err(DecodeError::Truncated { at: 0 }));
/// # Ok::<(), DecodeError>(())
/// ```
pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    Decoder { bytes, pos: 0 }.run()
}

/// Bounds-checked little-endian reads.
struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Decoder<'_> {
    #[inline]
    fn peek(&self) -> Result<u8, DecodeError> {
        self.bytes
            .get(self.pos)
            .copied()
            .ok_or(DecodeError::Truncated { at: self.pos })
    }

    #[inline]
    fn byte(&mut self) -> Result<u8, DecodeError> {
        let b = self.peek()?;
        self.pos += 1;
        Ok(b)
    }

    /// Reads `n` (1, 2, 4, or 8) bytes as a little-endian integer, sign- or
    /// zero-extended to 64 bits.
    fn int(&mut self, n: usize, signed: bool) -> Result<i64, DecodeError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(DecodeError::Truncated { at: self.pos })?;
        let Some(field) = self.bytes.get(self.pos..end) else {
            return Err(DecodeError::Truncated {
                at: self.bytes.len().max(self.pos),
            });
        };
        let mut raw = [0u8; 8];
        for (dst, src) in raw.iter_mut().zip(field) {
            *dst = *src;
        }
        self.pos = end;
        let value = u64::from_le_bytes(raw);
        let bits = (n * 8) as u32;
        Ok(if bits >= 64 {
            value as i64
        } else if signed {
            let shift = 64 - bits;
            ((value << shift) as i64) >> shift
        } else {
            value as i64
        })
    }

    fn run(mut self) -> Result<Decoded, DecodeError> {
        let mut p66 = false;
        loop {
            let b = self.peek()?;
            match b {
                0x66 if p66 => return Err(DecodeError::RepeatedPrefix { at: self.pos }),
                0x66 => p66 = true,
                0xF0 | 0xF2 | 0xF3 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 | 0x67 => {
                    return Err(DecodeError::UnsupportedPrefix {
                        at: self.pos,
                        byte: b,
                    });
                }
                _ => break,
            }
            self.pos += 1;
        }
        let mut rex = Rex::NONE;
        if self.peek()? & 0xF0 == 0x40 {
            rex = Rex {
                bits: self.byte()?,
                at: self.pos - 1,
            };
            if is_prefix(self.peek()?) {
                return Err(DecodeError::MisplacedRex { at: rex.at });
            }
        }
        let mut map = 0u8;
        if self.peek()? == 0x0F {
            map = 1;
            self.pos += 1;
        }
        let opcode_at = self.pos;
        let opcode = self.byte()?;
        let candidates = table::by_opcode(map, opcode);
        if candidates.is_empty() {
            return Err(DecodeError::UnknownOpcode {
                at: opcode_at,
                escaped: map == 1,
                opcode,
            });
        }
        let mut modrm: Option<ModRm> = None;
        let mut miss = Miss::Size;
        for &id in candidates {
            let Some(form) = table::form(id) else {
                continue;
            };
            if !size_matches(form, p66, rex) || !opreg_allowed(form, opcode, rex) {
                continue;
            }
            if !matches!(form.modrm, ModRmSpec::Absent) && modrm.is_none() {
                let at = self.pos;
                let byte = self.byte()?;
                modrm = Some(ModRm {
                    md: byte >> 6,
                    reg: (byte >> 3) & 7,
                    rm: byte & 7,
                    at,
                });
            }
            match check(form, modrm, rex, &mut self)? {
                Ok(()) => return self.operands(form, opcode, modrm, rex),
                Err(m) => miss = miss.closer(m),
            }
        }
        Err(miss.into_error(opcode_at, modrm, rex))
    }

    /// Decodes the operands of the form that matched.
    fn operands(
        mut self,
        form: &Form,
        opcode: u8,
        modrm: Option<ModRm>,
        rex: Rex,
    ) -> Result<Decoded, DecodeError> {
        let has_rex = rex.present();
        let rm_operand = match modrm {
            Some(m) if m.md == 3 => RmOperand::Reg(rex.b() | m.rm),
            Some(m) => RmOperand::Mem(self.address(m, rex)?),
            None => RmOperand::None,
        };
        let mut ops = [Operand::Imm(0); MAX_OPERANDS];
        let mut rel_slot = None;
        let count = usize::from(form.count).min(MAX_OPERANDS);
        for i in 0..count {
            let (Some(&kind), Some(&slot)) = (form.kinds.get(i), form.slots.get(i)) else {
                continue;
            };
            let size = kind_size(kind);
            let op = match (slot, kind) {
                (Slot::Implied, OperandKind::Fixed(r)) => Operand::Reg(r),
                (Slot::Implied, _) => Operand::Imm(1),
                (Slot::Reg, _) => {
                    let num = rex.r() | modrm.map_or(0, |m| m.reg);
                    Operand::Reg(Reg::decode(size, num, has_rex))
                }
                (Slot::Opcode, _) => {
                    Operand::Reg(Reg::decode(size, rex.b() | (opcode & 7), has_rex))
                }
                (Slot::Rm, _) => match rm_operand {
                    RmOperand::Reg(num) => Operand::Reg(Reg::decode(size, num, has_rex)),
                    RmOperand::Mem(addr) => Operand::Mem(addr.with_size(size)),
                    RmOperand::None => Operand::Imm(0),
                },
                (Slot::Imm, OperandKind::Rel { .. }) => {
                    rel_slot = Some(i);
                    Operand::Rel(self.int(usize::from(form.imm_bytes), true)?)
                }
                (Slot::Imm, OperandKind::Imm { signed, .. }) => {
                    Operand::Imm(self.int(usize::from(form.imm_bytes), signed)?)
                }
                (Slot::Imm, _) => Operand::Imm(0),
            };
            if let Some(o) = ops.get_mut(i) {
                *o = op;
            }
        }
        let len = self.pos;
        // A relative target is stored from the start of the instruction.
        if let Some(Operand::Rel(r)) = rel_slot.and_then(|i| ops.get_mut(i)) {
            *r += len as i64;
        }
        let cond = Cond::from_code(opcode & 15).unwrap_or(Cond::O);
        let mnemonic = Mnemonic::from_parts(form.op, cond);
        let inst = Inst::from_slice(mnemonic, ops.get(..count).unwrap_or(&[]))
            .unwrap_or(Inst::new(mnemonic, []));
        let mut flags = form.flags;
        if form.reads_cond {
            flags = flags.with_read(cond.reads());
        }
        Ok(Decoded {
            inst,
            flags,
            form: form.id,
            len: len.min(15) as u8,
        })
    }

    /// Decodes a ModRM memory operand (mod != 11), with SIB and displacement.
    /// The size is filled in by the caller.
    fn address(&mut self, m: ModRm, rex: Rex) -> Result<Mem, DecodeError> {
        if m.md == 0 && m.rm == 0b101 {
            let disp = self.int(4, true)? as i32;
            return Ok(Mem::rip(Size::Qword, disp));
        }
        let (base, index, scale) = if m.rm == 0b100 {
            let sib = self.byte()?;
            let index_num = rex.x() | ((sib >> 3) & 7);
            let index = (index_num != 0b100).then(|| Reg::decode(Size::Qword, index_num, true));
            let base_low = sib & 7;
            let base = if base_low == 0b101 && m.md == 0 {
                None
            } else {
                Some(Reg::decode(Size::Qword, rex.b() | base_low, true))
            };
            (base, index, Scale::from_bits(sib >> 6))
        } else {
            (
                Some(Reg::decode(Size::Qword, rex.b() | m.rm, true)),
                None,
                Scale::X1,
            )
        };
        let disp = match (m.md, base) {
            (0, None) | (2, _) => self.int(4, true)? as i32,
            (1, _) => self.int(1, true)? as i32,
            _ => 0,
        };
        Ok(Mem::decoded(Size::Qword, base, index, scale, disp))
    }
}

/// The ModRM byte, split.
#[derive(Clone, Copy)]
struct ModRm {
    md: u8,
    reg: u8,
    rm: u8,
    at: usize,
}

/// The REX prefix, if any.
#[derive(Clone, Copy)]
struct Rex {
    bits: u8,
    at: usize,
}

impl Rex {
    const NONE: Rex = Rex {
        bits: 0,
        at: usize::MAX,
    };

    fn present(self) -> bool {
        self.bits != 0
    }
    fn w(self) -> bool {
        self.bits & 8 != 0
    }
    fn r(self) -> u8 {
        (self.bits & 4) << 1
    }
    fn x(self) -> u8 {
        (self.bits & 2) << 2
    }
    fn b(self) -> u8 {
        (self.bits & 1) << 3
    }
}

enum RmOperand {
    None,
    Reg(u8),
    Mem(Mem),
}

/// Why a candidate form did not match, ordered from farthest to closest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Miss {
    Size,
    Extension,
    Register,
    Rex,
}

impl Miss {
    fn closer(self, other: Miss) -> Miss {
        self.max(other)
    }

    fn into_error(self, opcode_at: usize, modrm: Option<ModRm>, rex: Rex) -> DecodeError {
        match (self, modrm) {
            (Miss::Extension, Some(m)) => DecodeError::UnknownExtension {
                at: m.at,
                ext: m.reg,
            },
            (Miss::Register, Some(m)) => DecodeError::RegisterOperand { at: m.at },
            (Miss::Rex, _) => DecodeError::UnusedRex { at: rex.at },
            _ => DecodeError::InvalidOperandSize { at: opcode_at },
        }
    }
}

fn is_prefix(b: u8) -> bool {
    matches!(
        b,
        0x66 | 0xF0 | 0xF2 | 0xF3 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 | 0x67
    ) || b & 0xF0 == 0x40
}

/// `66` must be present exactly when the form has it; REX.W likewise, except
/// that default-64 forms accept a redundant REX.W.
fn size_matches(form: &Form, p66: bool, rex: Rex) -> bool {
    p66 == form.p66 && (rex.w() == form.rex_w || (form.attrs & ATTR_D64 != 0 && !form.rex_w))
}

/// The `XCHG r, eAX` forms exclude register 0: `90` is `NOP`.
fn opreg_allowed(form: &Form, opcode: u8, rex: Rex) -> bool {
    !(matches!(form.field, OpcodeField::Reg)
        && form.attrs & ATTR_NOT_R0 != 0
        && opcode & 7 == 0
        && rex.b() == 0)
}

/// Checks the ModRM-dependent conditions. The outer `Result` is a hard
/// error (truncation while reading the SIB byte); the inner one says whether
/// this form matches.
fn check(
    form: &Form,
    modrm: Option<ModRm>,
    rex: Rex,
    decoder: &mut Decoder<'_>,
) -> Result<Result<(), Miss>, DecodeError> {
    let mut uses_r = false;
    let mut uses_x = false;
    let mut uses_b = matches!(form.field, OpcodeField::Reg);
    if let Some(m) = modrm.filter(|_| !matches!(form.modrm, ModRmSpec::Absent)) {
        if let ModRmSpec::Ext(n) = form.modrm {
            if m.reg != n {
                return Ok(Err(Miss::Extension));
            }
        } else {
            uses_r = true;
        }
        if m.md == 3 {
            if form.rm_mem_only {
                return Ok(Err(Miss::Register));
            }
            uses_b = true;
        } else if m.rm == 0b100 {
            // The SIB byte decides whether X and B are used; peek at it.
            let sib = decoder.peek()?;
            uses_x = true;
            uses_b = !(sib & 7 == 0b101 && m.md == 0);
        } else {
            uses_b = !(m.rm == 0b101 && m.md == 0);
        }
    }
    let unused = (rex.bits & 4 != 0 && !uses_r)
        || (rex.bits & 2 != 0 && !uses_x)
        || (rex.bits & 1 != 0 && !uses_b);
    Ok(if unused { Err(Miss::Rex) } else { Ok(()) })
}

/// The register width an operand kind decodes to.
fn kind_size(kind: OperandKind) -> Size {
    match kind {
        OperandKind::Reg(s) | OperandKind::RegMem(s) | OperandKind::Mem(s) => s,
        OperandKind::Fixed(r) => r.size(),
        OperandKind::One | OperandKind::Imm { .. } | OperandKind::Rel { .. } => Size::Qword,
    }
}

/// An iterator that decodes instructions one after another from a byte
/// slice, yielding each with its offset. It stops after the first error.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::decode_all;
///
/// let code = [0x55, 0x48, 0x89, 0xE5, 0x5D, 0xC3];
/// let text: Vec<String> = decode_all(&code)
///     .map(|r| r.map(|(_, d)| d.inst().to_string()))
///     .collect::<Result<_, _>>()?;
/// assert_eq!(text, ["push rbp", "mov rbp, rsp", "pop rbp", "ret"]);
/// # Ok::<(), isa_lang::x86_64::DecodeError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Instructions<'a> {
    bytes: &'a [u8],
    offset: usize,
    failed: bool,
}

/// Decodes every instruction in `bytes`, in order. See [`Instructions`].
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::decode_all;
///
/// assert_eq!(decode_all(&[0x90, 0x90, 0xC3]).count(), 3);
/// // An error ends the sequence, with the offset of the bad byte.
/// let last = decode_all(&[0x90, 0x0F, 0xFF]).last().unwrap();
/// assert_eq!(last.unwrap_err().at(), 2);
/// ```
#[must_use]
pub fn decode_all(bytes: &[u8]) -> Instructions<'_> {
    Instructions {
        bytes,
        offset: 0,
        failed: false,
    }
}

impl Iterator for Instructions<'_> {
    type Item = Result<(usize, Decoded), DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let rest = self.bytes.get(self.offset..).filter(|r| !r.is_empty())?;
        match decode(rest) {
            Ok(d) => {
                let at = self.offset;
                self.offset += d.len();
                Some(Ok((at, d)))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(rebase(e, self.offset)))
            }
        }
    }
}

impl FusedIterator for Instructions<'_> {}

/// Shifts an error's offset from the start of a sub-slice to the start of
/// the whole input.
fn rebase(e: DecodeError, by: usize) -> DecodeError {
    match e {
        DecodeError::Truncated { at } => DecodeError::Truncated { at: at + by },
        DecodeError::UnsupportedPrefix { at, byte } => {
            DecodeError::UnsupportedPrefix { at: at + by, byte }
        }
        DecodeError::RepeatedPrefix { at } => DecodeError::RepeatedPrefix { at: at + by },
        DecodeError::MisplacedRex { at } => DecodeError::MisplacedRex { at: at + by },
        DecodeError::UnknownOpcode {
            at,
            escaped,
            opcode,
        } => DecodeError::UnknownOpcode {
            at: at + by,
            escaped,
            opcode,
        },
        DecodeError::InvalidOperandSize { at } => DecodeError::InvalidOperandSize { at: at + by },
        DecodeError::UnknownExtension { at, ext } => {
            DecodeError::UnknownExtension { at: at + by, ext }
        }
        DecodeError::RegisterOperand { at } => DecodeError::RegisterOperand { at: at + by },
        DecodeError::UnusedRex { at } => DecodeError::UnusedRex { at: at + by },
    }
}

/// A disassembly listing: one line per instruction with its address, its
/// bytes, and its Intel-syntax text. Bytes that do not decode are listed one
/// at a time as `(bad)` and the listing carries on.
///
/// The output is deterministic: same bytes and address, same text.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::disassemble;
///
/// let code = [0x31, 0xC0, 0x74, 0x00, 0x0F, 0xC3];
/// let text = disassemble(&code, 0x1000).to_string();
/// assert_eq!(
///     text,
///     "0000000000001000  31 c0                    xor eax, eax\n\
///      0000000000001002  74 00                    je 0x1004\n\
///      0000000000001004  0f                       (bad)\n\
///      0000000000001005  c3                       ret\n",
/// );
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Disassembly<'a> {
    bytes: &'a [u8],
    ip: u64,
}

/// Lists `bytes` as instructions starting at address `ip`. See
/// [`Disassembly`].
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::disassemble;
///
/// assert_eq!(disassemble(&[0xC3], 0).to_string(), "0000000000000000  c3                       ret\n");
/// ```
#[must_use]
pub fn disassemble(bytes: &[u8], ip: u64) -> Disassembly<'_> {
    Disassembly { bytes, ip }
}

/// Width of the byte column: room for 8 bytes before the text starts.
const BYTE_COLUMN: usize = 25;

impl fmt::Display for Disassembly<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut offset = 0usize;
        while let Some(rest) = self.bytes.get(offset..).filter(|r| !r.is_empty()) {
            let ip = self.ip.wrapping_add(offset as u64);
            let (len, decoded) = match decode(rest) {
                Ok(d) => (d.len(), Some(d)),
                Err(_) => (1, None),
            };
            write!(f, "{ip:016x}  ")?;
            let mut written = 0;
            for b in rest.iter().take(len) {
                write!(f, "{b:02x} ")?;
                written += 3;
            }
            for _ in written..BYTE_COLUMN {
                f.write_str(" ")?;
            }
            match decoded {
                Some(d) => writeln!(f, "{}", d.inst().display_at(ip))?,
                None => f.write_str("(bad)\n")?,
            }
            offset += len;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> alloc::string::String {
        use alloc::string::ToString;
        decode(bytes)
            .map(|d| d.inst().to_string())
            .unwrap_or_else(|e| e.to_string())
    }

    #[test]
    fn test_decode_prefix_errors() {
        assert_eq!(decode(&[0x66]), Err(DecodeError::Truncated { at: 1 }));
        assert_eq!(
            decode(&[0x66, 0x66, 0x01, 0xC0]),
            Err(DecodeError::RepeatedPrefix { at: 1 })
        );
        assert_eq!(
            decode(&[0x48, 0x66, 0x01, 0xC0]),
            Err(DecodeError::MisplacedRex { at: 0 })
        );
        assert_eq!(
            decode(&[0x48, 0x41, 0x01, 0xC0]),
            Err(DecodeError::MisplacedRex { at: 0 })
        );
        assert_eq!(
            decode(&[0x67, 0x8B, 0x00]),
            Err(DecodeError::UnsupportedPrefix { at: 0, byte: 0x67 })
        );
        assert_eq!(decode(&[0x48]), Err(DecodeError::Truncated { at: 1 }));
        assert_eq!(decode(&[0x0F]), Err(DecodeError::Truncated { at: 1 }));
    }

    #[test]
    fn test_decode_form_errors() {
        // F7 /1 is not defined.
        assert_eq!(
            decode(&[0xF7, 0xC8]),
            Err(DecodeError::UnknownExtension { at: 1, ext: 1 })
        );
        // lea eax, eax
        assert_eq!(
            decode(&[0x8D, 0xC0]),
            Err(DecodeError::RegisterOperand { at: 1 })
        );
        // REX.B on ret
        assert_eq!(decode(&[0x41, 0xC3]), Err(DecodeError::UnusedRex { at: 0 }));
        // REX.X without a SIB byte
        assert_eq!(
            decode(&[0x42, 0x01, 0xC0]),
            Err(DecodeError::UnusedRex { at: 0 })
        );
        // 66 on movsxd has no form
        assert_eq!(
            decode(&[0x66, 0x63, 0xC0]),
            Err(DecodeError::InvalidOperandSize { at: 1 })
        );
        // Truncated SIB, displacement, immediate
        assert_eq!(decode(&[0x8B, 0x04]), Err(DecodeError::Truncated { at: 2 }));
        assert_eq!(
            decode(&[0x8B, 0x80, 1, 2]),
            Err(DecodeError::Truncated { at: 4 })
        );
        assert_eq!(decode(&[0x05, 1]), Err(DecodeError::Truncated { at: 2 }));
    }

    #[test]
    fn test_decode_nop_and_xchg_share_90() {
        assert_eq!(text(&[0x90]), "nop");
        assert_eq!(text(&[0x66, 0x90]), "nop");
        assert_eq!(text(&[0x48, 0x90]), "nop");
        assert_eq!(text(&[0x40, 0x90]), "nop");
        assert_eq!(text(&[0x41, 0x90]), "xchg r8d, eax");
        assert_eq!(text(&[0x49, 0x90]), "xchg r8, rax");
        assert_eq!(text(&[0x93]), "xchg ebx, eax");
        assert_eq!(text(&[0x66, 0x93]), "xchg bx, ax");
    }

    #[test]
    fn test_decode_byte_registers_depend_on_rex() {
        assert_eq!(text(&[0x88, 0xE0]), "mov al, ah");
        assert_eq!(text(&[0x40, 0x88, 0xE0]), "mov al, spl");
        assert_eq!(text(&[0x0F, 0xB6, 0xC4]), "movzx eax, ah");
        assert_eq!(text(&[0x48, 0x0F, 0xB6, 0xC4]), "movzx rax, spl");
    }

    #[test]
    fn test_decode_d64_accepts_redundant_rex_w() {
        assert_eq!(text(&[0x48, 0xFF, 0xE0]), "jmp rax");
        assert_eq!(text(&[0x48, 0x50]), "push rax");
        assert_eq!(text(&[0x48, 0xFF, 0x25, 0, 0, 0, 0]), "jmp qword ptr [rip]");
        assert_eq!(
            text(&[0x66, 0x50]),
            "opcode at byte 1 has no form with this operand size"
        );
    }

    #[test]
    fn test_decode_rel_is_from_instruction_start() {
        assert_eq!(text(&[0xEB, 0xFE]), "jmp $+0x0");
        assert_eq!(text(&[0xE9, 0, 0, 0, 0]), "jmp $+0x5");
        assert_eq!(text(&[0x0F, 0x84, 0xFA, 0xFF, 0xFF, 0xFF]), "je $+0x0");
    }

    #[test]
    fn test_rebase_shifts_every_variant() {
        let all = [
            DecodeError::Truncated { at: 1 },
            DecodeError::UnsupportedPrefix { at: 1, byte: 0xF0 },
            DecodeError::RepeatedPrefix { at: 1 },
            DecodeError::MisplacedRex { at: 1 },
            DecodeError::UnknownOpcode {
                at: 1,
                escaped: false,
                opcode: 6,
            },
            DecodeError::InvalidOperandSize { at: 1 },
            DecodeError::UnknownExtension { at: 1, ext: 1 },
            DecodeError::RegisterOperand { at: 1 },
            DecodeError::UnusedRex { at: 1 },
        ];
        for e in all {
            assert_eq!(rebase(e, 10).at(), 11);
        }
    }
}
