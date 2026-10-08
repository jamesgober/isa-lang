//! Instructions: a mnemonic and up to three operands.

use core::fmt;

use super::operand::{Cond, Operand};

/// An instruction's operation.
///
/// One variant per mnemonic family. The condition-code families carry their
/// [`Cond`]: `Mnemonic::Jcc(Cond::E)` is `je`. Operand count and kinds pick
/// the form, so `imul` with one, two, or three operands is the same
/// mnemonic.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Cond, Mnemonic};
///
/// assert_eq!(Mnemonic::Add.to_string(), "add");
/// assert_eq!(Mnemonic::Jcc(Cond::Ne).to_string(), "jne");
/// assert_eq!(Mnemonic::Cmovcc(Cond::G).to_string(), "cmovg");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mnemonic {
    /// Add with carry.
    Adc,
    /// Add.
    Add,
    /// Bitwise and.
    And,
    /// Bit test: copy one bit into `CF`.
    Bt,
    /// Call a procedure.
    Call,
    /// Sign-extend `AL` into `AX`.
    Cbw,
    /// Sign-extend `EAX` into `EDX:EAX`.
    Cdq,
    /// Sign-extend `EAX` into `RAX`.
    Cdqe,
    /// Conditional move.
    Cmovcc(Cond),
    /// Compare (subtract, setting flags only).
    Cmp,
    /// Sign-extend `RAX` into `RDX:RAX`.
    Cqo,
    /// Sign-extend `AX` into `DX:AX`.
    Cwd,
    /// Sign-extend `AX` into `EAX`.
    Cwde,
    /// Decrement by one.
    Dec,
    /// Unsigned divide.
    Div,
    /// Signed divide.
    Idiv,
    /// Signed multiply (one, two, or three operands).
    Imul,
    /// Increment by one.
    Inc,
    /// Breakpoint trap.
    Int3,
    /// Conditional jump.
    Jcc(Cond),
    /// Unconditional jump.
    Jmp,
    /// Load effective address.
    Lea,
    /// Move.
    Mov,
    /// Move with sign extension.
    Movsx,
    /// Move doubleword to quadword with sign extension.
    Movsxd,
    /// Move with zero extension.
    Movzx,
    /// Unsigned multiply.
    Mul,
    /// Two's-complement negate.
    Neg,
    /// No operation (one byte, or the multi-byte `nop r/m` form).
    Nop,
    /// Bitwise complement.
    Not,
    /// Bitwise or.
    Or,
    /// Pop from the stack.
    Pop,
    /// Push onto the stack.
    Push,
    /// Return from a procedure.
    Ret,
    /// Rotate left.
    Rol,
    /// Rotate right.
    Ror,
    /// Arithmetic shift right.
    Sar,
    /// Subtract with borrow.
    Sbb,
    /// Set a byte to 0 or 1 from a condition.
    Setcc(Cond),
    /// Shift left.
    Shl,
    /// Logical shift right.
    Shr,
    /// Subtract.
    Sub,
    /// Bitwise and, setting flags only.
    Test,
    /// Guaranteed invalid opcode.
    Ud2,
    /// Exchange.
    Xchg,
    /// Bitwise exclusive or.
    Xor,
}

/// The mnemonic family without a condition: the key the form table uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Adc,
    Add,
    And,
    Bt,
    Call,
    Cbw,
    Cdq,
    Cdqe,
    Cmovcc,
    Cmp,
    Cqo,
    Cwd,
    Cwde,
    Dec,
    Div,
    Idiv,
    Imul,
    Inc,
    Int3,
    Jcc,
    Jmp,
    Lea,
    Mov,
    Movsx,
    Movsxd,
    Movzx,
    Mul,
    Neg,
    Nop,
    Not,
    Or,
    Pop,
    Push,
    Ret,
    Rol,
    Ror,
    Sar,
    Sbb,
    Setcc,
    Shl,
    Shr,
    Sub,
    Test,
    Ud2,
    Xchg,
    Xor,
}

/// Number of [`Op`] values.
pub(crate) const OP_COUNT: usize = 46;

/// Every `Op`, in discriminant order.
pub(crate) const OPS: [Op; OP_COUNT] = [
    Op::Adc,
    Op::Add,
    Op::And,
    Op::Bt,
    Op::Call,
    Op::Cbw,
    Op::Cdq,
    Op::Cdqe,
    Op::Cmovcc,
    Op::Cmp,
    Op::Cqo,
    Op::Cwd,
    Op::Cwde,
    Op::Dec,
    Op::Div,
    Op::Idiv,
    Op::Imul,
    Op::Inc,
    Op::Int3,
    Op::Jcc,
    Op::Jmp,
    Op::Lea,
    Op::Mov,
    Op::Movsx,
    Op::Movsxd,
    Op::Movzx,
    Op::Mul,
    Op::Neg,
    Op::Nop,
    Op::Not,
    Op::Or,
    Op::Pop,
    Op::Push,
    Op::Ret,
    Op::Rol,
    Op::Ror,
    Op::Sar,
    Op::Sbb,
    Op::Setcc,
    Op::Shl,
    Op::Shr,
    Op::Sub,
    Op::Test,
    Op::Ud2,
    Op::Xchg,
    Op::Xor,
];

