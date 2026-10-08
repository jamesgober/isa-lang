//! Round-trip properties over the whole operand space of every form.
//!
//! For each form, every operand position is enumerated over its whole
//! candidate space (every register, every addressing shape, every
//! immediate/displacement boundary) while the other positions take
//! representative values, every register pair is enumerated, and every
//! condition code of the `cc` families. Each instruction must:
//!
//! - encode with that exact form, and decode back to the same instruction,
//!   the same form, and the same length;
//! - encode with the free encoder too, no longer than the form's encoding,
//!   and decode back to the same instruction;
//!
//! or be refused for a documented reason (`AH`-`BH` with REX, `XCHG`'s `+r`
//! forms with register 0). Randomized operands then cover the combinations
//! between the boundaries.

mod common;

use common::{
    DISPS, SCALES, candidates, check_form, conds, is_cc, mnemonic, regs, rel_len, representatives,
};
use isa_lang::x86_64::{
    Cond, Form, Inst, Mem, Mnemonic, Operand, OperandKind, Reg, Size, encode, forms,
};
use proptest::prelude::*;

/// Calls `f` with every combination taking `full` at `pos` and one of
/// `reps` everywhere else.
fn each_combination(
    all: &[Vec<Operand>],
    reps: &[Vec<Operand>],
    pos: usize,
    f: &mut impl FnMut(&[Operand]),
) {
    let n = all.len();
    let mut idx = vec![0usize; n];
    let lists: Vec<&[Operand]> = (0..n)
        .map(|i| {
            if i == pos {
                all[i].as_slice()
            } else {
                reps[i].as_slice()
            }
        })
        .collect();
    loop {
        let ops: Vec<Operand> = (0..n).map(|i| lists[i][idx[i]]).collect();
        f(&ops);
        let mut k = 0;
        loop {
            if k == n {
                return;
            }
            idx[k] += 1;
            if idx[k] < lists[k].len() {
                break;
            }
            idx[k] = 0;
            k += 1;
        }
    }
}

fn register_list(kind: OperandKind) -> Option<Vec<Operand>> {
    match kind {
        OperandKind::Reg(s) | OperandKind::RegMem(s) => {
            Some(regs(s).into_iter().map(Operand::Reg).collect())
        }
        _ => None,
    }
}

fn round_trip_form(form: &Form) -> (usize, usize) {
    let len = rel_len(form);
    let kinds = form.operands();
    let all: Vec<Vec<Operand>> = kinds.iter().map(|&k| candidates(k, len)).collect();
    let reps: Vec<Vec<Operand>> = all.iter().map(|c| representatives(c)).collect();
    let conds: Vec<Cond> = if is_cc(form) {
        conds().collect()
    } else {
        vec![Cond::E]
    };
    let (mut tried, mut encoded) = (0, 0);
    let mut check = |m: Mnemonic, ops: &[Operand]| {
        let inst = Inst::from_slice(m, ops).unwrap();
        tried += 1;
        if check_form(form, &inst) {
            encoded += 1;
        }
    };
    for &cond in &conds {
        let m = mnemonic(form, cond);
        if kinds.is_empty() {
            check(m, &[]);
        }
        for pos in 0..kinds.len() {
            each_combination(&all, &reps, pos, &mut |ops| check(m, ops));
        }
        // Every pair of registers in two register-capable positions.
        for a in 0..kinds.len() {
            for b in a + 1..kinds.len() {
                let (Some(ra), Some(rb)) = (register_list(kinds[a]), register_list(kinds[b]))
                else {
                    continue;
                };
                let mut lists = reps.clone();
                lists[a] = ra;
                lists[b] = rb;
                each_combination(&lists, &lists, usize::MAX, &mut |ops| check(m, ops));
            }
        }
    }
    (tried, encoded)
}

#[test]
fn every_form_round_trips_over_its_operand_space() {
    let mut tried = 0;
    let mut encoded = 0;
    for form in forms() {
        let (t, e) = round_trip_form(form);
        assert!(e > 0, "{form} never encodes");
        tried += t;
        encoded += e;
    }
    // Guard against the enumeration silently shrinking.
    assert!(tried > 500_000, "only {tried} instructions tried");
    assert!(encoded * 100 > tried * 95, "{encoded} of {tried} encoded");
}

