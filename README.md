<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>isa-lang</b>
    <br>
    <sub><sup>INSTRUCTION SET ENCODING</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/isa-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/isa-lang"></a>
    <a href="https://crates.io/crates/isa-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/isa-lang?color=%230099ff"></a>
    <a href="https://docs.rs/isa-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/isa-lang"></a>
    <a href="https://github.com/jamesgober/isa-lang/actions"><img alt="CI" src="https://github.com/jamesgober/isa-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>isa-lang</strong> turns machine instructions into bytes and bytes back into instructions. It is the last step of a native compiler backend: the instruction selector and register allocator decide <em>what</em> to run, and isa-lang produces the exact bytes, decodes them again for testing and debugging, and prints them as Intel-syntax assembly.
    </p>
    <p>
        Encoding is driven by a <strong>target description</strong>: every instruction form is one row of data, written the way the architecture manual's opcode tables write it (<code>REX.W 83 /0 ib</code> &nbsp;<code>ADD r/m64, imm8</code>), and one table-driven encoder and one decoder interpret every row. Adding a form is adding a row. This release covers the <strong>x86-64</strong> integer, control-flow, and memory core &mdash; 340 forms &mdash; with an assembler that resolves labels and picks short or long branches. AArch64 follows in the same model.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, no dependencies.
    </p>
    <blockquote>
        <strong>Status: pre-1.0.</strong> v0.2.0 delivers the x86-64 core. The public surface is designed across the 0.x series and frozen at <code>1.0.0</code>, after the family's native backend emits through it (see <a href="./dev/ROADMAP.md"><code>dev/ROADMAP.md</code></a> and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>).
    </blockquote>
</div>

<hr>
<br>

## The model