/// Names as the form table spells them (SDM style), indexed by `Op`.
pub(crate) const OP_TABLE_NAMES: [&str; OP_COUNT] = [
    "ADC", "ADD", "AND", "BT", "CALL", "CBW", "CDQ", "CDQE", "CMOVcc", "CMP", "CQO", "CWD", "CWDE",
    "DEC", "DIV", "IDIV", "IMUL", "INC", "INT3", "Jcc", "JMP", "LEA", "MOV", "MOVSX", "MOVSXD",
    "MOVZX", "MUL", "NEG", "NOP", "NOT", "OR", "POP", "PUSH", "RET", "ROL", "ROR", "SAR", "SBB",
    "SETcc", "SHL", "SHR", "SUB", "TEST", "UD2", "XCHG", "XOR",
];

/// Disassembly names, indexed by `Op`. Condition families hold the prefix.
const OP_NAMES: [&str; OP_COUNT] = [
    "adc", "add", "and", "bt", "call", "cbw", "cdq", "cdqe", "cmov", "cmp", "cqo", "cwd", "cwde",
    "dec", "div", "idiv", "imul", "inc", "int3", "j", "jmp", "lea", "mov", "movsx", "movsxd",
    "movzx", "mul", "neg", "nop", "not", "or", "pop", "push", "ret", "rol", "ror", "sar", "sbb",
    "set", "shl", "shr", "sub", "test", "ud2", "xchg", "xor",
];

impl Op {
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) const fn has_cond(self) -> bool {
        matches!(self, Op::Jcc | Op::Setcc | Op::Cmovcc)
    }
}

impl Mnemonic {
    /// The family and, for `Jcc`/`SETcc`/`CMOVcc`, the condition.
    pub(crate) const fn parts(self) -> (Op, Option<Cond>) {
        match self {
            Mnemonic::Adc => (Op::Adc, None),
            Mnemonic::Add => (Op::Add, None),
            Mnemonic::And => (Op::And, None),
            Mnemonic::Bt => (Op::Bt, None),
            Mnemonic::Call => (Op::Call, None),
            Mnemonic::Cbw => (Op::Cbw, None),
            Mnemonic::Cdq => (Op::Cdq, None),
            Mnemonic::Cdqe => (Op::Cdqe, None),
            Mnemonic::Cmovcc(c) => (Op::Cmovcc, Some(c)),
            Mnemonic::Cmp => (Op::Cmp, None),
            Mnemonic::Cqo => (Op::Cqo, None),
            Mnemonic::Cwd => (Op::Cwd, None),
            Mnemonic::Cwde => (Op::Cwde, None),
            Mnemonic::Dec => (Op::Dec, None),
            Mnemonic::Div => (Op::Div, None),
            Mnemonic::Idiv => (Op::Idiv, None),
            Mnemonic::Imul => (Op::Imul, None),
            Mnemonic::Inc => (Op::Inc, None),
            Mnemonic::Int3 => (Op::Int3, None),
            Mnemonic::Jcc(c) => (Op::Jcc, Some(c)),
            Mnemonic::Jmp => (Op::Jmp, None),
            Mnemonic::Lea => (Op::Lea, None),
            Mnemonic::Mov => (Op::Mov, None),
            Mnemonic::Movsx => (Op::Movsx, None),
            Mnemonic::Movsxd => (Op::Movsxd, None),
            Mnemonic::Movzx => (Op::Movzx, None),
            Mnemonic::Mul => (Op::Mul, None),
            Mnemonic::Neg => (Op::Neg, None),
            Mnemonic::Nop => (Op::Nop, None),
            Mnemonic::Not => (Op::Not, None),
            Mnemonic::Or => (Op::Or, None),
            Mnemonic::Pop => (Op::Pop, None),
            Mnemonic::Push => (Op::Push, None),
            Mnemonic::Ret => (Op::Ret, None),
            Mnemonic::Rol => (Op::Rol, None),
            Mnemonic::Ror => (Op::Ror, None),
            Mnemonic::Sar => (Op::Sar, None),
            Mnemonic::Sbb => (Op::Sbb, None),
            Mnemonic::Setcc(c) => (Op::Setcc, Some(c)),
            Mnemonic::Shl => (Op::Shl, None),
            Mnemonic::Shr => (Op::Shr, None),
            Mnemonic::Sub => (Op::Sub, None),
            Mnemonic::Test => (Op::Test, None),
            Mnemonic::Ud2 => (Op::Ud2, None),
            Mnemonic::Xchg => (Op::Xchg, None),
            Mnemonic::Xor => (Op::Xor, None),
        }
    }

