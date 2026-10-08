//! Intel-syntax text: the disassembler's output format.
//!
//! The format is fixed so that equal instructions always print the same:
//! lowercase mnemonics and registers, `size ptr [base+index*scale+disp]`
//! memory operands (no size for `lea`), and every number in hexadecimal with
//! an explicit sign when negative. Branch targets print as `$+0x10` (relative
//! to the start of the instruction, as NASM writes it) or, through
//! [`Inst::display_at`], as an absolute address.

use core::fmt;

use super::inst::{Inst, Mnemonic};
use super::operand::{Mem, Operand};

/// Writes `value` as `0x..` or `-0x..`.
pub(crate) fn hex(f: &mut fmt::Formatter<'_>, value: i64) -> fmt::Result {
    if value < 0 {
        write!(f, "-0x{:x}", value.unsigned_abs())
    } else {
        write!(f, "0x{value:x}")
    }
}

/// Writes `value` as `+0x..` or `-0x..`.
pub(crate) fn signed_hex(f: &mut fmt::Formatter<'_>, value: i64) -> fmt::Result {
    if value >= 0 {
        f.write_str("+")?;
    }
    hex(f, value)
}

/// Writes the bracketed address of a memory operand.
fn address(f: &mut fmt::Formatter<'_>, m: &Mem) -> fmt::Result {
    f.write_str("[")?;
    let disp = i64::from(m.disp());
    let mut any = false;
    if m.is_rip_relative() {
        f.write_str("rip")?;
        any = true;
    }
    if let Some(base) = m.base() {
        f.write_str(base.name())?;
        any = true;
    }
    if let Some(index) = m.index() {
        if any {
            f.write_str("+")?;
        }
        write!(f, "{}*{}", index.name(), m.scale().factor())?;
        any = true;
    }
    if !any {
        hex(f, disp)?;
    } else if disp != 0 {
        signed_hex(f, disp)?;
    }
    f.write_str("]")
}

fn operand(f: &mut fmt::Formatter<'_>, op: &Operand, sized: bool, ip: Option<u64>) -> fmt::Result {
    match op {
        Operand::Reg(r) => f.write_str(r.name()),
        Operand::Mem(m) => {
            if sized {
                write!(f, "{} ptr ", m.size().keyword())?;
            }
            address(f, m)
        }
        Operand::Imm(v) => hex(f, *v),
        Operand::Rel(d) => match ip {
            Some(ip) => write!(f, "0x{:x}", ip.wrapping_add(*d as u64)),
            None => {
                f.write_str("$")?;
                signed_hex(f, *d)
            }
        },
    }
}

fn inst(f: &mut fmt::Formatter<'_>, i: &Inst, ip: Option<u64>) -> fmt::Result {
    write!(f, "{}", i.mnemonic())?;
    // `lea` computes an address; the access size means nothing there.
    let sized = i.mnemonic() != Mnemonic::Lea;
    for (n, op) in i.operands().iter().enumerate() {
        f.write_str(if n == 0 { " " } else { ", " })?;
        operand(f, op, sized, ip)?;
    }
    Ok(())
}

impl fmt::Display for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        padded(f, self, None)
    }
}

/// Writes an instruction, honouring a width (`{:<40}`) by rendering first.
/// The unpadded path writes straight to the formatter, allocation-free.
fn padded(f: &mut fmt::Formatter<'_>, i: &Inst, ip: Option<u64>) -> fmt::Result {
    if f.width().is_none() {
        return inst(f, i, ip);
    }
    f.pad(&alloc::format!("{}", Unpadded(i, ip)))
}

/// An instruction's text with no width applied.
struct Unpadded<'a>(&'a Inst, Option<u64>);

impl fmt::Display for Unpadded<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        inst(f, self.0, self.1)
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        operand(f, self, true, None)
    }
}

impl fmt::Display for Mem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ptr ", self.size().keyword())?;
        address(f, self)
    }
}

impl fmt::Debug for Mem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mem({self})")
    }
}

/// An [`Inst`] shown with branch targets as absolute addresses, for an
/// instruction located at a known address. Built by [`Inst::display_at`].
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Inst, Mnemonic, Operand};
///
/// let call = Inst::new(Mnemonic::Call, [Operand::Rel(0x40)]);
/// assert_eq!(call.display_at(0x40_1000).to_string(), "call 0x401040");
/// assert_eq!(call.to_string(), "call $+0x40");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct DisplayAt<'a> {
    inst: &'a Inst,
    ip: u64,
}

impl Inst {
    /// Displays the instruction as located at address `ip`, so `Rel`
    /// targets print as absolute addresses (wrapping at 2^64).
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Cond, Inst, Mnemonic, Operand};
    ///
    /// let back = Inst::new(Mnemonic::Jcc(Cond::Ne), [Operand::Rel(-16)]);
    /// assert_eq!(back.display_at(0x1010).to_string(), "jne 0x1000");
    /// ```
    #[must_use]
    pub fn display_at(&self, ip: u64) -> DisplayAt<'_> {
        DisplayAt { inst: self, ip }
    }
}

impl fmt::Display for DisplayAt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        padded(f, self.inst, Some(self.ip))
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;
    use crate::x86_64::{Reg, Scale, Size};

    #[test]
    fn test_hex_extremes() {
        assert_eq!(Operand::Imm(i64::MIN).to_string(), "-0x8000000000000000");
        assert_eq!(Operand::Imm(i64::MAX).to_string(), "0x7fffffffffffffff");
        assert_eq!(Operand::Imm(0).to_string(), "0x0");
        assert_eq!(Operand::Rel(0).to_string(), "$+0x0");
        assert_eq!(Operand::Rel(-1).to_string(), "$-0x1");
    }

    #[test]
    fn test_memory_text() {
        let q = Size::Qword;
        assert_eq!(Mem::new(q, Reg::RAX).to_string(), "qword ptr [rax]");
        assert_eq!(Mem::rip(q, 0).to_string(), "qword ptr [rip]");
        assert_eq!(Mem::absolute(q, -16).to_string(), "qword ptr [-0x10]");
        assert_eq!(Mem::absolute(q, 0).to_string(), "qword ptr [0x0]");
        let idx = Mem::absolute(q, 0).with_index(Reg::R12, Scale::X1);
        assert_eq!(idx.to_string(), "qword ptr [r12*1]");
        let full = Mem::new(Size::Byte, Reg::R13)
            .with_index(Reg::RBP, Scale::X2)
            .with_disp(i32::MIN);
        assert_eq!(full.to_string(), "byte ptr [r13+rbp*2-0x80000000]");
    }

    #[test]
    fn test_lea_has_no_size() {
        let lea = Inst::new(
            Mnemonic::Lea,
            [
                Reg::RAX.into(),
                Mem::new(Size::Qword, Reg::RBX).with_disp(8).into(),
            ],
        );
        assert_eq!(lea.to_string(), "lea rax, [rbx+0x8]");
    }

    #[test]
    fn test_padding_is_honoured() {
        let i = Inst::new(Mnemonic::Ret, []);
        assert_eq!(alloc::format!("[{i:>5}]"), "[  ret]");
        assert_eq!(alloc::format!("[{:<6}]", i.display_at(0)), "[ret   ]");
    }

    #[test]
    fn test_display_at_wraps() {
        let j = Inst::new(Mnemonic::Jmp, [Operand::Rel(-2)]);
        assert_eq!(j.display_at(0).to_string(), "jmp 0xfffffffffffffffe");
    }
}
