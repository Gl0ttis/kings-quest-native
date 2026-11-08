//! Display options, all off by default (the default is the original's picture, pixel for
//! pixel, as an RGB monitor shows double hi-res):
//!
//! - **NTSC colour fringing.** On a composite monitor the colour of a dot depends on
//!   its neighbours, so colour boundaries and text get coloured fringes. Each dot takes
//!   the colour of the 4-dot window around it, read at the dot's colour phase (the usual
//!   "sliding window" model of Apple II artifact colour).
//! - **Fill specks.** A picture pixel that no line or fill reached keeps the buffer's
//!   initial white ($1F). Where all its neighbours share one colour it is shown in that
//!   colour. Display only: the buffer, and so priorities and the game, are unchanged.
//! - **The Apple character ROM.** The text screen (the inventory) uses the game's own
//!   hi-res font, since the port ships no ROM. Given a dump of the Apple IIe video ROM,
//!   it uses the real text-mode characters.

use super::text::FRAME_W;
use crate::pic::{dhgr_rgb, HEIGHT, WIDTH};
use std::rc::Rc;

#[derive(Default, Clone)]
pub struct Display {
    pub ntsc: bool,
    pub fill_specks: bool,
    pub charrom: Option<Rc<CharRom>>,
}

fn rgb(v: u8) -> u32 {
    let c = dhgr_rgb(v);
    (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
}

/// The colour the speck fill gives picture pixel (x, y), if it is a speck: still the
/// initial $1F, not covered by a sprite, with no white beside it (so white areas are
/// left alone), and at least two of its neighbours in one colour, which it takes (the
/// first such colour in the order left, right, up, down).
pub fn speck(base: &[u8], screen: &[u8], x: usize, y: usize) -> Option<u8> {
    let i = y * WIDTH + x;
    if base[i] != 0x1F || screen[i] != base[i] {
        return None;
    }
    let mut around = Vec::with_capacity(4);
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
        if nx < 0 || ny < 0 || nx >= WIDTH as i32 || ny >= HEIGHT as i32 {
            continue;
        }
        let c = base[ny as usize * WIDTH + nx as usize] & 0x0F;
        if c == 0x0F {
            return None;
        }
        around.push(c);
    }
    let count = |c: u8| around.iter().filter(|&&k| k == c).count();
    let best =
        around.iter().copied().max_by_key(|&c| (count(c), std::cmp::Reverse(around.iter().position(|&k| k == c))))?;
    (count(best) >= 2).then_some(best)
}

/// Colour fringing over a whole frame (FRAME_W dots per line).
pub fn ntsc(fb: &mut [u32]) {
    // Every colour on screen is one of the 16 (text is black and white), so each pixel
    // gives back its 4-dot pattern.
    let mut by_rgb = std::collections::HashMap::new();
    for v in 0..16u8 {
        by_rgb.insert(rgb(v), v);
    }
    let pattern = |v: u8| ((v >> 1) | (v << 3)) & 0x0F; // $0F3D: the value rotated right
    for line in fb.chunks_mut(FRAME_W) {
        let dots: Vec<u8> =
            line.iter().enumerate().map(|(i, p)| by_rgb.get(p).map_or(0, |&v| pattern(v) >> (i & 3) & 1)).collect();
        for i in 0..line.len() {
            let mut n = 0u8;
            for j in i as isize - 1..i as isize + 3 {
                if j >= 0 && (j as usize) < dots.len() && dots[j as usize] != 0 {
                    n |= 1 << (j & 3);
                }
            }
            // Back from the pattern to the stored value (rotate left), then its colour.
            line[i] = rgb(((n << 1) | (n >> 3)) & 0x0F);
        }
    }
}

/// An Apple video ROM's characters: for each screen code, 8 rows of 7 dots, bit 0 the
/// leftmost dot, 1 lit.
pub struct CharRom {
    glyphs: Vec<[u8; 8]>,
}

impl CharRom {
    /// A dump of the Apple IIe video ROM (342-0133 or 342-0265: 4 KB, the first 2 KB the
    /// primary set) or a 2 KB character ROM. Dumps differ in dot order and polarity, so
    /// both are found from the glyphs themselves: the normal space ($A0) has no dots
    /// lit, and the normal L ($CC) has only its leftmost column lit in rows 1-5.
    pub fn from_rom(rom: &[u8]) -> Result<Self, String> {
        if rom.len() < 2048 {
            return Err(format!("a character ROM is at least 2048 bytes, not {}", rom.len()));
        }
        let raw = |code: usize, row: usize| rom[code * 8 + row];
        let space: Vec<u8> = (0..8).map(|r| raw(0xA0, r)).collect();
        let lit_is_zero = if space.iter().all(|&b| b & 0x7F == 0) {
            false
        } else if space.iter().all(|&b| b & 0x7F == 0x7F) {
            true
        } else {
            return Err("not an Apple character ROM: the space ($A0) is not blank".into());
        };
        // Seven dots in bits 0-6; bit 7 is unused (or a flash flag).
        let fix = |b: u8| (if lit_is_zero { !b } else { b }) & 0x7F;
        // Which way the dots run: the L's upright stands at the left end of its foot.
        // The upright is the row with one dot lit, the foot the row with the most.
        let l: Vec<u8> = (0..8).map(|r| fix(raw(0xCC, r))).collect();
        let upright =
            l.iter().copied().find(|b| b.count_ones() == 1).ok_or("not an Apple character ROM: no L at $CC")?;
        let foot = l.iter().copied().max_by_key(|b| b.count_ones()).unwrap_or(0);
        if foot.count_ones() < 3 || foot & upright == 0 {
            return Err("not an Apple character ROM: no L at $CC".into());
        }
        let low_first = upright.trailing_zeros() == foot.trailing_zeros();
        if !low_first && 7 - upright.leading_zeros() != 7 - foot.leading_zeros() {
            return Err("not an Apple character ROM: no L at $CC".into());
        }
        // Dot d (0 = left) is bit d, or bit 6 - d when the dots run from the high end.
        let dot = |b: u8, d: u8| if low_first { b >> d & 1 } else { b >> (6 - d) & 1 };
        let glyphs = (0..256)
            .map(|code| {
                let mut g = [0u8; 8];
                for (r, row) in g.iter_mut().enumerate() {
                    let b = fix(raw(code, r));
                    *row = (0..7).fold(0, |acc, d| acc | dot(b, d as u8) << d);
                }
                g
            })
            .collect();
        Ok(CharRom { glyphs })
    }