    /// The mnemonic for a family and condition. `cond` is used only by the
    /// condition families, which the decoder always supplies one for.
    pub(crate) const fn from_parts(op: Op, cond: Cond) -> Mnemonic {
        match op {
            Op::Adc => Mnemonic::Adc,
            Op::Add => Mnemonic::Add,
            Op::And => Mnemonic::And,
            Op::Bt => Mnemonic::Bt,
            Op::Call => Mnemonic::Call,
            Op::Cbw => Mnemonic::Cbw,
            Op::Cdq => Mnemonic::Cdq,
            Op::Cdqe => Mnemonic::Cdqe,
            Op::Cmovcc => Mnemonic::Cmovcc(cond),
            Op::Cmp => Mnemonic::Cmp,
            Op::Cqo => Mnemonic::Cqo,
            Op::Cwd => Mnemonic::Cwd,
            Op::Cwde => Mnemonic::Cwde,
            Op::Dec => Mnemonic::Dec,
            Op::Div => Mnemonic::Div,
            Op::Idiv => Mnemonic::Idiv,
            Op::Imul => Mnemonic::Imul,
            Op::Inc => Mnemonic::Inc,
            Op::Int3 => Mnemonic::Int3,
            Op::Jcc => Mnemonic::Jcc(cond),
            Op::Jmp => Mnemonic::Jmp,
            Op::Lea => Mnemonic::Lea,
            Op::Mov => Mnemonic::Mov,
            Op::Movsx => Mnemonic::Movsx,
            Op::Movsxd => Mnemonic::Movsxd,
            Op::Movzx => Mnemonic::Movzx,
            Op::Mul => Mnemonic::Mul,
            Op::Neg => Mnemonic::Neg,
            Op::Nop => Mnemonic::Nop,
            Op::Not => Mnemonic::Not,
            Op::Or => Mnemonic::Or,
            Op::Pop => Mnemonic::Pop,
            Op::Push => Mnemonic::Push,
            Op::Ret => Mnemonic::Ret,
            Op::Rol => Mnemonic::Rol,
            Op::Ror => Mnemonic::Ror,
            Op::Sar => Mnemonic::Sar,
            Op::Sbb => Mnemonic::Sbb,
            Op::Setcc => Mnemonic::Setcc(cond),
            Op::Shl => Mnemonic::Shl,
            Op::Shr => Mnemonic::Shr,
            Op::Sub => Mnemonic::Sub,
            Op::Test => Mnemonic::Test,
            Op::Ud2 => Mnemonic::Ud2,
            Op::Xchg => Mnemonic::Xchg,
            Op::Xor => Mnemonic::Xor,
        }
    }
}

impl fmt::Display for Mnemonic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (op, cond) = self.parts();
        f.write_str(OP_NAMES[op.index()])?;
        match cond {
            Some(c) => f.write_str(c.suffix()),
            None => Ok(()),
        }
    }
}

/// The most operands any instruction form takes (`imul r, r/m, imm`).
pub const MAX_OPERANDS: usize = 3;

// Unused operand slots hold this value, so derived equality and hashing
// compare only the operands that exist.
const FILLER: Operand = Operand::Imm(0);

/// One machine instruction: a [`Mnemonic`] and up to three [`Operand`]s, in
/// Intel order (destination first).
///
/// An `Inst` describes *what* to execute, not *which bytes* to use: the
/// encoder chooses the shortest form that fits, and the decoder turns any of
/// the equivalent encodings back into the same `Inst`. So
/// `decode(encode(i)) == i` for every instruction the encoder accepts.
///
/// `Display` prints Intel syntax; [`display_at`](Inst::display_at) prints
/// branch targets as absolute addresses.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{Inst, Mem, Mnemonic, Operand, Reg, Size};
///
/// let load = Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Mem::new(Size::Qword, Reg::RSP).with_disp(8).into()]);
/// assert_eq!(load.to_string(), "mov rax, qword ptr [rsp+0x8]");
/// assert_eq!(load.operands().len(), 2);
///
/// let ret = Inst::new(Mnemonic::Ret, []);
/// assert_eq!(ret.to_string(), "ret");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inst {
    operands: [Operand; MAX_OPERANDS],
    mnemonic: Mnemonic,
    count: u8,
}

