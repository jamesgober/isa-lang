//! A disassembler: hex bytes in, an Intel-syntax listing out.
//!
//! ```text
//! cargo run --example disassemble -- 55 48 89 e5 8b 45 fc 5d c3
//! ```
//!
//! Without arguments it lists a built-in function. Bytes outside the
//! supported instruction set are listed as `(bad)`, one at a time.

use isa_lang::x86_64::disassemble;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes: Vec<u8> = if args.is_empty() {
        vec![
            0x55, 0x48, 0x89, 0xE5, 0x48, 0x83, 0xEC, 0x10, 0x89, 0x7D, 0xFC, 0x8B, 0x45, 0xFC,
            0x0F, 0xAF, 0xC0, 0x85, 0xC0, 0x7E, 0x02, 0xF7, 0xD8, 0x48, 0x83, 0xC4, 0x10, 0x5D,
            0xC3,
        ]
    } else {
        let parsed: Result<Vec<u8>, _> = args
            .iter()
            .flat_map(|a| a.split(|c: char| c == ',' || c.is_whitespace()))
            .filter(|s| !s.is_empty())
            .map(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16))
            .collect();
        match parsed {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("not a hex byte: {e}");
                std::process::exit(2);
            }
        }
    };
    print!("{}", disassemble(&bytes, 0x40_1000));
}
