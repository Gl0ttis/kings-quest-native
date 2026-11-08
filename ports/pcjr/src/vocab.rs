//! The PCjr vocabulary: the Apple version's format (ports/apple2/src/vocab.rs: a word,
//! NUL, then position, type and id; ending with an empty word), with every byte
//! rotated right one bit. It is in the engine's data segment, at the address its
//! word list pointer `[$CD6]` starts with (`$16`); the parser (`$E10`) walks it.
//! The word ids differ from the Apple version's.

pub use kq1_apple2::vocab::{Entry, Names};

/// The engine's data segment starts `$4E30` bytes into its code segment
/// (DS = CS + `$4E3`, `$537`).
pub const DATA_SEGMENT: usize = 0x4E30;
/// The vocabulary's offset in the data segment (`[$CD6]`'s initial value).
pub const VOCAB: usize = 0x16;

/// The vocabulary, from the engine's code segment (`Disk::code_segment`).
pub fn read(code_segment: &[u8]) -> Result<Vec<Entry>, String> {
    let start = DATA_SEGMENT + VOCAB;
    let raw: Vec<u8> = code_segment.get(start..).ok_or("engine too short")?.iter().map(|b| b.rotate_left(1)).collect();
    kq1_apple2::vocab::parse(&raw)
}