impl Inst {
    /// An instruction with the given operands. More than three operands is a
    /// compile-time error.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Inst, Mnemonic, Operand, Reg};
    ///
    /// let i = Inst::new(Mnemonic::Imul, [Reg::ECX.into(), Reg::EDX.into(), Operand::Imm(10)]);
    /// assert_eq!(i.to_string(), "imul ecx, edx, 0xa");
    /// ```
    #[must_use]
    pub const fn new<const N: usize>(mnemonic: Mnemonic, operands: [Operand; N]) -> Inst {
        const {
            assert!(
                N <= MAX_OPERANDS,
                "an x86-64 instruction has at most three operands"
            )
        }
        let mut all = [FILLER; MAX_OPERANDS];
        let mut i = 0;
        while i < N {
            all[i] = operands[i];
            i += 1;
        }
        Inst {
            operands: all,
            mnemonic,
            count: N as u8,
        }
    }

    /// An instruction from a slice of operands, or `None` if there are more
    /// than three. For callers whose operand count is only known at run time.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Inst, Mnemonic, Operand, Reg};
    ///
    /// let ops = vec![Operand::from(Reg::RDI)];
    /// let push = Inst::from_slice(Mnemonic::Push, &ops).unwrap();
    /// assert_eq!(push.to_string(), "push rdi");
    /// assert!(Inst::from_slice(Mnemonic::Nop, &[Operand::Imm(0); 4]).is_none());
    /// ```
    #[must_use]
    pub fn from_slice(mnemonic: Mnemonic, operands: &[Operand]) -> Option<Inst> {
        if operands.len() > MAX_OPERANDS {
            return None;
        }
        let mut all = [FILLER; MAX_OPERANDS];
        all[..operands.len()].copy_from_slice(operands);
        Some(Inst {
            operands: all,
            mnemonic,
            count: operands.len() as u8,
        })
    }

    /// The instruction's mnemonic.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Inst, Mnemonic};
    ///
    /// assert_eq!(Inst::new(Mnemonic::Cqo, []).mnemonic(), Mnemonic::Cqo);
    /// ```
    #[inline]
    #[must_use]
    pub const fn mnemonic(&self) -> Mnemonic {
        self.mnemonic
    }

    /// The operands, destination first.
    ///
    /// # Examples
    ///
    /// ```
    /// use isa_lang::x86_64::{Inst, Mnemonic, Operand, Reg};
    ///
    /// let i = Inst::new(Mnemonic::Xor, [Reg::EAX.into(), Reg::EAX.into()]);
    /// assert_eq!(i.operands(), [Operand::Reg(Reg::EAX), Operand::Reg(Reg::EAX)]);
    /// ```
    #[inline]
    #[must_use]
    pub fn operands(&self) -> &[Operand] {
        // `count` is at most MAX_OPERANDS by construction.
        &self.operands[..usize::from(self.count).min(MAX_OPERANDS)]
    }
}

impl fmt::Debug for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Inst({self})")
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::*;
    use crate::x86_64::reg::Reg;

    #[test]
    fn test_mnemonic_parts_round_trip() {
        for (i, op) in OPS.iter().enumerate() {
            assert_eq!(op.index(), i);
            let m = Mnemonic::from_parts(*op, Cond::Ge);
            let (back, cond) = m.parts();
            assert_eq!(back, *op);
            assert_eq!(cond.is_some(), op.has_cond());
        }
    }

    #[test]
    fn test_inst_equality_ignores_unused_slots() {
        let a = Inst::new(Mnemonic::Push, [Reg::RAX.into()]);
        let b = Inst::from_slice(Mnemonic::Push, &[Reg::RAX.into()]);
        assert_eq!(Some(a), b);
        assert_ne!(a, Inst::new(Mnemonic::Push, [Reg::RCX.into()]));
        assert_ne!(
            Inst::new(Mnemonic::Ret, []),
            Inst::new(Mnemonic::Ret, [Operand::Imm(0)])
        );
    }

    #[test]
    fn test_inst_debug_uses_intel_syntax() {
        let i = Inst::new(Mnemonic::Neg, [Reg::R11.into()]);
        assert_eq!(format!("{i:?}"), "Inst(neg r11)");
    }

    #[test]
    fn test_from_slice_limits() {
        assert!(Inst::from_slice(Mnemonic::Imul, &[Operand::Imm(0); 3]).is_some());
        assert!(Inst::from_slice(Mnemonic::Imul, &[Operand::Imm(0); 4]).is_none());
        assert_eq!(
            Inst::from_slice(Mnemonic::Ud2, &[]).map(|i| i.operands().len()),
            Some(0)
        );
    }
}