- An **[`Inst`](./docs/API.md#inst)** is a [`Mnemonic`](./docs/API.md#mnemonic) and up to three **[`Operand`](./docs/API.md#operand)**s: a [`Reg`](./docs/API.md#reg), a [`Mem`](./docs/API.md#mem) (`qword ptr [base+index*scale+disp]`, `[rip+disp]`, or absolute), an immediate, or a relative branch target. It says *what* to execute; the encoder chooses the bytes.
- **[`encode`](./docs/API.md#encode)** picks the shortest form that fits and returns the bytes on the stack, with the form used. **[`decode`](./docs/API.md#decode)** turns bytes back into the same `Inst`. `Display` prints Intel syntax; **[`disassemble`](./docs/API.md#disassemble)** lists a buffer.
- An **[`Assembler`](./docs/API.md#assembler)** adds labels: branches to labels are relaxed to `rel8` where they reach and `rel32` where they do not, and `call`s and RIP-relative references to labels are patched.
- **[`forms`](./docs/API.md#forms)** exposes the target description itself: every **[`Form`](./docs/API.md#form)**'s encoding recipe, operand kinds, and flag effects. **[`encode_form`](./docs/API.md#encode_form)** emits one named form, which is what an instruction selector generated from the description calls.

<br>

What it guarantees, and how each guarantee is checked:

| Guarantee | How it is held |
|---|---|
| `decode(encode(i)) == i`, and the decoder reports the form that was used. | Every form's operand space is enumerated position by position (all registers, every base × index × scale, every displacement and immediate boundary, every condition code) and every register pair: over 500,000 instructions, plus random operands. |
| Whatever the decoder accepts, the encoder encodes back to the same instruction. | Every prefix combination × every opcode in both maps × every ModRM byte × several SIB/displacement tails: 13.4 million inputs, plus every 1- and 2-byte input and random bytes. |
| The decoder never panics, and every error names the byte at fault. | The same inputs, including every accepted instruction cut short at every length. |
| Encodings match the Intel SDM byte for byte. | 171 known-answer tests with the REX/ModRM/SIB arithmetic written out. While building, the encodings were also checked against LLVM 18 (assembler and disassembler); see the [release notes](./docs/release/v0.2.0.md). |
| The encoder picks a shortest form. | Checked against every other form that fits, in the enumeration above. |
| Branch relaxation is optimal and fast at scale. | 3,000 random programs compared byte for byte with a naive reference; a 100,000-branch cascade relaxes in well under a second. |

<hr>
<br>

## Installation

```toml
[dependencies]
isa-lang = "0.2"
```

Without the standard library:

```toml
[dependencies]
isa-lang = { version = "0.2", default-features = false }
```

<hr>
<br>

## Quick start

```rust
use isa_lang::x86_64::{decode, encode, Inst, Mem, Mnemonic, Operand, Reg, Size};

// add rax, 8
let add = Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(8)]);
assert_eq!(&*encode(&add)?, [0x48, 0x83, 0xC0, 0x08]);

// mov dword ptr [rsp+4], r9d
let store = Inst::new(Mnemonic::Mov, [Mem::new(Size::Dword, Reg::RSP).with_disp(4).into(), Reg::R9D.into()]);
let bytes = encode(&store)?;
assert_eq!(&*bytes, [0x44, 0x89, 0x4C, 0x24, 0x04]);

// And back.
let decoded = decode(&bytes).unwrap();
assert_eq!(decoded.inst(), store);
assert_eq!(decoded.inst().to_string(), "mov dword ptr [rsp+0x4], r9d");
assert_eq!(decoded.form().to_string(), "89 /r | MOV r/m32, r32");
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

### Labels and branches

```rust
use isa_lang::x86_64::{disassemble, Assembler, Cond, Inst, Mnemonic, Reg};

// Sum 1..=n, with n in edi.
let mut asm = Assembler::new();
let top = asm.label();
let done = asm.label();
asm.emit(&Inst::new(Mnemonic::Xor, [Reg::EAX.into(), Reg::EAX.into()]))?;
asm.emit(&Inst::new(Mnemonic::Test, [Reg::EDI.into(), Reg::EDI.into()]))?;
asm.jcc(Cond::E, done)?;
asm.bind(top)?;
asm.emit(&Inst::new(Mnemonic::Add, [Reg::EAX.into(), Reg::EDI.into()]))?;
asm.emit(&Inst::new(Mnemonic::Dec, [Reg::EDI.into()]))?;
asm.jcc(Cond::Ne, top)?;
asm.bind(done)?;
asm.emit(&Inst::new(Mnemonic::Ret, []))?;
let code = asm.finish()?;

assert_eq!(
    disassemble(code.bytes(), 0x1000).to_string(),
    "0000000000001000  31 c0                    xor eax, eax\n\
     0000000000001002  85 ff                    test edi, edi\n\
     0000000000001004  74 06                    je 0x100c\n\
     0000000000001006  01 f8                    add eax, edi\n\
     0000000000001008  ff cf                    dec edi\n\
     000000000000100a  75 fa                    jne 0x1006\n\
     000000000000100c  c3                       ret\n",
);
# Ok::<(), isa_lang::x86_64::AsmError>(())
```

### Errors say what is wrong

Immediates are the operand's value as a signed integer of the operation's width, and nothing is ever truncated:

```rust
use isa_lang::x86_64::{decode, encode, DecodeError, EncodeError, Inst, Mnemonic, Operand, Reg};

// A 64-bit add takes at most a sign-extended 32-bit immediate.
let too_big = Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(1 << 40)]);
assert_eq!(encode(&too_big), Err(EncodeError::ImmediateOutOfRange { value: 1 << 40 }));

// ah cannot be encoded with a REX prefix, which r8b requires.
let mix = Inst::new(Mnemonic::Mov, [Reg::AH.into(), Reg::R8B.into()]);
assert_eq!(encode(&mix), Err(EncodeError::HighByteWithRex));

// The decoder names the byte at fault.
assert_eq!(decode(&[0x48, 0x8B]), Err(DecodeError::Truncated { at: 2 }));
assert_eq!(decode(&[0xF7, 0xC8]), Err(DecodeError::UnknownExtension { at: 1, ext: 1 }));
```

### The description, as data

```rust
use isa_lang::x86_64::{encode_form, forms, Flags, Inst, Mnemonic, Operand, Reg};

let form = forms().iter().find(|f| f.syntax() == "ADD r/m32, imm32").unwrap();
assert_eq!(form.encoding(), "81 /0 id");
assert_eq!(form.flags().written(), Flags::ALL);

// Force this form even though an 8-bit immediate would fit.
let inst = Inst::new(Mnemonic::Add, [Reg::ECX.into(), Operand::Imm(1)]);
assert_eq!(&*encode_form(form, &inst)?, [0x81, 0xC1, 0x01, 0x00, 0x00, 0x00]);
# Ok::<(), isa_lang::x86_64::EncodeError>(())
```

<hr>
<br>

## What is covered

`mov` (register, memory, and immediate forms including `imm64`), `lea`, `add` `or` `adc` `sbb` `and` `sub` `xor` `cmp` `test`, `inc` `dec` `neg` `not`, `mul` `imul` (one, two, and three operands) `div` `idiv`, `cbw` `cwde` `cdqe` `cwd` `cdq` `cqo`, `shl` `shr` `sar` `rol` `ror` (by 1, `imm8`, `cl`), `movzx` `movsx` `movsxd`, `setcc`, `cmovcc`, `jcc` (`rel8`/`rel32`), `jmp` (`rel8`/`rel32`/indirect), `call` (`rel32`/indirect), `ret`, `push`, `pop`, `nop` (one-byte and `nop r/m`, plus the manual's recommended 1–9 byte sequences), `ud2`, `int3`, `xchg`, `bt` &mdash; at 8, 16, 32, and 64 bits wherever the architecture has the form.

Not covered yet, and reported as errors by the decoder: floating point and SIMD, string instructions, `LOCK`/`REP`/segment/address-size prefixes (so no `fs:`/`gs:` TLS access yet), `enter`/`leave`, far and system instructions, and the other bit-manipulation and atomic instructions. The decoder is exact: it accepts the encodings the table describes, and rejects encodings that carry prefix bits the hardware would ignore (see [`docs/API.md`](./docs/API.md#decoder-acceptance)).

<hr>
<br>

## Examples

Runnable programs in [`examples/`](./examples):

| Example | What it shows |
|---|---|
| [`encode`](./examples/encode.rs) | The one-call path: encode a function's instructions, print bytes, text, and the form each used, and decode them back. |
| [`assemble`](./examples/assemble.rs) | A loop with labels, a RIP-relative constant, and the disassembly listing of the result. |
| [`disassemble`](./examples/disassemble.rs) | Hex bytes in, an Intel-syntax listing out. `cargo run --example disassemble -- 55 48 89 e5 c3` |

<hr>
<br>

## Performance

Encoding and decoding allocate nothing: an encoded instruction is at most 15 bytes on the stack, and a decode is a few table lookups and bounds-checked reads. Candidate forms come from two indexes built at compile time (by mnemonic and by opcode byte), so neither direction searches the whole table.

Measured with the benchmarks in [`benches/`](./benches) over a 100,000-instruction mix shaped like compiler output (frame loads and stores, array indexing, arithmetic, compares, branches, calls), Windows x86_64, Rust stable, release profile:

| Benchmark | What it measures | Per instruction | Throughput |
|---|---|---:|---:|
| `encode/select_and_encode_100k` | `encode`: choose the form, lay out, append to a buffer | ~40 ns | ~25 M/s |
| `encode/encode_form_100k` | `encode_form`: the form already chosen | ~22 ns | ~45 M/s |
| `decode/decode_100k` | `decode`, one instruction after another | ~40 ns | ~25 M/s |
| `decode/display_100k` | Intel-syntax text into a `String` | ~55 ns | ~18 M/s |
| `assemble/labels_and_relaxation_100k` | `Assembler` with a loop branch every 16 instructions, then `finish` | ~49 ns | ~20 M/s |

```bash
cargo bench --bench bench
```

Numbers vary by CPU; use the trend across runs, not a single absolute. Linux numbers are measured centrally for the release.

<hr>
<br>

## Design notes

- **One canonical instruction.** An immediate is the operand's signed value at the operation's width, a branch target is measured from the start of the branch, and a memory operand is an address, not a byte layout. So every encoding of the same operation decodes to the same `Inst`, and equal instructions compare equal.
- **Rows, not code.** The table is written as the manual writes it and compiled by a `const fn` while the crate builds; a malformed row, or a row whose operands and encoding disagree, is a compile error. The encoder and decoder never mention a mnemonic.
- **Strict decoding.** The decoder is the exact inverse of the encoder over the table, plus two redundant prefixes that real code contains (an empty REX, and REX.W on 64-bit-default forms). Everything else it rejects with the offset of the offending byte, which is what makes it a precise oracle for the encoder.
- **Least-fixed-point relaxation.** Branches start short and lengthen only when they cannot reach; lengthening never shrinks anything, so the result is unique and minimal. A lengthening re-checks only the branches within 256 bytes of it, so relaxation is O(n log n).

The target-description model, the engines, and how AArch64 fits the same model are specified in `_lexersketch/specs/TARGET-DESC.md` in the family repository.

<hr>
<br>

## Testing

```bash
cargo test                       # unit + integration + property + doctests
cargo test --no-default-features # no_std + alloc
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench bench
```

[`tests/properties.rs`](./tests/properties.rs) enumerates every form's operand space; [`tests/decoder.rs`](./tests/decoder.rs) sweeps the prefix × opcode × ModRM space and random bytes; [`tests/known_answers.rs`](./tests/known_answers.rs) holds the byte-exact SDM cases; [`tests/assembler.rs`](./tests/assembler.rs) compares the assembler with a naive reference. Every `rust` example in this README and in [`docs/API.md`](./docs/API.md) is compiled and run as a doctest.

<hr>
<br>

## Cross-platform support

- Linux (x86_64, aarch64)
- macOS (x86_64, Apple Silicon)
- Windows (x86_64)

The crate produces and reads x86-64 machine code on any host: it uses no operating-system facilities and no platform-specific code. It does not execute the code it produces; mapping code executable is `pager-lang`'s job.

<hr>
<br>

## Contributing

See [`REPS.md`](./REPS.md) for the engineering standards every change is held to, [`dev/DIRECTIVES.md`](./dev/DIRECTIVES.md) for this crate's invariants, and [`dev/ROADMAP.md`](./dev/ROADMAP.md) for what comes next. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober.</strong></sup>
</div>