    /// Row `row` of character `c` (ASCII) as 7 dots (bit 0 left, 1 = lit), from the
    /// normal glyphs ($80-$FF), which every dump has; inverse is drawn by the caller.
    pub fn row(&self, c: u8, _inverse: bool, row: usize) -> u8 {
        self.glyphs[(c | 0x80) as usize][row]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntsc_keeps_solid_colours_and_fringes_edges() {
        // A line of dark blue (value 8) then white.
        let mut fb = vec![0u32; FRAME_W];
        for (i, p) in fb.iter_mut().enumerate() {
            *p = if i < 280 { rgb(8) } else { rgb(0x0F) };
        }
        let before = fb.clone();
        ntsc(&mut fb);
        assert_eq!(fb[100], before[100]);
        assert_eq!(fb[400], before[400]);
        assert!((276..284).any(|i| fb[i] != before[i]), "a fringe at the edge");
    }

    /// An L with a blank first column, as real ROMs have: upright at dot 1, foot dots 1-5.
    fn l_rows(dot_bit: impl Fn(u8) -> u8) -> [u8; 8] {
        let mut rows = [0u8; 8];
        for row in rows.iter_mut().take(6) {
            *row = dot_bit(1);
        }
        rows[6] = (1..=5).fold(0, |a, d| a | dot_bit(d));
        rows
    }

    #[test]
    fn char_rom_orientation_is_detected() {
        // The IIe layout (342-0265-A): bit 0 is the left dot, normal glyphs stored
        // inverted (0 = lit), bit 7 set.
        let mut rom = vec![0xFFu8; 4096];
        for (r, b) in l_rows(|d| 1 << d).iter().enumerate() {
            rom[0xCC * 8 + r] = !b;
        }
        let c = CharRom::from_rom(&rom).unwrap();
        assert_eq!(c.row(b'L', false, 2), 0b10);
        assert_eq!(c.row(b'L', false, 6), 0b111110);
        assert_eq!(c.row(b' ', false, 3), 0);
        // The II+ layout (341-0036): bit 6 is the left dot, 1 = lit, bit 7 a flag.
        let mut rom2 = vec![0u8; 2048];
        for (r, b) in l_rows(|d| 1 << (6 - d)).iter().enumerate() {
            rom2[0xCC * 8 + r] = b | 0x80;
        }
        let c2 = CharRom::from_rom(&rom2).unwrap();
        assert_eq!(c2.row(b'L', false, 2), 0b10);
        assert_eq!(c2.row(b'L', false, 6), 0b111110);
    }

    /// The real ROM, if the user has put one in original/ (the port ships none).
    #[test]
    fn real_iie_rom_reads_as_expected() {
        let p =
            concat!(env!("CARGO_MANIFEST_DIR"), "/original/Apple IIe Enhanced Video ROM - 342-0265-A - US 1983.bin");
        let Ok(rom) = std::fs::read(p) else { return };
        let c = CharRom::from_rom(&rom).unwrap();
        // L: upright in the second column, foot across five.
        assert_eq!(c.row(b'L', false, 0), 0b10);
        assert_eq!(c.row(b'L', false, 6), 0b111110);
        // A lower-case a, and the normal glyphs match the ROM's own inverse ones.
        assert_eq!(c.row(b'a', false, 2), 0b11100);
        for ch in *b"AW1?" {
            for r in 0..8 {
                let inverse_code = (ch & 0x3F) as usize; // $00-$3F hold the inverse set
                assert_eq!(c.glyphs[inverse_code][r], !c.row(ch, false, r) & 0x7F, "{} row {r}", ch as char);
            }
        }
    }

    #[test]
    fn specks_take_their_surroundings() {
        let mut base = vec![0x14u8; WIDTH * HEIGHT];
        base[10 * WIDTH + 10] = 0x1F;
        assert_eq!(speck(&base, &base, 10, 10), Some(4));
        assert_eq!(speck(&base, &base, 11, 10), None);
        // Two greens beside it outvote one other colour.
        base[10 * WIDTH + 11] = 0x16;
        assert_eq!(speck(&base, &base, 10, 10), Some(4));
        // A white neighbour: part of a white area, left alone.
        base[9 * WIDTH + 10] = 0x2F;
        assert_eq!(speck(&base, &base, 10, 10), None);
    }
}
