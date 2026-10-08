//! The assembler against a reference: random programs of code, branches,
//! calls, RIP-relative loads, and labels are assembled twice, by the
//! `Assembler` and by a deliberately naive reference (lay out, lengthen every
//! out-of-range branch, repeat until stable; bytes written by hand), and
//! must agree byte for byte. The reference's fixed point is the least one,
//! so agreement also proves the assembler uses no long branch it does not
//! need.

use isa_lang::x86_64::{
    AsmError, Assembler, Cond, Inst, Label, Mem, Mnemonic, Operand, Reg, Size, decode_all,
};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Item {
    Bytes(usize),
    Jmp(usize),
    Jcc(u8, usize),
    Call(usize),
    /// `lea rax, [rip + label + addend]`
    Rip(usize, i32),
    Bind(usize),
}

fn filler(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn assemble(items: &[Item], labels: usize) -> Result<(Vec<u8>, Vec<Option<usize>>), AsmError> {
    let mut asm = Assembler::new();
    let ls: Vec<Label> = (0..labels).map(|_| asm.label()).collect();
    for item in items {
        match *item {
            Item::Bytes(n) => asm.bytes(&filler(n))?,
            Item::Jmp(l) => asm.jmp(ls[l])?,
            Item::Jcc(c, l) => asm.jcc(Cond::from_code(c).unwrap(), ls[l])?,
            Item::Call(l) => asm.call(ls[l])?,
            Item::Rip(l, addend) => asm.emit_rip(
                &Inst::new(
                    Mnemonic::Lea,
                    [Reg::RAX.into(), Mem::rip(Size::Qword, addend).into()],
                ),
                ls[l],
            )?,
            Item::Bind(l) => asm.bind(ls[l])?,
        }
    }
    let code = asm.finish()?;
    let offsets = ls.iter().map(|&l| code.label_offset(l)).collect();
    Ok((code.into_bytes(), offsets))
}

/// The reference: naive whole-program iteration and hand-written bytes.
fn reference(items: &[Item], labels: usize) -> (Vec<u8>, Vec<Option<usize>>) {
    let branch_count = items
        .iter()
        .filter(|i| matches!(i, Item::Jmp(_) | Item::Jcc(..)))
        .count();
    let mut long = vec![false; branch_count];
    loop {
        let (starts, label_at) = layout(items, labels, &long);
        let mut changed = false;
        let mut b = 0;
        for (i, item) in items.iter().enumerate() {
            if let Item::Jmp(l) | Item::Jcc(_, l) = *item {
                let target = label_at[l].unwrap() as i64;
                if !long[b] && !(-128..=127).contains(&(target - (starts[i] as i64 + 2))) {
                    long[b] = true;
                    changed = true;
                }
                b += 1;
            }
        }
        if !changed {
            break;
        }
    }
    let (starts, label_at) = layout(items, labels, &long);
    let mut out = Vec::new();
    let mut b = 0;
    for (i, item) in items.iter().enumerate() {
        let start = starts[i] as i64;
        let to = |l: usize| label_at[l].unwrap() as i64;
        match *item {
            Item::Bytes(n) => out.extend(filler(n)),
            Item::Jmp(l) => {
                if long[b] {
                    out.push(0xE9);
                    out.extend(((to(l) - start - 5) as i32).to_le_bytes());
                } else {
                    out.extend([0xEB, (to(l) - start - 2) as i8 as u8]);
                }
                b += 1;
            }
            Item::Jcc(c, l) => {
                if long[b] {
                    out.extend([0x0F, 0x80 + c]);
                    out.extend(((to(l) - start - 6) as i32).to_le_bytes());
                } else {
                    out.extend([0x70 + c, (to(l) - start - 2) as i8 as u8]);
                }
                b += 1;
            }
            Item::Call(l) => {
                out.push(0xE8);
                out.extend(((to(l) - start - 5) as i32).to_le_bytes());
            }
            Item::Rip(l, addend) => {
                // lea rax, [rip+disp32] = REX.W 8D /r, ModRM 00 000 101.
                out.extend([0x48, 0x8D, 0x05]);
                out.extend(((to(l) + i64::from(addend) - start - 7) as i32).to_le_bytes());
            }
            Item::Bind(_) => {}
        }
    }
    (out, label_at)
}

fn layout(items: &[Item], labels: usize, long: &[bool]) -> (Vec<usize>, Vec<Option<usize>>) {
    let mut starts = Vec::with_capacity(items.len());
    let mut label_at = vec![None; labels];
    let mut pos = 0;
    let mut b = 0;
    for item in items {
        starts.push(pos);
        pos += match *item {
            Item::Bytes(n) => n,
            Item::Jmp(_) => {
                b += 1;
                if long[b - 1] { 5 } else { 2 }
            }
            Item::Jcc(..) => {
                b += 1;
                if long[b - 1] { 6 } else { 2 }
            }
            Item::Call(_) => 5,
            Item::Rip(..) => 7,
            Item::Bind(l) => {
                label_at[l] = Some(pos);
                0
            }
        };
    }
    (starts, label_at)
}

fn program() -> impl Strategy<Value = (Vec<Item>, usize)> {
    (1usize..6).prop_flat_map(|labels| {
        let item = prop_oneof![
            4 => (0usize..140).prop_map(Item::Bytes),
            3 => (0..labels).prop_map(Item::Jmp),
            3 => (0u8..16, 0..labels).prop_map(|(c, l)| Item::Jcc(c, l)),
            1 => (0..labels).prop_map(Item::Call),
            1 => (0..labels, -64i32..64).prop_map(|(l, a)| Item::Rip(l, a)),
        ];
        (
            proptest::collection::vec(item, 0..60),
            proptest::collection::vec(any::<usize>(), labels),
        )
            .prop_map(move |(mut items, slots)| {
                // Bind every label exactly once, at a random point.
                for (l, slot) in slots.into_iter().enumerate() {
                    let at = slot % (items.len() + 1);
                    items.insert(at, Item::Bind(l));
                }
                (items, labels)
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, ..ProptestConfig::default() })]

    #[test]
    fn assembler_matches_the_naive_reference((items, labels) in program()) {
        let got = assemble(&items, labels).unwrap();
        let want = reference(&items, labels);
        prop_assert_eq!(&got.0, &want.0);
        prop_assert_eq!(&got.1, &want.1);
    }
}

#[test]
fn branches_land_on_their_labels() {
    // Decode the output and follow every branch.
    let items = vec![
        Item::Bind(0),
        Item::Bytes(100),
        Item::Jcc(4, 1),
        Item::Bytes(30),
        Item::Jmp(0),
        Item::Bytes(200),
        Item::Bind(1),
        Item::Jmp(0),
    ];
    let (bytes, offsets) = assemble(&items, 2).unwrap();
    assert_eq!(bytes, reference(&items, 2).0);
    let mut targets = Vec::new();
    let mut at = 0;
    for item in &items {
        match item {
            Item::Bytes(n) => at += n,
            Item::Bind(_) => {}
            _ => {
                let (_, d) = decode_all(&bytes[at..]).next().unwrap().unwrap();
                let Operand::Rel(rel) = d.inst().operands()[0] else {
                    panic!()
                };
                targets.push(at as i64 + rel);
                at += d.len();
            }
        }
    }
    let l0 = offsets[0].unwrap() as i64;
    let l1 = offsets[1].unwrap() as i64;
    assert_eq!(targets, [l1, l0, l0]);
}

/// Branch `i` jumps over 125 bytes and branch `i + 1` to a label just past
/// it, so it reaches exactly when branch `i + 1` is short. `tail` decides
/// whether the last branch is short, and so whether all of them are.
fn cascade(n: usize, tail: usize) -> Vec<u8> {
    let mut asm = Assembler::with_capacity(n * 131);
    let labels: Vec<Label> = (0..n).map(|_| asm.label()).collect();
    for i in 0..n {
        asm.jmp(labels[i]).unwrap();
        if i > 0 {
            asm.bind(labels[i - 1]).unwrap();
        }
        asm.bytes(&[0x90; 125]).unwrap();
    }
    asm.bytes(&vec![0x90; tail]).unwrap();
    asm.bind(labels[n - 1]).unwrap();
    asm.finish().unwrap().into_bytes()
}

#[test]
fn cascading_relaxation_at_scale() {
    // The last branch is long, so every branch must become long in turn,
    // last to first: a pass-until-stable relaxation needs one pass per branch.
    const N: usize = 100_000;
    let long = cascade(N, 200);
    assert_eq!(long.len(), N * (5 + 125) + 200);
    assert!((0..N).all(|i| long[i * 130] == 0xE9));
    // With the last branch short, nothing grows.
    let short = cascade(N, 0);
    assert_eq!(short.len(), N * (2 + 125));
    assert!((0..N).all(|i| short[i * 127] == 0xEB));
    // Both match the reference on a smaller instance.
    for tail in [0, 2, 3, 200] {
        let mut items = Vec::new();
        for i in 0..300 {
            items.push(Item::Jmp(i));
            if i > 0 {
                items.push(Item::Bind(i - 1));
            }
            items.push(Item::Bytes(125));
        }
        items.push(Item::Bytes(tail));
        items.push(Item::Bind(299));
        assert_eq!(
            assemble(&items, 300).unwrap().0,
            reference(&items, 300).0,
            "tail {tail}"
        );
    }
}

#[test]
fn many_short_branches_at_scale() {
    // 200k instructions with a loop branch every 16: all short.
    let mut asm = Assembler::new();
    let mut count = 0;
    while count < 200_000 {
        let top = asm.label();
        asm.bind(top).unwrap();
        for _ in 0..15 {
            asm.emit(&Inst::new(
                Mnemonic::Add,
                [Reg::RAX.into(), Operand::Imm(1)],
            ))
            .unwrap();
        }
        asm.jcc(Cond::Ne, top).unwrap();
        count += 16;
    }
    let code = asm.finish().unwrap();
    assert_eq!(code.bytes().len(), (200_000 / 16) * (15 * 4 + 2));
    assert_eq!(decode_all(code.bytes()).count(), 200_000);
}

#[test]
fn unbound_and_rebound_labels() {
    let mut asm = Assembler::new();
    let a = asm.label();
    asm.bind(a).unwrap();
    assert_eq!(asm.bind(a), Err(AsmError::Rebound(a)));
    let b = asm.label();
    asm.call(b).unwrap();
    assert_eq!(asm.clone().finish().unwrap_err(), AsmError::Unbound(b));
    let c = asm.label();
    asm.emit_rip(
        &Inst::new(Mnemonic::Jmp, [Mem::rip(Size::Qword, 0).into()]),
        c,
    )
    .unwrap();
    asm.bind(b).unwrap();
    assert_eq!(asm.finish().unwrap_err(), AsmError::Unbound(c));
}

#[test]
fn encode_errors_surface_through_emit() {
    let mut asm = Assembler::new();
    let bad = Inst::new(Mnemonic::Mov, [Reg::RAX.into(), Reg::EAX.into()]);
    assert!(matches!(asm.emit(&bad), Err(AsmError::Encode(_))));
    assert_eq!(
        AsmError::Encode(isa_lang::x86_64::EncodeError::RspIndex).to_string(),
        "rsp cannot be an index register"
    );
    assert_eq!(
        AsmError::TooLarge.to_string(),
        "code exceeds 2147483647 bytes"
    );
}

#[test]
fn backward_branch_at_the_rel8_boundary() {
    // A backward jmp reaches 128 bytes behind its own end: 126 bytes of
    // body plus the 2-byte jmp itself.
    for (body, short) in [(126usize, true), (127, false)] {
        let mut asm = Assembler::new();
        let top = asm.label();
        asm.bind(top).unwrap();
        asm.bytes(&vec![0x90; body]).unwrap();
        asm.jmp(top).unwrap();
        let code = asm.finish().unwrap();
        assert_eq!(code.bytes().len() - body == 2, short, "body {body}");
    }
}
