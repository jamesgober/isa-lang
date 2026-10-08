# isa-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
> Specification: ../_lexersketch/specs/TARGET-DESC.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation (DONE)
- [x] Target-description table format; x86-64 integer, control-flow, and memory core: encoder, decoder, round-trip property tests.

Delivered:
- `_lexersketch/specs/TARGET-DESC.md`: the target-description model (registers and classes,
  operand kinds, ModRM/SIB memory operands, immediates, relative targets, forms as encoding
  recipe + operand signature + flag effects + attributes, the patterns hook, the ABI placeholder,
  cache-maintenance rules), how the engines interpret it, and how AArch64 fits the same model.
- The x86-64 description: 340 forms written in the SDM's notation and compiled by a `const fn`
  at build time (malformed or inconsistent rows are compile errors), with indexes by mnemonic
  and by opcode built the same way.
- Register model (`Reg`, `Size`), operand model (`Operand`, `Mem`, `Scale`, `Cond`), `Mnemonic`,
  `Inst`.
- Table-driven encoder (`encode`: shortest fitting form; `encode_form`: a given form) and decoder
  (`decode`, `decode_all`), both allocation-free; deterministic Intel-syntax `Display`,
  `display_at`, and `disassemble`.
- The description exposed as data: `forms`, `Form`, `OperandKind`, `Flags`, `FlagEffects`.
- `Assembler` with labels, `call`/RIP-relative fixups, and least-fixed-point branch relaxation
  (rel8/rel32) in O(n log n); `recommended_nop` and `Assembler::nops`.
- Proof: per-form enumeration of the whole operand space (over 500,000 instructions) and random
  operands for `decode(encode(i)) == i`, form identity, and shortest-form choice; a 13.4-million
  input sweep of prefix × opcode × ModRM space plus random bytes for "never panics" and
  "accepted ⇒ re-encodes to the same instruction"; 171 known-answer tests against the SDM; the
  assembler against a naive relaxation reference; 100,000-branch scale tests; criterion benches.
- One-off external check (D3, not part of the suite): llvm-objdump 18.1.8 agreed with every one
  of 195,609 per-form encodings and 588,468 instructions decoded from random bytes; clang's
  assembler produced the same bytes for 169 of 171 known-answer texts (the two others are `xchg`
  with its commutative operands written in the other order).

Dependency wiring (decided here, recorded per the anti-deferral rule):
- **No first-party crate wired.** isa-lang sits below every crate that would consume it
  (backend-lang, jit-lang 2.0, aot-lang 2.0) and needs nothing from above: registers, operands,
  and errors are its own vocabulary, it formats its own text, and it has no source spans or
  diagnostics to report (its errors are byte offsets).
- **pager-lang: not wired.** Making code executable belongs to the JIT, not to the encoder;
  isa-lang neither maps nor runs code.
- **NOML front end for the description: not built in 0.2.0.** The rows are Rust `const` data in
  the same three-string shape a NOML file would hold (TARGET-DESC §13). The reader belongs with
  the first crate that consumes the description besides isa-lang (backend-lang's pattern tables,
  backend-lang 0.5), where the generated isel tables need it.

Decisions recorded at 0.2.0:
- The decoder's acceptance set is exact (TARGET-DESC §7.3): prefix bits the processor ignores
  (unused REX.R/X/B, REX.W on byte forms, `66` on forms without a 16-bit row, repeated `66`) are
  errors, not silently dropped. Measured on LLVM-decoded random bytes: of 915,936 instructions
  using covered mnemonics, the decoder accepts 88.3%; the rest are out-of-scope prefixes and
  opcodes (segment, REP, LOCK, address-size, `moffs`, segment-register moves, far branches, APX)
  or carry ignored prefix bits. Whether to add a lenient mode for foreign code is open; it is not
  needed by the backend.
- `fs:`/`gs:` segment prefixes (thread-local storage access) are not supported yet. rt-lang's TLS
  needs them; they are listed for v0.5.0 below.
- A coverage-guided fuzz target is v0.9.0 work, as planned; 0.2.0 relies on the exhaustive
  prefix × opcode × ModRM sweep and proptest random bytes (no `libfuzzer-sys` dependency is
  allowed in the crate).

## v0.5.0 - Implementation
- [ ] AArch64 core; floating point and SIMD subsets used by the backend; textual assembler for both.
- [ ] `fs:`/`gs:` segment-override prefixes for TLS access (needed by rt-lang); `LOCK` with the
      atomic forms the backend emits (`xchg` is implicitly locked; `cmpxchg`, `xadd`).
- [ ] AArch64 cache-maintenance rule as data (TARGET-DESC §10).
- [ ] Implicit operands as table data: implicit register uses/defs (`RDX:RAX` for mul/div/cqo, `RSP` for push/pop/call/ret, `CL` counts) so backend-lang's register allocator can read them (coordinator review of 0.2.0).
- [ ] `Form::mnemonic()` structured accessor (backend-lang needs it; tests currently parse the syntax string).
- [ ] Assembler: alignment directive, external-symbol relocations for object-lang/AOT, cross-assembler label check.

## v0.9.0 - Hardening
- [ ] Fuzzing decoders; exhaustive form coverage audit; benchmarks.

## v1.0.0 - Stable
- [ ] Frozen after backend-lang emits through it on both ISAs (D18).
