<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>isa-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

---

## [0.2.0] - 2026-10-08

Foundation: the target-description model and the x86-64 integer, control-flow,
and memory core. Instructions encode to bytes and decode back through one
table of 340 forms, written the way the Intel SDM writes its opcode tables.

### Added

- `x86_64` module. The target description: 340 instruction forms as rows of
  data (`row("REX.W 83 /0 ib", "ADD r/m64, imm8", ARITH)`), compiled by a
  `const fn` during the build, so a malformed row or one whose operands and
  encoding disagree is a compile error. Indexes by mnemonic and by opcode are
  built at compile time.
- Forms: `mov` (register, memory, `imm8`–`imm64`), `lea`, `add` `or` `adc`
  `sbb` `and` `sub` `xor` `cmp` `test`, `inc` `dec` `neg` `not`, `mul` `imul`
  (one, two, three operands) `div` `idiv`, `cbw` `cwde` `cdqe` `cwd` `cdq`
  `cqo`, `shl` `shr` `sar` `rol` `ror` (by 1, `imm8`, `cl`), `movzx` `movsx`
  `movsxd`, `setcc`, `cmovcc`, `jcc` (`rel8`/`rel32`), `jmp`
  (`rel8`/`rel32`/indirect), `call` (`rel32`/indirect), `ret`, `push`, `pop`,
  `nop` (`90`, `66 90`, `REX.W 90`, `0F 1F /0`), `ud2`, `int3`, `xchg`, `bt`, at
  8/16/32/64 bits wherever the architecture has the form.
- Operand model: `Reg` (sixteen registers at four widths plus `AH`–`BH`, one
  byte each), `Size`, `Mem` (base, index, scale, 32-bit displacement,
  RIP-relative, absolute; eight bytes), `Scale`, `Operand` (`Reg`, `Mem`, `Imm`,
  `Rel`), `Cond` (the sixteen condition codes with `negate` and `reads`),
  `Mnemonic`, `Inst`.
- `encode`: the shortest form that fits, allocation-free, returning `Encoded`
  (the bytes on the stack, the form used, and its flag effects).
  `encode_form`: one given form.
- `decode`: never panics, names the offending byte in every `DecodeError`,
  returns `Decoded` (instruction, length, form, flag effects). `decode_all`
  iterates a buffer; `disassemble` produces a deterministic listing.
- Intel-syntax `Display` for `Inst`, `Operand`, `Mem`, `Reg`, `Mnemonic`,
  `Cond`, and `Form`; `Inst::display_at` prints branch targets as absolute
  addresses.
- The description as data: `forms`, `Form` (`encoding`, `syntax`,
  `operands`, `flags`, `reads_condition`, `index`), `OperandKind`, `Flags`,
  `FlagEffects`.
- `Assembler`, `Label`, `Code`: labels, `call` and RIP-relative fixups, and
  branch relaxation to the least fixed point (no branch longer than
  necessary) in O(n log n), with `nops` using the SDM's recommended `NOP`
  sequences (also `recommended_nop`).
- Errors: `EncodeError`, `DecodeError`, `AsmError`, all `#[non_exhaustive]`.
- Tests: per-form enumeration of the whole operand space (over 500,000
  instructions), random operands, a 13.4-million-input decoder sweep with
  the accept-then-re-encode property, 171 SDM known-answer tests, the
  assembler against a naive reference, and 100,000-branch scale tests.
- Benchmarks (`benches/bench.rs`): encode, encode_form, decode, display, and
  assemble over a 100,000-instruction mix.
- Examples: `encode`, `assemble`, `disassemble`.
- `_lexersketch/specs/TARGET-DESC.md` (family repository): the
  target-description model, the engines, and how AArch64 fits it.

### Changed

- Version 0.2.0. `README.md` and `docs/API.md` document the surface.

---

## [0.1.0] - 2026-10-08

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/isa-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/isa-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/isa-lang/releases/tag/v0.1.0
