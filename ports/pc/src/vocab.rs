//! The vocabulary: the Apple version's format (ports/apple2/src/vocab.rs: a word, NUL,
//! then position, type and id; ending with an empty word), stored plainly (the PCjr's
//! has each byte rotated). It is in the engine's data segment, 4 bytes in.

pub use kq1_formats::vocab::{Entry, Names};

/// The engine's data segment starts `$5110` bytes into its code segment: DS = CS +
/// `$511` (start-up, `$3E`: the relocated `$511`, less the 32 paragraphs of the EXE
/// header the image was loaded after).
pub const DATA_SEGMENT: usize = 0x5110;
/// The vocabulary's offset in the data segment.
pub const VOCAB: usize = 0x4;

/// The vocabulary, from the engine's code segment (`Disk::code_segment`).
pub fn read(code_segment: &[u8]) -> Result<Vec<Entry>, String> {
    kq1_formats::vocab::parse(code_segment.get(DATA_SEGMENT + VOCAB..).ok_or("engine too short")?)
}
