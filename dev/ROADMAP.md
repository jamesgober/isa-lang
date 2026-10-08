# isa-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation
- [ ] Target-description table format; x86-64 integer, control-flow, and memory core: encoder, decoder, round-trip property tests.

## v0.5.0 - Implementation
- [ ] AArch64 core; floating point and SIMD subsets used by the backend; textual assembler for both.

## v0.9.0 - Hardening
- [ ] Fuzzing decoders; exhaustive form coverage audit; benchmarks.

## v1.0.0 - Stable
- [ ] Frozen after backend-lang emits through it on both ISAs (D18).
