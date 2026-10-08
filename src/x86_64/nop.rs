//! The recommended multi-byte `NOP` sequences.

/// Intel SDM Vol. 2B, `NOP`, Table 4-12 "Recommended Multi-Byte Sequence of
/// NOP Instruction": one instruction of each length from 1 to 9 bytes.
const NOPS: [&[u8]; 9] = [
    &[0x90],
    &[0x66, 0x90],
    &[0x0F, 0x1F, 0x00],
    &[0x0F, 0x1F, 0x40, 0x00],
    &[0x0F, 0x1F, 0x44, 0x00, 0x00],
    &[0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00],
    &[0x0F, 0x1F, 0x80, 0x00, 0x00, 0x00, 0x00],
    &[0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
    &[0x66, 0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
];

/// The architecture manual's recommended single `NOP` instruction of `len`
/// bytes (1 to 9), or `None` for other lengths.
///
/// These are fixed byte sequences, not encoder output: the encoder always
/// picks the shortest form of an instruction, but padding needs an exact
/// length. Each sequence decodes as one `nop`.
///
/// # Examples
///
/// ```
/// use isa_lang::x86_64::{decode, recommended_nop};
///
/// let five = recommended_nop(5).unwrap();
/// assert_eq!(five, [0x0F, 0x1F, 0x44, 0x00, 0x00]);
/// assert_eq!(decode(five)?.len(), 5);
/// assert_eq!(decode(five)?.inst().to_string(), "nop dword ptr [rax+rax*1]");
/// assert_eq!(recommended_nop(0), None);
/// assert_eq!(recommended_nop(10), None);
/// # Ok::<(), isa_lang::x86_64::DecodeError>(())
/// ```
#[must_use]
pub fn recommended_nop(len: usize) -> Option<&'static [u8]> {
    len.checked_sub(1).and_then(|i| NOPS.get(i)).copied()
}
