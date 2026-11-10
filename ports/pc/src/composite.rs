//! Composite colour: what a composite monitor (or a TV) shows of the CGA's 640-dot
//! mode with the colour burst on, the PC release's display (docs/disk-layout.md, "The
//! display").
//!
//! **The signal.** The engine puts only two colours on screen: 0 (black) and the
//! foreground it sets, 7 (`3D9h` = `27h`). On the CGA both are colours without chroma,
//! so the composite signal is two levels, one per dot, at 14.318 MHz: four dots to a
//! cycle of the 3.58 MHz colour subcarrier. The colours are the monitor's decoding of
//! the dot pattern: the dots' average is the brightness, and their pattern against the
//! subcarrier's phase the hue and saturation. (Because the signal has only two levels,
//! the CGA's revision, whose composite output differs for the other colours, changes
//! only its overall level here, which the monitor's knobs set.)
//!
//! **The two modes** decode the same signal:
//! - [`Mode::Groups`]: each 4-dot group as one colour, the group's pattern decoded on its
//!   own (the 16 colours the game was drawn for);
//! - [`Mode::Signal`]: every dot decoded with the dots round it, as the monitor does:
//!   the brightness over one subcarrier cycle (4 dots), the colour over three (12
//!   dots: a monitor's colour has a narrower bandwidth than its brightness, so thin
//!   lines, the text's strokes, show mostly as brightness, and colour spreads across
//!   edges). The same colours inside areas of one colour, and the fringes a monitor
//!   shows where they meet.
//!
//! **Calibration.** Three numbers, measured from the original running in 86Box (an IBM
//! PC with the CGA on a composite monitor, docs/port-plan.md): the subcarrier's phase
//! against the dots (the hue), the saturation, and the foreground's level, colour 7's
//! against the white of colour 15 (so the game is darker than white text would be).
//! Fitted to 86Box's colours of room 1, they give them within 6/255 (RMS). The engine's
//! table at DS:`$12BA` gives the pattern it draws for each PCjr colour number
//! ([`PATTERN`]).

/// The engine's colour translation (DS:`$12BA`): PCjr colour number → the 4-dot pattern
/// it draws, leftmost dot in the high bit. Dark grey (8) is drawn black, and magenta (5)
/// as `1010`, which shows as grey.
pub const PATTERN: [u8; 16] = [0x0, 0x2, 0x1, 0x9, 0x4, 0xA, 0x8, 0x5, 0x0, 0x3, 0xB, 0x7, 0xE, 0x6, 0xD, 0xF];

/// Where the table is in the engine's data segment.
pub const PATTERN_TABLE: usize = 0x12BA;

/// The subcarrier's phase at the first dot of a line, in degrees (the hue; measured).
pub const HUE: f64 = 322.5;
/// The saturation (measured).
pub const SATURATION: f64 = 0.58;
/// The level of a dot in colour 7, the engine's foreground, as a fraction of white
/// (measured).
pub const LEVEL: f64 = 0.62;

/// How the dots are turned into colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Each 4-dot group in its own colour.
    #[default]
    Groups,
    /// The composite signal decoded dot by dot, with the fringes between colours.
    Signal,
}

/// The colour components of `dots` (each 0 or 1, a whole number of subcarrier
/// cycles), whose first is at subcarrier position `first` (0-3).
fn chroma(dots: &[u8], first: usize) -> (f64, f64) {
    let cycles = (dots.len() / 4) as f64;
    let (mut i, mut q) = (0.0, 0.0);
    for (k, &d) in dots.iter().enumerate() {
        let d = d as f64 * LEVEL;
        let a = HUE.to_radians() + std::f64::consts::FRAC_PI_2 * ((first + k) % 4) as f64;
        i += d * a.cos() / 2.0 / cycles;
        q += d * a.sin() / 2.0 / cycles;
    }
    (i * SATURATION, q * SATURATION)
}

/// A colour from its brightness `y` and colour components.
fn rgb(y: f64, (i, q): (f64, f64)) -> [u8; 3] {
    let to = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [to(y + 0.956 * i + 0.621 * q), to(y - 0.272 * i - 0.647 * q), to(y - 1.106 * i + 1.703 * q)]
}

