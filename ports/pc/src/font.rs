//! The BIOS's 8 × 8 font, which the original draws its text in and which can't be
//! shipped: the player's own, from an image of the IBM PC's BIOS ROM (its 8 KB, or a
//! 64 KB image of segment F000) at F000:FA6E, or a dump of the font itself.

/// Characters 00h-7Fh, 8 bytes each, a byte a row from the top, the leftmost pixel in
/// the high bit.
pub struct Font(pub Vec<u8>);

impl Font {
    /// The font from a BIOS image or a font dump (1,024 bytes, or 2,048 with characters
    /// 80h-FFh, of which the first half is taken).
    pub fn from_bios(rom: &[u8]) -> Result<Font, String> {
        let at = match rom.len() {
            0x2000 => 0x1A6E,
            0x10000 => 0xFA6E,
            0x400 | 0x800 => 0,
            n => return Err(format!("not a BIOS image or a font dump ({n} bytes)")),
        };
        Ok(Font(rom[at..at + 0x400].to_vec()))
    }

    /// Row `line` of character `c` (none above 7Fh: the BIOS takes those from the font
    /// INT 1Fh points to, which the game never sets).
    pub fn row(&self, c: u8, line: usize) -> u8 {
        if c < 0x80 {
            self.0[c as usize * 8 + line]
        } else {
            0
        }
    }
}

/// A glyph's row as the BIOS draws it in mode 4: 8 pixels of 2 bits, the leftmost
/// highest, `colour` where the row's bit is set, 0 elsewhere.
pub fn expand(row: u8, colour: u8) -> [u8; 2] {
    (0..8).fold(0u16, |w, k| w << 2 | if row >> (7 - k) & 1 != 0 { colour as u16 & 3 } else { 0 }).to_be_bytes()
}
