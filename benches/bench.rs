//! Criterion benchmarks: encoding, decoding, assembling, and disassembly
//! text, over a 100,000-instruction mix shaped like compiler output.
//!
//! ```text
//! cargo bench --bench bench
//! ```

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use isa_lang::x86_64::{
    Assembler, Cond, Inst, Mem, Mnemonic, Operand, Reg, Scale, Size, decode, decode_all, encode,
    encode_form,
};

const N: usize = 100_000;

fn reg(n: usize, size: Size) -> Reg {
    Reg::new(size, (n % 16) as u8).unwrap_or(Reg::RAX)
}

/// A deterministic mix: loads and stores through frame and array addresses,
/// register arithmetic, compares, small immediates, zero extension, calls,
/// and branches.
fn workload() -> Vec<Inst> {
    let mut v = Vec::with_capacity(N);
    let mut i = 0usize;
    while v.len() < N {
        let a = reg(i * 7 + 1, Size::Qword);
        let b = reg(i * 5 + 3, Size::Qword);
        let frame = Mem::new(Size::Qword, Reg::RBP).with_disp(-8 * (1 + (i % 32) as i32));
        let index = reg(i + 2, Size::Qword);
        let elem = if index == Reg::RSP {
            Mem::new(Size::Dword, b)
        } else {
            Mem::new(Size::Dword, b).with_index(index, Scale::X4)
        };
        let cond = Cond::from_code((i % 16) as u8).unwrap_or(Cond::E);
        let inst = match i % 16 {
            0 => Inst::new(Mnemonic::Mov, [a.into(), frame.into()]),
            1 => Inst::new(Mnemonic::Mov, [frame.into(), b.into()]),
            2 => Inst::new(Mnemonic::Add, [a.into(), b.into()]),
            3 => Inst::new(Mnemonic::Sub, [a.into(), Operand::Imm((i % 100) as i64)]),
            4 => Inst::new(Mnemonic::Cmp, [a.into(), Operand::Imm(1000 + i as i64 % 7)]),
            5 => Inst::new(Mnemonic::Jcc(cond), [Operand::Rel(-40)]),
            6 => Inst::new(
                Mnemonic::Lea,
                [a.into(), Mem::new(Size::Qword, b).with_disp(16).into()],
            ),
            7 => Inst::new(
                Mnemonic::Movzx,
                [
                    reg(i, Size::Dword).into(),
                    elem.with_size(Size::Byte).into(),
                ],
            ),
            8 => Inst::new(Mnemonic::Mov, [reg(i, Size::Dword).into(), elem.into()]),
            9 => Inst::new(Mnemonic::Imul, [a.into(), b.into()]),
            10 => Inst::new(
                Mnemonic::Xor,
                [reg(i, Size::Dword).into(), reg(i, Size::Dword).into()],
            ),
            11 => Inst::new(Mnemonic::Shl, [a.into(), Operand::Imm(3)]),
            12 => Inst::new(Mnemonic::Call, [Operand::Rel(0x1000)]),
            13 => Inst::new(Mnemonic::Mov, [a.into(), Operand::Imm(0x1234_5678_9ABC)]),
            14 => Inst::new(Mnemonic::Test, [a.into(), a.into()]),
            _ => Inst::new(Mnemonic::Jmp, [Operand::Rel(0x400)]),
        };
        v.push(inst);
        i += 1;
    }
    v
}

fn bench_encode(c: &mut Criterion) {
    let insts = workload();
    let forms: Vec<_> = insts
        .iter()
        .filter_map(|i| encode(i).ok().map(|e| e.form()))
        .collect();
    assert_eq!(forms.len(), N, "the workload must encode");
    let mut g = c.benchmark_group("encode");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("select_and_encode_100k", |bench| {
        let mut out = Vec::with_capacity(N * 8);
        bench.iter(|| {
            out.clear();
            for inst in &insts {
                if let Ok(e) = encode(black_box(inst)) {
                    out.extend_from_slice(&e);
                }
            }
            black_box(out.len())
        });
    });
    g.bench_function("encode_form_100k", |bench| {
        let mut out = Vec::with_capacity(N * 8);
        bench.iter(|| {
            out.clear();
            for (inst, form) in insts.iter().zip(&forms) {
                if let Ok(e) = encode_form(form, black_box(inst)) {
                    out.extend_from_slice(&e);
                }
            }
            black_box(out.len())
        });
    });
    g.finish();
}

fn encoded_workload() -> Vec<u8> {
    let mut out = Vec::new();
    for inst in workload() {
        if let Ok(e) = encode(&inst) {
            out.extend_from_slice(&e);
        }
    }
    out
}

fn bench_decode(c: &mut Criterion) {
    let bytes = encoded_workload();
    let decoded: Vec<Inst> = decode_all(&bytes)
        .filter_map(Result::ok)
        .map(|(_, d)| d.inst())
        .collect();
    assert_eq!(decoded.len(), N, "the workload must decode");
    let mut g = c.benchmark_group("decode");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("decode_100k", |bench| {
        bench.iter(|| {
            let mut at = 0;
            let mut n = 0;
            while let Some(rest) = bytes.get(at..).filter(|r| !r.is_empty()) {
                match decode(black_box(rest)) {
                    Ok(d) => at += d.len(),
                    Err(_) => break,
                }
                n += 1;
            }
            black_box(n)
        });
    });
    g.bench_function("display_100k", |bench| {
        let mut text = String::with_capacity(N * 32);
        bench.iter(|| {
            text.clear();
            for inst in &decoded {
                let _ = writeln!(text, "{inst}");
            }
            black_box(text.len())
        });
    });
    g.finish();
}

fn bench_assemble(c: &mut Criterion) {
    let insts = workload();
    let mut g = c.benchmark_group("assemble");
    g.throughput(Throughput::Elements(N as u64));
    g.bench_function("labels_and_relaxation_100k", |bench| {
        bench.iter(|| {
            let mut asm = Assembler::with_capacity(N * 8);
            let mut top = asm.label();
            for (i, inst) in insts.iter().enumerate() {
                match i % 16 {
                    0 => {
                        top = asm.label();
                        let _ = asm.bind(top);
                    }
                    5 => {
                        let _ = asm.jcc(Cond::Ne, top);
                        continue;
                    }
                    _ => {}
                }
                let _ = asm.emit(inst);
            }
            black_box(asm.finish().map(|c| c.bytes().len()))
        });
    });
    g.finish();
}

criterion_group!(benches, bench_encode, bench_decode, bench_assemble);
criterion_main!(benches);
