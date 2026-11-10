//! The text font. The original draws its text through the PCjr's BIOS, in the ROM's
//! 8 × 8 font, which can't be shipped. The port has its own for the printable ASCII
//! characters, in the same bold style (vertical strokes two pixels wide) but of its own
//! design: its 5 × 7 letters with each stroke doubled, and those whose strokes would
//! close up (M, W, m, w, ...) drawn as such; the rightmost column is left clear between
//! characters, and the bottom row is for the descenders of g, j, p, q and y. The
//! player's own ROM font can be used instead ([`Font::from_dump`]): a dump of it, or
//! the whole PCjr BIOS image it is in.

use kq1_ibm::font::BUILT_IN;

/// Where the PCjr BIOS (F000:0000) keeps its 8 × 8 font for codes 0-127.
const BIOS_FONT: usize = 0xFA6E;

/// 8 × 8 glyphs for character codes 0-255; those it doesn't have are blank.
#[derive(Clone)]
pub struct Font {
    glyphs: Vec<[u8; 8]>,
}

impl Default for Font {
    fn default() -> Self {
        let mut glyphs = vec![[0u8; 8]; 256];
        glyphs[0x20..0x7F].copy_from_slice(&BUILT_IN);
        Font { glyphs }
    }
}

impl Font {
    /// A font dump, 8 bytes a character from code 0: 1,024 bytes (codes 0-127, as the
    /// PCjr's ROM holds them at F000:FA6E) or 2,048 (0-255); or a 64 KB PCjr BIOS image
    /// (F000:0000-FFFF), the font taken from F000:FA6E. Codes it lacks keep the
    /// built-in glyphs.
    pub fn from_dump(bytes: &[u8]) -> Result<Font, String> {
        let bytes = match bytes.len() {
            1024 | 2048 => bytes,
            0x10000 => {
                let font = &bytes[BIOS_FONT..BIOS_FONT + 1024];
                // The space blank, the letters inked: a font, not some other 64 KB.
                let looks_right = font[0x20 * 8..0x21 * 8].iter().all(|&b| b == 0)
                    && (b'A'..=b'Z').all(|c| font[c as usize * 8..c as usize * 8 + 8].iter().any(|&b| b != 0));
                if !looks_right {
                    return Err("no font at F000:FA6E: not a PCjr BIOS image".into());
                }
                font
            }
            n => {
                return Err(format!("a font is a 1,024- or 2,048-byte dump or a 64 KB PCjr BIOS image, not {n} bytes"))
            }
        };
        let mut f = Font::default();
        for (i, g) in bytes.chunks(8).enumerate() {
            f.glyphs[i].copy_from_slice(g);
        }
        Ok(f)
    }

    /// Character `c`'s rows.
    pub fn glyph(&self, c: u8) -> &[u8; 8] {
        &self.glyphs[c as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_have_ink_and_the_space_none() {
        let f = Font::default();
        assert!(f.glyph(b' ').iter().all(|&r| r == 0));
        for c in b'!'..=b'~' {
            assert!(f.glyph(c).iter().any(|&r| r != 0), "{}", c as char);
        }
        // The rightmost column is clear, between characters.
        assert!((0x20u8..0x7F).all(|c| f.glyph(c).iter().all(|&r| r & 0x01 == 0)));
    }

    #[test]
    fn a_bios_image_gives_its_font() {
        // A made-up BIOS image: its font at F000:FA6E, every glyph but the space inked.
        let mut rom = vec![0u8; 0x10000];
        for c in 0..128usize {
            if c != 0x20 {
                rom[BIOS_FONT + c * 8] = c as u8 | 1;
            }
        }
        let f = Font::from_dump(&rom).expect("a font");
        assert_eq!(f.glyph(b'A')[0], b'A' | 1);
        assert!(f.glyph(b' ').iter().all(|&r| r == 0));
        // 64 KB of something else.
        assert!(Font::from_dump(&vec![0u8; 0x10000]).is_err());
    }
}
