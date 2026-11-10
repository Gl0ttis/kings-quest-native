//! The BIOS's 8 × 8 font, which the original draws its text in and which can't be
//! shipped: the player's own, from an image of the IBM PC's BIOS ROM (its 8 KB, or a
//! 64 KB image of segment F000) at F000:FA6E, or a dump of the font itself. Without
//! one, the port's own font ([`Font::built_in`], the PCjr port's: printable ASCII in
//! the same bold style, of its own design).

/// The port's own font (crates/kq1-ibm, the PCjr port's).
use kq1_ibm::font::BUILT_IN;

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

    /// The port's own font, for the printable ASCII characters.
    pub fn built_in() -> Font {
        let mut f = vec![0u8; 0x400];
        for (i, g) in BUILT_IN.iter().enumerate() {
            f[(0x20 + i) * 8..(0x21 + i) * 8].copy_from_slice(g);
        }
        Font(f)
    }

    /// Whether this looks like the BIOS's font: the space blank, and 'A' and '0'
    /// drawn (an image that isn't the IBM PC's BIOS has other bytes there).
    pub fn plausible(&self) -> bool {
        let g = |c: usize| &self.0[c * 8..c * 8 + 8];
        g(0x20).iter().all(|&b| b == 0)
            && g(0x41).iter().filter(|&&b| b != 0).count() >= 5
            && g(0x30).iter().any(|&b| b != 0)
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
