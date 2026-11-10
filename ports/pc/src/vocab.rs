//! The vocabulary: the Apple version's format (ports/apple2/src/vocab.rs: a word, NUL,
//! then position, type and id; ending with an empty word), stored plainly (the PCjr's
//! has each byte rotated). It is in the engine's data segment, 4 bytes in.

pub use kq1_formats::vocab::{Entry, Names};

/// The first edition's data segment starts `$5110` bytes into its code segment, DS
/// being CS plus `$511` (start-up, `$3E`: the relocated `$511`, less the 32 paragraphs
/// of the EXE header the image was loaded after). Each edition's:
/// `disk::Edition::data_segment`.
pub const DATA_SEGMENT: usize = 0x5110;
/// The vocabulary's offset in the data segment.
pub const VOCAB: usize = 0x4;

/// The vocabulary, from the engine's code segment (`Disk::code_segment`) of `edition`.
pub fn read(code_segment: &[u8], edition: crate::disk::Edition) -> Result<Vec<Entry>, String> {
    let at = edition.data_segment() + edition.ds(VOCAB).ok_or("no vocabulary")?;
    kq1_formats::vocab::parse(code_segment.get(at..).ok_or("engine too short")?)
}