#[test]
fn every_form_has_a_distinct_row() {
    let all = forms();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert!(
                (a.encoding(), a.syntax()) != (b.encoding(), b.syntax()),
                "duplicate row {a}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Randomized operands
// ---------------------------------------------------------------------------

fn pick<T: Copy>(items: &[T], seed: u64) -> T {
    items[(seed % items.len() as u64) as usize]
}

fn random_disp(seed: u64) -> i32 {
    match seed % 4 {
        0 => 0,
        1 => (seed >> 8) as i8 as i32,
        2 => pick(&DISPS, seed >> 8),
        _ => (seed >> 16) as i32,
    }
}

fn random_mem(size: Size, seed: u64) -> Mem {
    let base = seed % 18;
    let index = (seed >> 5) % 17;
    let scale = pick(&SCALES, seed >> 10);
    let disp = random_disp(seed >> 12);
    let q = |n: u64| Reg::new(Size::Qword, n as u8).unwrap();
    let m = match base {
        16 => return Mem::rip(size, disp),
        17 => Mem::absolute(size, disp),
        b => Mem::new(size, q(b)).with_disp(disp),
    };
    if index < 16 && index != 4 {
        m.with_index(q(index), scale)
    } else {
        m
    }
}

fn random_in(min: i64, max: i64, seed: u64) -> i64 {
    let span = (max as i128 - min as i128 + 1) as u128;
    (min as i128 + (seed as u128 % span) as i128) as i64
}

fn random_operand(kind: OperandKind, seed: u64, rel_len: i64) -> Operand {
    match kind {
        OperandKind::Reg(s) => Operand::Reg(pick(&regs(s), seed)),
        OperandKind::RegMem(s) if seed % 2 == 0 => Operand::Reg(pick(&regs(s), seed >> 1)),
        OperandKind::RegMem(s) | OperandKind::Mem(s) => Operand::Mem(random_mem(s, seed >> 1)),
        OperandKind::Fixed(r) => Operand::Reg(r),
        OperandKind::One => Operand::Imm(1),
        OperandKind::Imm { bits, signed } => {
            let (min, max) = match (bits, signed) {
                (64, _) => (i64::MIN, i64::MAX),
                (b, true) => (-(1i64 << (b - 1)), (1i64 << (b - 1)) - 1),
                (b, false) => (0, (1i64 << b) - 1),
            };
            Operand::Imm(random_in(min, max, seed))
        }
        OperandKind::Rel { bits } => {
            let half = 1i64 << (bits - 1);
            Operand::Rel(random_in(-half, half - 1, seed) + rel_len)
        }
        _ => panic!("unknown operand kind"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 20_000, ..ProptestConfig::default() })]

    #[test]
    fn random_operands_round_trip(
        index in 0..forms().len(),
        seeds in proptest::array::uniform3(any::<u64>()),
        cond in 0u8..16,
    ) {
        let form = &forms()[index];
        let len = rel_len(form);
        let ops: Vec<Operand> = form
            .operands()
            .iter()
            .zip(seeds)
            .map(|(&k, s)| random_operand(k, s, len))
            .collect();
        let inst = Inst::from_slice(mnemonic(form, Cond::from_code(cond).unwrap()), &ops).unwrap();
        check_form(form, &inst);
    }

    #[test]
    fn out_of_range_immediates_are_refused_never_truncated(value in any::<i64>()) {
        // 64-bit add takes a sign-extended imm32; everything else is refused.
        let inst = Inst::new(Mnemonic::Add, [Reg::RAX.into(), Operand::Imm(value)]);
        let fits = i32::try_from(value).is_ok();
        prop_assert_eq!(encode(&inst).is_ok(), fits);
        let mov = Inst::new(Mnemonic::Mov, [Reg::R11.into(), Operand::Imm(value)]);
        prop_assert!(encode(&mov).is_ok());
        let mov32 = Inst::new(Mnemonic::Mov, [Reg::R11D.into(), Operand::Imm(value)]);
        prop_assert_eq!(encode(&mov32).is_ok(), fits);
    }
}