/// Decode one cycle of `dots`, whose first is at subcarrier position `first` (0-3).
fn decode(dots: [u8; 4], first: usize) -> [u8; 3] {
    let y = dots.iter().map(|&d| d as f64 * LEVEL).sum::<f64>() / 4.0;
    rgb(y, chroma(&dots, first))
}

fn bits(nibble: u8) -> [u8; 4] {
    [nibble >> 3 & 1, nibble >> 2 & 1, nibble >> 1 & 1, nibble & 1]
}

/// The colour of a 4-dot group pattern, repeated.
pub fn group(pattern: u8) -> [u8; 3] {
    decode(bits(pattern), 0)
}

/// The 16 group colours, by pattern.
pub fn palette() -> [[u8; 3]; 16] {
    std::array::from_fn(|p| group(p as u8))
}

/// One line of dots (each 0 or 1, from the line's first dot) as colours, one per dot.
/// The border beyond the line is black.
pub fn line(dots: &[u8], mode: Mode) -> Vec<[u8; 3]> {
    match mode {
        Mode::Groups => {
            let colours = palette();
            dots.chunks(4)
                .flat_map(|g| {
                    let p = g.iter().fold(0u8, |v, &d| v << 1 | d) << (4 - g.len());
                    std::iter::repeat_n(colours[p as usize], g.len())
                })
                .collect()
        }
        Mode::Signal => {
            let at = |n: isize| if n < 0 { 0 } else { dots.get(n as usize).copied().unwrap_or(0) };
            (0..dots.len() as isize)
                .map(|n| {
                    // The brightness over the cycle from two dots before this one to one
                    // after it; the colour over three cycles round it.
                    let y = (n - 2..n + 2).map(|k| at(k) as f64 * LEVEL).sum::<f64>() / 4.0;
                    let w: Vec<u8> = (n - 6..n + 6).map(at).collect();
                    rgb(y, chroma(&w, (n - 6).rem_euclid(4) as usize))
                })
                .collect()
        }
    }
}

/// A picture row of PCjr colour numbers (each pixel 4 dots, through [`PATTERN`]) as
/// colours, 4 to a pixel.
pub fn picture_row(colours: &[u8], mode: Mode) -> Vec<[u8; 3]> {
    let dots: Vec<u8> = colours.iter().flat_map(|&c| bits(PATTERN[(c & 15) as usize])).collect();
    line(&dots, mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hues of the one-dot patterns run green, blue, red, brown, a quarter turn
    /// apart, as on composite CGA, and the two-dot ones in between.
    #[test]
    fn the_group_colours() {
        let p = palette();
        assert_eq!(p[0x0], [0, 0, 0]);
        // All dots on: colour 7's grey.
        assert!(p[0xF].iter().all(|&v| v.abs_diff(158) <= 1), "{:?}", p[0xF]);
        // The two greys: patterns at twice the subcarrier's frequency have no colour.
        for c in [p[0x5], p[0xA]] {
            assert!(c.iter().all(|&v| v.abs_diff(79) <= 1), "{c:?}");
        }
        let dominant = |c: [u8; 3]| (0..3).max_by_key(|&k| c[k]).unwrap();
        // The PCjr's blue, green and red, as the engine draws them.
        assert_eq!(dominant(group(PATTERN[1])), 2);
        assert_eq!(dominant(group(PATTERN[2])), 1);
        assert_eq!(dominant(group(PATTERN[4])), 0);
        assert_eq!(dominant(group(PATTERN[9])), 2); // light blue
        assert_eq!(dominant(group(PATTERN[10])), 1); // light green
        assert_eq!(dominant(group(PATTERN[12])), 0); // light red
    }

    /// Inside an area of one pattern the signal's decoding is the group's colour; the
    /// two modes differ only at the edges.
    #[test]
    fn the_signal_inside_an_area_is_the_group_colour() {
        for c in 0..16u8 {
            let row = vec![c; 40];
            let (g, s) = (picture_row(&row, Mode::Groups), picture_row(&row, Mode::Signal));
            assert_eq!(&g[8..152], &s[8..152], "colour {c}");
        }
        let mut row = vec![1u8; 20];
        row.extend([14u8; 20]);
        let (g, s) = (picture_row(&row, Mode::Groups), picture_row(&row, Mode::Signal));
        assert_ne!(g[78..82], s[78..82]);
    }
}
