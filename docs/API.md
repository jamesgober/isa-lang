# isa-lang &mdash; API Reference

> Complete reference for every public item in `isa-lang`, with examples.
> **Status: 0.2.0, pre-1.0.** The surface is designed across the 0.x series and
> frozen at `1.0.0`, after the family's native backend emits through it. See
> [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [Instructions, not bytes](#instructions-not-bytes)
  - [Immediates](#immediates)
  - [Branch targets](#branch-targets)
  - [Memory operands](#memory-operands)
  - [Byte registers and REX](#byte-registers-and-rex)
  - [Form selection](#form-selection)
  - [Decoder acceptance](#decoder-acceptance)
  - [Text format](#text-format)
- Functions: [`encode`](#encode) · [`encode_form`](#encode_form) · [`decode`](#decode) ·
  [`decode_all`](#decode_all) · [`disassemble`](#disassemble) · [`forms`](#forms) ·
  [`recommended_nop`](#recommended_nop)
- Instructions: [`Inst`](#inst) · [`Mnemonic`](#mnemonic) · [`Operand`](#operand) ·
  [`Reg`](#reg) · [`Size`](#size) · [`Mem`](#mem) · [`Scale`](#scale) · [`Cond`](#cond) ·
  [`DisplayAt`](#displayat)
- Results: [`Encoded`](#encoded) · [`Decoded`](#decoded) · [`Instructions`](#instructions) ·
  [`Disassembly`](#disassembly)
- The description: [`Form`](#form) · [`OperandKind`](#operandkind) · [`Flags`](#flags) ·
  [`FlagEffects`](#flageffects)
- Assembler: [`Assembler`](#assembler) · [`Label`](#label) · [`Code`](#code)
- Errors: [`EncodeError`](#encodeerror) · [`DecodeError`](#decodeerror) · [`AsmError`](#asmerror)
- [Constants](#constants)
- [Feature flags](#feature-flags)
- [Limits](#limits)
- [Guide: emitting a function](#guide-emitting-a-function)
- [Guide: instruction selection through forms](#guide-instruction-selection-through-forms)

## Overview

`isa-lang` encodes and decodes machine instructions from a target description:
every instruction form is a row of data, and one encoder and one decoder
interpret all rows. Everything for x86-64 lives in the `isa_lang::x86_64`
module.

| Item | Kind | Purpose |
|---|---|---|
| [`encode`](#encode) | fn | Encode an `Inst` with the shortest form that fits. |
| [`encode_form`](#encode_form) | fn | Encode an `Inst` with one given form. |
| [`decode`](#decode) | fn | Decode the instruction at the start of a byte slice. |
| [`decode_all`](#decode_all) | fn | Decode a whole buffer, instruction by instruction. |
| [`disassemble`](#disassemble) | fn | An Intel-syntax listing of a buffer. |
| [`forms`](#forms) | fn | Every form of the target description. |
| [`recommended_nop`](#recommended_nop) | fn | The SDM's `NOP` sequence of a given length. |
| [`Inst`](#inst) | struct | A mnemonic and up to three operands. |
| [`Mnemonic`](#mnemonic) | enum | The operation (`Add`, `Jcc(Cond::E)`, ...). |
| [`Operand`](#operand) | enum | A register, memory, immediate, or branch target. |
| [`Reg`](#reg) / [`Size`](#size) | struct / enum | General-purpose registers and widths. |
| [`Mem`](#mem) / [`Scale`](#scale) | struct / enum | Memory operands. |
| [`Cond`](#cond) | enum | Condition codes. |
| [`Encoded`](#encoded) / [`Decoded`](#decoded) | struct | Results, with the form used and flag effects. |
| [`Form`](#form) / [`OperandKind`](#operandkind) | struct / enum | One row of the description, and its operand kinds. |
| [`Flags`](#flags) / [`FlagEffects`](#flageffects) | struct | Status flags and what a form does to them. |
| [`Assembler`](#assembler) / [`Label`](#label) / [`Code`](#code) | struct | Labels, fixups, branch relaxation. |
| [`EncodeError`](#encodeerror) / [`DecodeError`](#decodeerror) / [`AsmError`](#asmerror) | enum | Why something failed. |

## Installation

```toml
[dependencies]
isa-lang = "0.2"
```

For `no_std` targets (the crate needs only `alloc`):

```toml
[dependencies]
isa-lang = { version = "0.2", default-features = false }
```

## Quick start

```rust
use isa_lang::x86_64::{decode, encode, Inst, Mnemonic, Operand, Reg};

let inst = Inst::new(Mnemonic::Sub, [Reg::RSP.into(), Operand::Imm(40)]);
let bytes = encode(&inst)?;
assert_eq!(&*bytes, [0x48, 0x83, 0xEC, 0x28]);
assert_eq!(decode(&bytes).unwrap().inst(), inst);
assert_eq!(inst.to_string(), "sub rsp, 0x28");
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

## Concepts

### Instructions, not bytes

An [`Inst`](#inst) describes an operation, not an encoding. x86-64 often has
several encodings of the same operation (`add eax, ebx` is `01 D8` or `03 C3`;
`add rax, 8` fits both an 8-bit and a 32-bit immediate form). The encoder
picks one; the decoder turns every one of them into the same `Inst`. Hence
the central guarantee: `decode(encode(i)) == i`.

```rust
use isa_lang::x86_64::decode;

let a = decode(&[0x01, 0xD8]).unwrap().inst();
let b = decode(&[0x03, 0xC3]).unwrap().inst();
assert_eq!(a, b);
assert_eq!(a.to_string(), "add eax, ebx");
```

### Immediates

`Operand::Imm(v)` is **the operand's value as a signed integer of the
operation's width**. For a 32-bit operation, all ones is `-1`, not
`0xFFFF_FFFF`; for an 8-bit operation the range is `-128..=127`. A field the
hardware sign-extends (`imm8` in a 64-bit `add`) accepts exactly the values it
can represent. Unsigned fields &mdash; shift and rotate counts, `bt` bit
numbers, `ret`'s pop count &mdash; take `0..=255` or `0..=65535`. Anything out
of range is [`ImmediateOutOfRange`](#encodeerror); nothing is truncated.

```rust
use isa_lang::x86_64::{encode, EncodeError, Inst, Mnemonic, Operand, Reg};

let ok = Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Operand::Imm(-1)]);
assert_eq!(&*encode(&ok)?, [0xB8, 0xFF, 0xFF, 0xFF, 0xFF]);

let not_canonical = Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Operand::Imm(0xFFFF_FFFF)]);
assert_eq!(encode(&not_canonical), Err(EncodeError::ImmediateOutOfRange { value: 0xFFFF_FFFF }));
# Ok::<(), EncodeError>(())
```

### Branch targets

`Operand::Rel(d)` is a target `d` bytes from the **start** of the branch
(NASM's `$+d`). The encoder chooses `rel8` or `rel32` and subtracts the
instruction's own length, so the same `Rel` means the same target whichever
form is used. For code whose layout is not known yet, use the
[`Assembler`](#assembler) and labels.

```rust
use isa_lang::x86_64::{encode, Inst, Mnemonic, Operand};

// 2-byte jmp: field = 16 - 2.
assert_eq!(&*encode(&Inst::new(Mnemonic::Jmp, [Operand::Rel(16)]))?, [0xEB, 0x0E]);
// Too far for rel8: 5-byte jmp, field = 1000 - 5.
assert_eq!(&*encode(&Inst::new(Mnemonic::Jmp, [Operand::Rel(1000)]))?, [0xE9, 0xE3, 0x03, 0x00, 0x00]);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

### Memory operands

A [`Mem`](#mem) is an access size and an address: `[base + index*scale + disp]`,
`[rip + disp]` (relative to the end of the instruction), or absolute
`[disp]` / `[index*scale + disp]`. Base and index are 64-bit registers; `rsp`
cannot be an index. The encoder picks the shortest ModRM/SIB/displacement
layout, handling the special cases (`rsp`/`r12` bases need a SIB byte;
`rbp`/`r13` bases need a displacement; an absolute address needs a SIB byte
because `mod=00 rm=101` means RIP-relative).

```rust
use isa_lang::x86_64::{encode, Inst, Mem, Mnemonic, Reg, Size};

let load = |m: Mem| encode(&Inst::new(Mnemonic::Mov, [Reg::EAX.into(), m.into()])).map(|e| e.to_vec());
assert_eq!(load(Mem::new(Size::Dword, Reg::RAX))?, [0x8B, 0x00]);
assert_eq!(load(Mem::new(Size::Dword, Reg::RSP))?, [0x8B, 0x04, 0x24]);
assert_eq!(load(Mem::new(Size::Dword, Reg::RBP))?, [0x8B, 0x45, 0x00]);
assert_eq!(load(Mem::absolute(Size::Dword, 0x10))?, [0x8B, 0x04, 0x25, 0x10, 0, 0, 0]);
assert_eq!(load(Mem::rip(Size::Dword, 0x10))?, [0x8B, 0x05, 0x10, 0, 0, 0]);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

### Byte registers and REX

Without a REX prefix, 8-bit register numbers 4–7 are `ah ch dh bh`; with any
REX prefix they are `spl bpl sil dil`. So `spl`–`dil` and `r8b`–`r15b` make the
encoder emit a REX prefix (an empty `40` if no bit is needed), and `ah`–`bh`
cannot be combined with anything that needs one:
[`HighByteWithRex`](#encodeerror).

```rust
use isa_lang::x86_64::{encode, EncodeError, Inst, Mnemonic, Reg};

assert_eq!(&*encode(&Inst::new(Mnemonic::Mov, [Reg::AL.into(), Reg::AH.into()]))?, [0x88, 0xE0]);
assert_eq!(&*encode(&Inst::new(Mnemonic::Mov, [Reg::AL.into(), Reg::SPL.into()]))?, [0x40, 0x88, 0xE0]);
assert_eq!(
    encode(&Inst::new(Mnemonic::Movzx, [Reg::RAX.into(), Reg::AH.into()])),
    Err(EncodeError::HighByteWithRex),
);
# Ok::<(), EncodeError>(())
```

### Form selection

[`encode`](#encode) tries the mnemonic's forms in table order and uses the
first that fits. The table lists shorter forms first, so the choice is always
a shortest encoding (tested against every alternative): `imm8` before
`imm32`, accumulator forms where they are shorter, `rel8` before `rel32`,
shift-by-one before shift-by-`imm8`, `REX.W C7 /0 id` before
`REX.W B8+rd io` for a 64-bit `mov` of a small constant.
[`encode_form`](#encode_form) bypasses the choice.

### Decoder acceptance

The decoder accepts exactly the encodings the form table describes, plus two
redundancies that occur in real code: an empty REX prefix (`40`), and REX.W on
forms whose operation is 64-bit by default (`push`, `pop`, indirect `jmp` and
`call`). It rejects, with the offset of the offending byte:

- legacy prefixes other than `66` (`LOCK`, `REP`, segment, address size);
- a repeated `66`; a REX prefix not immediately before the opcode;
- REX.R, REX.X, or REX.B with no operand to extend, REX.W on a form without a
  64-bit row, and `66` on a form without a 16-bit row (bits the processor
  would ignore are errors here, not silently dropped);
- opcodes and opcode extensions outside the table.

This makes the decoder an exact inverse of the encoder and a precise oracle
for it. Decoding foreign code that uses ignored prefix bits is out of scope
for 0.2.

```rust
use isa_lang::x86_64::{decode, DecodeError};

assert_eq!(decode(&[0x40, 0x01, 0xC3]).unwrap().inst().to_string(), "add ebx, eax");
assert_eq!(decode(&[0x48, 0xFF, 0xE0]).unwrap().inst().to_string(), "jmp rax");
assert_eq!(decode(&[0x41, 0xC3]), Err(DecodeError::UnusedRex { at: 0 }));
assert_eq!(decode(&[0x64, 0x8B, 0x00]), Err(DecodeError::UnsupportedPrefix { at: 0, byte: 0x64 }));
```

### Text format

`Display` prints Intel syntax in one fixed format, so equal instructions print
identically:

- lowercase mnemonic, then operands separated by `, `, destination first;
- `size ptr [base+index*scale±disp]`, the scale always written, a zero
  displacement omitted, `[rip±disp]` for RIP-relative, `[0x…]` for absolute;
  `lea` omits `size ptr`;
- every number in hexadecimal, `-0x…` when negative;
- branch targets as `$±0x…`, or absolute through [`display_at`](#inst).

Width and alignment flags (`{:<40}`) are honoured.

```rust
use isa_lang::x86_64::{Inst, Mem, Mnemonic, Reg, Scale, Size};

let i = Inst::new(Mnemonic::Lea, [Reg::RAX.into(), Mem::new(Size::Qword, Reg::RBX).with_index(Reg::RCX, Scale::X1).with_disp(-8).into()]);
assert_eq!(i.to_string(), "lea rax, [rbx+rcx*1-0x8]");
assert_eq!(format!("[{i:<30}]"), "[lea rax, [rbx+rcx*1-0x8]      ]");
```

## `encode`

```rust,ignore
pub fn encode(inst: &Inst) -> Result<Encoded, EncodeError>
```

Encodes one instruction with the shortest form that fits
([form selection](#form-selection)). Allocation-free: the bytes are on the
stack in the returned [`Encoded`](#encoded).

| Parameter | Meaning |
|---|---|
| `inst` | The instruction. Operands destination first. |

**Errors.** [`EncodeError`](#encodeerror): `NoForm` if no form takes these
operand kinds and widths; `ImmediateOutOfRange` or `TargetOutOfRange` if forms
take the kinds but the value does not fit; `HighByteWithRex`; `BaseNotQword`,
`IndexNotQword`, `RspIndex`, `RipWithIndex` for malformed memory operands. When
several forms fail, the error is the closest miss.

```rust
use isa_lang::x86_64::{encode, EncodeError, Inst, Mem, Mnemonic, Reg, Size};

let push = encode(&Inst::new(Mnemonic::Push, [Reg::R12.into()]))?;
assert_eq!(&*push, [0x41, 0x54]);

let m = Mem::new(Size::Qword, Reg::RAX);
assert_eq!(
    encode(&Inst::new(Mnemonic::Mov, [m.into(), m.into()])),
    Err(EncodeError::NoForm { mnemonic: Mnemonic::Mov }),
);
# Ok::<(), EncodeError>(())
```

## `encode_form`

```rust,ignore
pub fn encode_form(form: &Form, inst: &Inst) -> Result<Encoded, EncodeError>
```

Encodes one instruction with exactly the given [`Form`](#form). The
instruction selector's door: a pattern that names a form gets that form's
bytes.

| Parameter | Meaning |
|---|---|
| `form` | A form from [`forms`](#forms), [`Encoded::form`](#encoded), or [`Decoded::form`](#decoded). |
| `inst` | The instruction; its mnemonic must be the form's. |

**Errors.** `NoForm` if the form belongs to another mnemonic or does not take
these operands; otherwise as [`encode`](#encode).

```rust
use isa_lang::x86_64::{encode_form, forms, Inst, Mnemonic, Operand, Reg};

let long = forms().iter().find(|f| f.syntax() == "JMP rel32").unwrap();
let inst = Inst::new(Mnemonic::Jmp, [Operand::Rel(5)]);
assert_eq!(&*encode_form(long, &inst)?, [0xE9, 0x00, 0x00, 0x00, 0x00]);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

## `decode`

```rust,ignore
pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError>
```

Decodes the instruction at the start of `bytes`; trailing bytes are ignored
and [`Decoded::len`](#decoded) says how many were used. Never panics, on any
input; never allocates.

**Errors.** [`DecodeError`](#decodeerror), naming the offset of the byte at
fault ([acceptance](#decoder-acceptance)).

```rust
use isa_lang::x86_64::{decode, DecodeError};

let d = decode(&[0x0F, 0xAF, 0xC1, 0xC3])?;
assert_eq!((d.inst().to_string(), d.len()), ("imul eax, ecx".to_string(), 3));
assert_eq!(decode(&[0x0F, 0xAF]), Err(DecodeError::Truncated { at: 2 }));
# Ok::<(), DecodeError>(())
```

## `decode_all`

```rust,ignore
pub fn decode_all(bytes: &[u8]) -> Instructions<'_>
```

An iterator over the instructions in `bytes`, each with its offset. It yields
the first error and stops. See [`Instructions`](#instructions).

```rust
use isa_lang::x86_64::decode_all;

let offsets: Vec<usize> = decode_all(&[0x55, 0x48, 0x89, 0xE5, 0xC3])
    .map(|r| r.map(|(at, _)| at))
    .collect::<Result<_, _>>()?;
assert_eq!(offsets, [0, 1, 4]);
# Ok::<(), isa_lang::x86_64::DecodeError>(())
```

## `disassemble`

```rust,ignore
pub fn disassemble(bytes: &[u8], ip: u64) -> Disassembly<'_>
```

A listing of `bytes` as code located at address `ip`: one line per
instruction (`address  bytes  text`), branch targets as absolute addresses,
and bytes that do not decode listed one at a time as `(bad)`. See
[`Disassembly`](#disassembly).

```rust
use isa_lang::x86_64::disassemble;

assert_eq!(
    disassemble(&[0xE8, 0x00, 0x00, 0x00, 0x00], 0x2000).to_string(),
    "0000000000002000  e8 00 00 00 00           call 0x2005\n",
);
```

## `forms`

```rust,ignore
pub fn forms() -> &'static [Form]
```

Every [`Form`](#form) of the x86-64 description, in table order. Within one
mnemonic, table order is the encoder's preference.

```rust
use isa_lang::x86_64::forms;

assert_eq!(forms().len(), 340);
let jcc: Vec<&str> = forms().iter().filter(|f| f.reads_condition()).map(|f| f.syntax()).collect();
assert_eq!(jcc, ["SETcc r/m8", "CMOVcc r16, r/m16", "CMOVcc r32, r/m32", "CMOVcc r64, r/m64", "Jcc rel8", "Jcc rel32"]);
```

## `recommended_nop`

```rust,ignore
pub fn recommended_nop(len: usize) -> Option<&'static [u8]>
```

The Intel SDM's recommended single `NOP` instruction of `len` bytes (1 to 9,
SDM Vol. 2B, `NOP`, Table 4-12), or `None`. Fixed byte sequences: padding
needs an exact length, which the encoder's shortest-form choice would not
give. [`Assembler::nops`](#assembler) uses them.

```rust
use isa_lang::x86_64::recommended_nop;

assert_eq!(recommended_nop(3), Some(&[0x0F, 0x1F, 0x00][..]));
assert_eq!(recommended_nop(10), None);
```

## `Inst`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inst { /* private */ }
impl Display for Inst {}
impl Debug for Inst {}   // Inst(add rax, rbx)
```

A [`Mnemonic`](#mnemonic) and up to three [`Operand`](#operand)s, destination
first. Equality and hashing compare the mnemonic and the operands that exist.

| Method | Meaning |
|---|---|
| `new<const N: usize>(mnemonic, [Operand; N]) -> Inst` | `const`. More than three operands is a compile-time error. |
| `from_slice(mnemonic, &[Operand]) -> Option<Inst>` | `None` for more than three operands. |
| `mnemonic(&self) -> Mnemonic` | The operation. |
| `operands(&self) -> &[Operand]` | The operands, destination first. |
| `display_at(&self, ip: u64) -> DisplayAt<'_>` | Text with branch targets as absolute addresses for an instruction at `ip`. |

```rust
use isa_lang::x86_64::{Cond, Inst, Mnemonic, Operand, Reg};

const RET: Inst = Inst::new(Mnemonic::Ret, []);
assert_eq!(RET.operands().len(), 0);

let ops = [Operand::from(Reg::RCX), Operand::Imm(3)];
let shl = Inst::from_slice(Mnemonic::Shl, &ops).unwrap();
assert_eq!(shl.to_string(), "shl rcx, 0x3");

let j = Inst::new(Mnemonic::Jcc(Cond::B), [Operand::Rel(-6)]);
assert_eq!(j.display_at(0x100).to_string(), "jb 0xfa");
```

## `Mnemonic`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mnemonic {
    Adc, Add, And, Bt, Call, Cbw, Cdq, Cdqe, Cmovcc(Cond), Cmp, Cqo, Cwd, Cwde,
    Dec, Div, Idiv, Imul, Inc, Int3, Jcc(Cond), Jmp, Lea, Mov, Movsx, Movsxd,
    Movzx, Mul, Neg, Nop, Not, Or, Pop, Push, Ret, Rol, Ror, Sar, Sbb,
    Setcc(Cond), Shl, Shr, Sub, Test, Ud2, Xchg, Xor,
}
impl Display for Mnemonic {}
```

The operation. Condition families carry their [`Cond`](#cond). The operand
count picks among forms of one mnemonic (`imul` has one-, two-, and
three-operand forms). The enum is `#[non_exhaustive]`: more mnemonics arrive
with more forms.

```rust
use isa_lang::x86_64::{Cond, Mnemonic};

assert_eq!(Mnemonic::Setcc(Cond::Ae).to_string(), "setae");
assert_eq!(Mnemonic::Movsxd.to_string(), "movsxd");
```

## `Operand`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operand {
    Reg(Reg),
    Mem(Mem),
    Imm(i64),   // see "Immediates"
    Rel(i64),   // see "Branch targets"
}
impl From<Reg> for Operand {}
impl From<Mem> for Operand {}
impl Display for Operand {}
```

```rust
use isa_lang::x86_64::{Mem, Operand, Reg, Size};

assert_eq!(Operand::from(Reg::R10).to_string(), "r10");
assert_eq!(Operand::from(Mem::new(Size::Word, Reg::RDX)).to_string(), "word ptr [rdx]");
assert_eq!(Operand::Imm(-16).to_string(), "-0x10");
assert_eq!(Operand::Rel(5).to_string(), "$+0x5");
```

## `Reg`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Reg { /* one byte */ }
impl Display for Reg {}   // the name
impl Debug for Reg {}     // the name
```

A general-purpose register at a width. Constants for all 68:
`RAX`–`R15`, `EAX`–`R15D`, `AX`–`R15W`, `AL`–`R15B` (with `SPL BPL SIL DIL`),
and `AH CH DH BH`.

| Method | Meaning |
|---|---|
| `new(size, num) -> Option<Reg>` | `const`. Register `num` (0–15) at `size`; `None` above 15. At `Size::Byte`, 4–7 are `SPL`–`DIL`. |
| `size(self) -> Size` | `const`. The width. |
| `num(self) -> u8` | `const`. The 4-bit hardware number; `AH`–`BH` report 4–7. |
| `is_high_byte(self) -> bool` | `const`. Whether it is `AH`, `CH`, `DH`, or `BH`. |
| `name(self) -> &'static str` | `const`. The lowercase name. |

```rust
use isa_lang::x86_64::{Reg, Size};

let r = Reg::new(Size::Word, 9).unwrap();
assert_eq!((r, r.name(), r.num()), (Reg::R9W, "r9w", 9));
assert_eq!(Reg::new(Size::Byte, 4), Some(Reg::SPL));
assert!(Reg::DH.is_high_byte() && Reg::DH.num() == 6);
```

## `Size`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Size { Byte, Word, Dword, Qword }
```

| Method | Meaning |
|---|---|
| `bits(self) -> u32` | `const`. 8, 16, 32, 64. |
| `bytes(self) -> u32` | `const`. 1, 2, 4, 8. |

```rust
use isa_lang::x86_64::Size;

assert_eq!((Size::Byte.bits(), Size::Qword.bytes()), (8, 8));
```

## `Mem`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mem { /* eight bytes */ }
impl Display for Mem {}   // qword ptr [rbp-0x8]
impl Debug for Mem {}
```

A memory operand ([concepts](#memory-operands)). Building one never fails;
malformed addresses are rejected by the encoder.

| Method | Meaning |
|---|---|
| `new(size, base) -> Mem` | `const`. `[base]`. |
| `rip(size, disp) -> Mem` | `const`. `[rip + disp]`. |
| `absolute(size, addr) -> Mem` | `const`. `[addr]`; add an index for `[index*scale + addr]`. |
| `with_disp(self, disp) -> Mem` | `const`. Replace the displacement. |
| `with_index(self, index, scale) -> Mem` | `const`. Add or replace the index. |
| `with_size(self, size) -> Mem` | `const`. Replace the access size. |
| `size`, `base`, `index`, `scale`, `disp`, `is_rip_relative` | `const` accessors. `base` is `None` for RIP-relative and absolute; `scale` is `X1` without an index. |

```rust
use isa_lang::x86_64::{Mem, Reg, Scale, Size};

let m = Mem::new(Size::Qword, Reg::R13).with_index(Reg::RAX, Scale::X8).with_disp(0x20);
assert_eq!(m.to_string(), "qword ptr [r13+rax*8+0x20]");
assert_eq!((m.base(), m.index(), m.scale(), m.disp()), (Some(Reg::R13), Some(Reg::RAX), Scale::X8, 0x20));
assert!(Mem::rip(Size::Byte, 0).is_rip_relative());
```

## `Scale`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scale { X1, X2, X4, X8 }
```

| Method | Meaning |
|---|---|
| `factor(self) -> u8` | `const`. 1, 2, 4, 8. |

```rust
use isa_lang::x86_64::Scale;

assert_eq!(Scale::X4.factor(), 4);
```

## `Cond`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cond { O, No, B, Ae, E, Ne, Be, A, S, Ns, P, Np, L, Ge, Le, G }
impl Display for Cond {}   // the suffix
```

A condition code; the discriminant is the hardware encoding, and a condition's
negation differs in bit 0.

| Method | Meaning |
|---|---|
| `code(self) -> u8` | `const`. 0–15. |
| `from_code(u8) -> Option<Cond>` | `const`. `None` above 15. |
| `negate(self) -> Cond` | `const`. The opposite condition. |
| `reads(self) -> Flags` | `const`. The flags tested. |
| `suffix(self) -> &'static str` | `const`. `"e"` in `je`. |

```rust
use isa_lang::x86_64::{Cond, Flags};

assert_eq!(Cond::Ge.negate(), Cond::L);
assert_eq!(Cond::A.reads(), Flags::CF | Flags::ZF);
assert_eq!(Cond::from_code(9).map(Cond::suffix), Some("ns"));
```

## `DisplayAt`

```rust,ignore
#[derive(Clone, Copy, Debug)]
pub struct DisplayAt<'a> { /* private */ }
impl Display for DisplayAt<'_> {}
```

Returned by [`Inst::display_at`](#inst): the instruction's text with `Rel`
targets printed as absolute addresses (`ip + d`, wrapping).

```rust
use isa_lang::x86_64::{Inst, Mnemonic, Operand};

let call = Inst::new(Mnemonic::Call, [Operand::Rel(0x10)]);
assert_eq!(call.display_at(0xFFFF_FFFF_FFFF_FFF8).to_string(), "call 0x8");
```

## `Encoded`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encoded { /* up to 15 bytes, on the stack */ }
impl Deref<Target = [u8]> for Encoded {}
impl AsRef<[u8]> for Encoded {}
impl Debug for Encoded {}   // Encoded(48 83 c0 08)
```

| Method | Meaning |
|---|---|
| `as_bytes(&self) -> &[u8]` | The bytes (also through `Deref`). |
| `form(&self) -> &'static Form` | The row used. |
| `flags(&self) -> FlagEffects` | `const`. Flag effects, with a condition's reads filled in. |

```rust
use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Reg};

let e = encode(&Inst::new(Mnemonic::Neg, [Reg::RDX.into()]))?;
assert_eq!((e.len(), e.form().syntax()), (3, "NEG r/m64"));
assert_eq!(e.flags().written(), Flags::ALL);
let mut buf = Vec::new();
buf.extend_from_slice(&e);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

## `Decoded`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Decoded { /* private */ }
```

| Method | Meaning |
|---|---|
| `inst(&self) -> Inst` | `const`. The instruction. |
| `len(&self) -> usize` | `const`. Bytes used, 1–15. |
| `form(&self) -> &'static Form` | The row the bytes matched. |
| `flags(&self) -> FlagEffects` | `const`. Flag effects, with a condition's reads filled in. |

```rust
use isa_lang::x86_64::{decode, Flags};

let d = decode(&[0x0F, 0x4C, 0xC1])?;   // cmovl eax, ecx
assert_eq!(d.inst().to_string(), "cmovl eax, ecx");
assert_eq!(d.form().encoding(), "0F 40+cc /r");
assert_eq!(d.flags().read(), Flags::SF | Flags::OF);
# Ok::<(), isa_lang::x86_64::DecodeError>(())
```

## `Instructions`

```rust,ignore
#[derive(Clone, Debug)]
pub struct Instructions<'a> { /* private */ }
impl Iterator for Instructions<'_> { type Item = Result<(usize, Decoded), DecodeError>; }
impl FusedIterator for Instructions<'_> {}
```

Returned by [`decode_all`](#decode_all). Yields `(offset, Decoded)` for each
instruction; on a decode error, yields the error (its offset relative to the
whole buffer) and then ends.

```rust
use isa_lang::x86_64::{decode_all, DecodeError};

let mut it = decode_all(&[0x90, 0x0F, 0x0B, 0xF4]);
assert_eq!(it.next().map(|r| r.map(|(at, _)| at)), Some(Ok(0)));
assert_eq!(it.next().map(|r| r.map(|(at, _)| at)), Some(Ok(1)));
assert_eq!(it.next().map(|r| r.map(|(at, _)| at)), Some(Err(DecodeError::UnknownOpcode { at: 3, escaped: false, opcode: 0xF4 })));
assert!(it.next().is_none());
```

## `Disassembly`

```rust,ignore
#[derive(Clone, Copy, Debug)]
pub struct Disassembly<'a> { /* private */ }
impl Display for Disassembly<'_> {}
```

Returned by [`disassemble`](#disassemble). Each line: the address as 16 hex
digits, two spaces, the bytes in lowercase hex padded to 25 columns, the
Intel-syntax text; `(bad)` for a byte that does not decode. Writing it
allocates nothing.

```rust
use isa_lang::x86_64::disassemble;

let listing = disassemble(&[0x48, 0x31, 0xC0, 0xF4], 0).to_string();
assert_eq!(listing.lines().collect::<Vec<_>>(), [
    "0000000000000000  48 31 c0                 xor rax, rax",
    "0000000000000003  f4                       (bad)",
]);
```

## `Form`

```rust,ignore
#[derive(Clone, Copy)]
pub struct Form { /* private */ }
impl PartialEq for Form {}   // by table position
impl Eq for Form {}
impl Display for Form {}     // "REX.W 83 /0 ib | ADD r/m64, imm8"
impl Debug for Form {}
```

One row of the target description: an encoding recipe, an operand signature,
and flag effects.

| Method | Meaning |
|---|---|
| `encoding(&self) -> &'static str` | `const`. The recipe in the SDM's notation, with `66` written and `+cc` for condition codes. |
| `syntax(&self) -> &'static str` | `const`. The signature, with `uimm8`/`uimm16` for unsigned fields. |
| `operands(&self) -> &[OperandKind]` | The operand kinds, destination first. |
| `flags(&self) -> FlagEffects` | `const`. Flag effects; for condition forms, `read` is empty (see `reads_condition`). |
| `reads_condition(&self) -> bool` | `const`. Whether the form reads its condition's flags. |
| `index(&self) -> usize` | `const`. Position in [`forms`](#forms). Not stable across releases; key long-lived tables by `(encoding, syntax)`. |

```rust
use isa_lang::x86_64::{forms, OperandKind, Size};

let f = forms().iter().find(|f| f.syntax() == "MOVZX r64, r/m16").unwrap();
assert_eq!(f.encoding(), "REX.W 0F B7 /r");
assert_eq!(f.operands(), [OperandKind::Reg(Size::Qword), OperandKind::RegMem(Size::Word)]);
assert_eq!(f.to_string(), "REX.W 0F B7 /r | MOVZX r64, r/m16");
assert_eq!(forms()[f.index()], *f);
```

## `OperandKind`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperandKind {
    Reg(Size),                       // r8 .. r64
    RegMem(Size),                    // r/m8 .. r/m64
    Mem(Size),                       // m (lea: the operation's width)
    Fixed(Reg),                      // AL, AX, EAX, RAX, CL
    One,                             // the constant 1 (shift by one)
    Imm { bits: u8, signed: bool },  // imm8 .. imm64, uimm8, uimm16
    Rel { bits: u8 },                // rel8, rel32
}
```

```rust
use isa_lang::x86_64::{forms, OperandKind, Reg};

let shl = forms().iter().find(|f| f.syntax() == "SHL r/m8, CL").unwrap();
assert_eq!(shl.operands()[1], OperandKind::Fixed(Reg::CL));
let ret = forms().iter().find(|f| f.syntax() == "RET uimm16").unwrap();
assert_eq!(ret.operands()[0], OperandKind::Imm { bits: 16, signed: false });
```

## `Flags`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(/* RFLAGS bits */);
impl BitOr for Flags {}
impl Display for Flags {}   // "CF|ZF", "none"
impl Debug for Flags {}
```

A set of status flags. Constants `NONE CF PF AF ZF SF OF ALL`; the bits are
the `RFLAGS` positions.

| Method | Meaning |
|---|---|
| `bits(self) -> u16` | `const`. |
| `union(self, other) -> Flags` | `const`. Also `|`. |
| `contains(self, other) -> bool` | `const`. |
| `is_empty(self) -> bool` | `const`. |

```rust
use isa_lang::x86_64::Flags;

let f = Flags::SF | Flags::OF;
assert!(f.contains(Flags::OF) && !f.is_empty());
assert_eq!(f.to_string(), "SF|OF");
assert_eq!(Flags::ZF.bits(), 0x40);
```

## `FlagEffects`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FlagEffects { /* read, written, undefined */ }
```

| Method | Meaning |
|---|---|
| `read(&self) -> Flags` | `const`. Flags the instruction depends on. |
| `written(&self) -> Flags` | `const`. Flags set to a defined value. |
| `undefined(&self) -> Flags` | `const`. Flags left unspecified. |
| `clobbered(&self) -> Flags` | `const`. `written ∪ undefined`. |

Shifts and rotates change no flags when the masked count is 0; `written`
describes a non-zero count.

```rust
use isa_lang::x86_64::{encode, Flags, Inst, Mnemonic, Operand, Reg};

let bt = encode(&Inst::new(Mnemonic::Bt, [Reg::EAX.into(), Operand::Imm(3)]))?;
assert_eq!(bt.flags().written(), Flags::CF);
assert!(!bt.flags().clobbered().contains(Flags::ZF));
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

## `Assembler`

```rust,ignore
#[derive(Clone, Default)]
pub struct Assembler { /* private */ }
```

A code buffer with labels. Instructions are encoded as they are emitted;
`jmp`/`jcc` to a label are relaxed at [`finish`](#assembler) to `rel8` where
they reach and `rel32` where they do not (the least fixed point: no branch is
longer than necessary); `call` and RIP-relative references to a label are
32-bit fields patched at `finish`.

| Method | Meaning | Errors |
|---|---|---|
| `new() -> Assembler` | Empty. | |
| `with_capacity(bytes) -> Assembler` | Empty, with room for `bytes`. | |
| `label(&mut self) -> Label` | A new, unbound label. | |
| `bind(&mut self, Label)` | Bind the label here. | `Rebound`, `UnknownLabel` |
| `emit(&mut self, &Inst)` | Encode an instruction (`Rel` operands as given). | `Encode`, `TooLarge` |
| `bytes(&mut self, &[u8])` | Raw bytes: data, or code from elsewhere. | `TooLarge` |
| `nops(&mut self, len)` | `len` bytes of recommended `NOP`s. | `TooLarge` |
| `jmp(&mut self, Label)` | Relaxed `jmp`. | `UnknownLabel`, `TooLarge` |
| `jcc(&mut self, Cond, Label)` | Relaxed `jcc`. | `UnknownLabel`, `TooLarge` |
| `call(&mut self, Label)` | `call rel32`. | `UnknownLabel`, `TooLarge` |
| `emit_rip(&mut self, &Inst, Label)` | `inst`'s RIP-relative operand resolved to `label + disp`. | `NoRipOperand`, `Encode`, `UnknownLabel`, `TooLarge` |
| `finish(self) -> Result<Code, AsmError>` | Lay out, relax, patch. | `Unbound` |

All methods but `new`, `with_capacity`, `label`, and `finish` return
`Result<(), AsmError>`. `MAX_CODE` is `i32::MAX`: the most code one assembler
produces, counting every branch at its longest.

```rust
use isa_lang::x86_64::{Assembler, Cond, Inst, Mem, Mnemonic, Operand, Reg, Size};

let mut asm = Assembler::new();
let table = asm.label();
let out = asm.label();
asm.emit(&Inst::new(Mnemonic::Cmp, [Reg::EDI.into(), Operand::Imm(3)]))?;
asm.jcc(Cond::Ae, out)?;
// rax = table[rdi]
asm.emit_rip(&Inst::new(Mnemonic::Lea, [Reg::RAX.into(), Mem::rip(Size::Qword, 0).into()]), table)?;
asm.emit(&Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Mem::new(Size::Qword, Reg::RAX).with_index(Reg::RDI, isa_lang::x86_64::Scale::X8).into()]))?;
asm.bind(out)?;
asm.emit(&Inst::new(Mnemonic::Ret, []))?;
asm.bind(table)?;
for v in [10u64, 20, 30] {
    asm.bytes(&v.to_le_bytes())?;
}
let code = asm.finish()?;
assert_eq!(code.label_offset(table), Some(17));
assert_eq!(code.bytes().len(), 17 + 24);
# Ok::<(), isa_lang::x86_64::AsmError>(())
```

## `Label`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(/* private */);
impl Debug for Label {}
```

A position in one assembler's code. `index(self) -> usize` is its creation
number. A label from another assembler is `UnknownLabel` (or, if that
assembler has as many labels, a different label of this one: keep labels with
the assembler that made them).

```rust
use isa_lang::x86_64::Assembler;

let mut asm = Assembler::new();
let (a, b) = (asm.label(), asm.label());
assert_eq!((a.index(), b.index()), (0, 1));
```

## `Code`

```rust,ignore
#[derive(Clone, PartialEq, Eq)]
pub struct Code { /* private */ }
```

| Method | Meaning |
|---|---|
| `bytes(&self) -> &[u8]` | The machine code. |
| `into_bytes(self) -> Vec<u8>` | The machine code, by value. |
| `label_offset(&self, Label) -> Option<usize>` | Final offset of a bound label. |

```rust
use isa_lang::x86_64::{Assembler, Inst, Mnemonic};

let mut asm = Assembler::new();
let entry = asm.label();
asm.emit(&Inst::new(Mnemonic::Int3, []))?;
asm.bind(entry)?;
asm.emit(&Inst::new(Mnemonic::Ret, []))?;
let code = asm.finish()?;
assert_eq!((code.label_offset(entry), code.bytes()), (Some(1), &[0xCC, 0xC3][..]));
# Ok::<(), isa_lang::x86_64::AsmError>(())
```

## `EncodeError`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodeError { /* variants below */ }
impl Display for EncodeError {}
impl core::error::Error for EncodeError {}
```

| Variant | Meaning | What to do |
|---|---|---|
| `NoForm { mnemonic }` | No form takes operands of these kinds, widths, and count. | Check order (destination first) and widths; memory-to-memory has no form. |
| `ImmediateOutOfRange { value }` | Forms take the kinds, but the value fits none. | Use the signed value at the operation's width; load wider constants into a register. |
| `TargetOutOfRange { offset }` | The branch target is beyond `rel32`. | Branch through a register. |
| `HighByteWithRex` | `AH`–`BH` with an operand that needs REX. | Use another byte register. |
| `BaseNotQword { base }` | A memory base is not 64-bit. | Addresses are 64-bit. |
| `IndexNotQword { index }` | A memory index is not 64-bit. | Addresses are 64-bit. |
| `RspIndex` | `RSP` as an index. | Swap base and index (scale 1). |
| `RipWithIndex` | A RIP-relative operand with an index. | Compute the address with `lea` first. |

```rust
use isa_lang::x86_64::{encode, EncodeError, Inst, Mem, Mnemonic, Reg, Scale, Size};

let bad = Mem::new(Size::Qword, Reg::RAX).with_index(Reg::RSP, Scale::X1);
let err = encode(&Inst::new(Mnemonic::Mov, [Reg::RAX.into(), bad.into()])).unwrap_err();
assert_eq!(err, EncodeError::RspIndex);
assert_eq!(err.to_string(), "rsp cannot be an index register");
```

## `DecodeError`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecodeError { /* variants below */ }
impl Display for DecodeError {}
impl core::error::Error for DecodeError {}
```

Every variant carries `at`, the offset of the byte at fault; `at(&self)` returns
it.

| Variant | Meaning |
|---|---|
| `Truncated { at }` | The input ends before the instruction; `at` is the first missing byte. |
| `UnsupportedPrefix { at, byte }` | A legacy prefix other than `66`. |
| `RepeatedPrefix { at }` | A second `66`. |
| `MisplacedRex { at }` | A REX prefix followed by another prefix. |
| `UnknownOpcode { at, escaped, opcode }` | No form has this opcode (`escaped`: after `0F`). |
| `InvalidOperandSize { at }` | The opcode has no form with this `66`/REX.W combination. |
| `UnknownExtension { at, ext }` | The opcode has no form with this ModRM `/digit`. |
| `RegisterOperand { at }` | Memory required (`lea`), ModRM names a register. |
| `UnusedRex { at }` | REX.R, REX.X, or REX.B set with no operand using it. |

```rust
use isa_lang::x86_64::{decode, DecodeError};

assert_eq!(decode(&[0x8D, 0xC0]), Err(DecodeError::RegisterOperand { at: 1 }));
assert_eq!(decode(&[0x66, 0x63, 0xC0]), Err(DecodeError::InvalidOperandSize { at: 1 }));
assert_eq!(decode(&[0x66, 0x66, 0x90]).unwrap_err().at(), 1);
```

## `AsmError`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AsmError {
    Encode(EncodeError),
    UnknownLabel(Label),
    Rebound(Label),
    Unbound(Label),
    NoRipOperand,
    TooLarge,
}
impl From<EncodeError> for AsmError {}
impl Display for AsmError {}
impl core::error::Error for AsmError {}   // source() is the EncodeError
```

| Variant | Meaning |
|---|---|
| `Encode(e)` | An emitted instruction could not be encoded. |
| `UnknownLabel(l)` | The label was not made by this assembler. |
| `Rebound(l)` | The label is already bound. |
| `Unbound(l)` | At `finish`: a branch or reference names a label never bound. |
| `NoRipOperand` | `emit_rip` without a RIP-relative memory operand. |
| `TooLarge` | Past `Assembler::MAX_CODE`. |

```rust
use isa_lang::x86_64::{AsmError, Assembler, Inst, Mnemonic, Reg};

let mut asm = Assembler::new();
let l = asm.label();
let plain = Inst::new(Mnemonic::Mov, [Reg::EAX.into(), Reg::EBX.into()]);
assert_eq!(asm.emit_rip(&plain, l), Err(AsmError::NoRipOperand));
```

## Constants

| Constant | Value | Meaning |
|---|---|---|
| `MAX_INST_LEN` | 15 | The longest x86-64 instruction; [`Encoded`](#encoded) holds this many bytes. |
| `MAX_OPERANDS` | 3 | The most operands an [`Inst`](#inst) has. |
| `Assembler::MAX_CODE` | `i32::MAX` | The most code one assembler produces. |

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | No effect today. Without it the crate is `no_std` and needs only `alloc`; the API is identical. |

## Limits

| Limit | Value | Error |
|---|---|---|
| Operands per instruction | 3 | compile time (`Inst::new`), `None` (`Inst::from_slice`) |
| Instruction length | 15 bytes (14 for this table) | never exceeded |
| Assembler output | `i32::MAX` bytes, counting each branch at its longest | `AsmError::TooLarge` |
| Labels per assembler | `u32::MAX` | further labels never bind (`UnknownLabel`) |

## Guide: emitting a function

A backend lowers a function to instructions, then emits it once. Labels stand
for blocks; the assembler chooses branch sizes.

```rust
use isa_lang::x86_64::{decode_all, Assembler, Cond, Inst, Mem, Mnemonic, Operand, Reg, Scale, Size};

// i64 max(const i64 *p, u64 n), System V: p in rdi, n in rsi; n >= 1.
fn max_fn() -> Result<Vec<u8>, isa_lang::x86_64::AsmError> {
    use Mnemonic::*;
    let mut asm = Assembler::new();
    let (top, skip, done) = (asm.label(), asm.label(), asm.label());
    let elem = Mem::new(Size::Qword, Reg::RDI).with_index(Reg::RCX, Scale::X8);
    asm.emit(&Inst::new(Mov, [Reg::RAX.into(), Mem::new(Size::Qword, Reg::RDI).into()]))?;
    asm.emit(&Inst::new(Mov, [Reg::ECX.into(), Operand::Imm(1)]))?;
    asm.bind(top)?;
    asm.emit(&Inst::new(Cmp, [Reg::RCX.into(), Reg::RSI.into()]))?;
    asm.jcc(Cond::Ae, done)?;
    asm.emit(&Inst::new(Mov, [Reg::RDX.into(), elem.into()]))?;
    asm.emit(&Inst::new(Cmp, [Reg::RDX.into(), Reg::RAX.into()]))?;
    asm.jcc(Cond::Le, skip)?;
    asm.emit(&Inst::new(Mov, [Reg::RAX.into(), Reg::RDX.into()]))?;
    asm.bind(skip)?;
    asm.emit(&Inst::new(Inc, [Reg::RCX.into()]))?;
    asm.jmp(top)?;
    asm.bind(done)?;
    asm.emit(&Inst::new(Ret, []))?;
    Ok(asm.finish()?.into_bytes())
}

let code = max_fn()?;
let text: Vec<String> = decode_all(&code)
    .map(|r| r.map(|(at, d)| d.inst().display_at(at as u64).to_string()))
    .collect::<Result<_, _>>()
    .unwrap();
assert_eq!(text, [
    "mov rax, qword ptr [rdi]",
    "mov ecx, 0x1",
    "cmp rcx, rsi",
    "jae 0x1e",
    "mov rdx, qword ptr [rdi+rcx*8]",
    "cmp rdx, rax",
    "jle 0x19",
    "mov rax, rdx",
    "inc rcx",
    "jmp 0x8",
    "ret",
]);
# Ok::<(), isa_lang::x86_64::AsmError>(())
```

## Guide: instruction selection through forms

An instruction selector generated from the description works in forms: it
finds the forms it needs once, by identity, and emits each with
[`encode_form`](#encode_form). Flag effects tell it when a compare can feed a
later branch.

```rust
use isa_lang::x86_64::{encode_form, forms, Cond, Flags, Form, Inst, Mnemonic, Operand, Reg};

fn form(syntax: &str) -> &'static Form {
    forms().iter().find(|f| f.syntax() == syntax).unwrap()
}

let add_imm8 = form("ADD r/m64, imm8");
let lea = form("LEA r64, m");
let cmp = form("CMP r/m64, r64");

// The selector knows a 64-bit add of a small constant fits ADD r/m64, imm8 ...
let i = Inst::new(Mnemonic::Add, [Reg::RBX.into(), Operand::Imm(16)]);
assert_eq!(&*encode_form(add_imm8, &i)?, [0x48, 0x83, 0xC3, 0x10]);

// ... that it clobbers the flags a following jl would read ...
assert!(add_imm8.flags().clobbered().contains(Cond::L.reads()));

// ... and that LEA does not, so it can sit between a compare and its branch.
assert_eq!(lea.flags().clobbered(), Flags::NONE);
assert_eq!(cmp.flags().written(), Flags::ALL);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```
