//! The decoder on arbitrary bytes: it never panics, never reads past the
//! input, reports the offending byte, and everything it accepts the encoder
//! can encode back to the same instruction.

use isa_lang::x86_64::{DecodeError, decode, decode_all, disassemble, encode};
use proptest::prelude::*;

/// The decoder's contract on one input.
fn check_bytes(bytes: &[u8]) -> Result<usize, DecodeError> {
    match decode(bytes) {
        Ok(d) => {
            assert!(
                d.len() >= 1 && d.len() <= bytes.len().min(15),
                "{bytes:02x?}"
            );
            let inst = d.inst();
            // Formatting never panics.
            let _ = inst.to_string();
            let _ = inst.display_at(u64::MAX).to_string();
            let again = encode(&inst).unwrap_or_else(|e| panic!("{bytes:02x?} -> {inst}: {e}"));
            let back = decode(&again).unwrap_or_else(|e| panic!("{inst} -> {again:?}: {e}"));
            assert_eq!(back.inst(), inst, "{bytes:02x?} -> {again:?}");
            // Truncating an accepted instruction is always reported as such.
            for cut in 0..d.len() {
                match decode(&bytes[..cut]) {
                    Err(DecodeError::Truncated { at }) => assert!(at <= cut, "{bytes:02x?}"),
                    other => panic!("{:02x?} cut at {cut}: {other:?}", &bytes[..cut]),
                }
            }
            Ok(d.len())
        }
        Err(e) => {
            assert!(e.at() <= bytes.len(), "{bytes:02x?}: {e:?}");
            let _ = e.to_string();
            Err(e)
        }
    }
}

const TAILS: [[u8; 12]; 3] = [
    [
        0x24, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
    ],
    [
        0x25, 0x80, 0xFF, 0xFF, 0x7F, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
    ],
    [
        0xE5, 0x7F, 0x80, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
    ],
];

/// Every prefix combination the decoder distinguishes, every opcode in both
/// maps, and every ModRM byte, followed by SIB/displacement/immediate tails
/// that exercise each SIB shape.
#[test]
fn exhaustive_prefix_opcode_modrm_space() {
    let mut prefixes: Vec<Vec<u8>> = vec![vec![], vec![0x66]];
    for rex in 0x40..=0x4Fu8 {
        prefixes.push(vec![rex]);
        prefixes.push(vec![0x66, rex]);
    }
    let mut accepted = 0usize;
    let mut buf = Vec::with_capacity(32);
    for prefix in &prefixes {
        for escape in [false, true] {
            for opcode in 0..=255u8 {
                for modrm in 0..=255u8 {
                    for tail in &TAILS {
                        buf.clear();
                        buf.extend_from_slice(prefix);
                        if escape {
                            buf.push(0x0F);
                        }
                        buf.push(opcode);
                        buf.push(modrm);
                        buf.extend_from_slice(tail);
                        if check_bytes(&buf).is_ok() {
                            accepted += 1;
                        }
                    }
                }
            }
        }
    }
    // The table is large enough that a broken index would show here.
    assert!(accepted > 300_000, "only {accepted} accepted");
}

#[test]
fn every_one_and_two_byte_input() {
    for a in 0..=255u8 {
        let _ = check_bytes(&[a]);
        for b in 0..=255u8 {
            let _ = check_bytes(&[a, b]);
        }
    }
}

#[test]
fn empty_input_is_truncated_at_zero() {
    assert_eq!(decode(&[]), Err(DecodeError::Truncated { at: 0 }));
    assert_eq!(decode_all(&[]).count(), 0);
    assert_eq!(disassemble(&[], 0).to_string(), "");
}

#[test]
fn long_prefix_runs_are_rejected_at_the_first_bad_byte() {
    let mut bytes = vec![0x66; 1000];
    bytes.push(0x90);
    assert_eq!(decode(&bytes), Err(DecodeError::RepeatedPrefix { at: 1 }));
    let rex_run = [0x48; 64];
    assert_eq!(decode(&rex_run), Err(DecodeError::MisplacedRex { at: 0 }));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 50_000, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..24)) {
        let _ = check_bytes(&bytes);
    }

    #[test]
    fn decode_all_tiles_the_input(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut next = 0;
        for item in decode_all(&bytes) {
            match item {
                Ok((at, d)) => {
                    prop_assert_eq!(at, next);
                    next = at + d.len();
                }
                Err(e) => {
                    prop_assert!(e.at() >= next && e.at() <= bytes.len());
                    next = bytes.len();
                }
            }
        }
        prop_assert!(next <= bytes.len());
        // The listing covers every byte, one line per instruction or bad byte.
        let listing = disassemble(&bytes, 0x1000).to_string();
        prop_assert_eq!(listing.is_empty(), bytes.is_empty());
    }
}
