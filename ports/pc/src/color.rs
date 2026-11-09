//! Colours. The pictures and views are the PCjr's, in the 16 IBM colour numbers. The
//! PC shows them in CGA's 640-dot mode with the colour burst on, where each 4-dot
//! group is one colour on a composite monitor; how the engine maps the colour numbers
//! to those groups, and the colours a composite monitor shows, are still to find.
//! Until then the tools show the IBM colours, as the PCjr does.

/// The 16 IBM colours (RGBI; colour 6 is brown, as IBM monitors show it).
pub const PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], // 0 black
    [0x00, 0x00, 0xAA], // 1 blue
    [0x00, 0xAA, 0x00], // 2 green
    [0x00, 0xAA, 0xAA], // 3 cyan
    [0xAA, 0x00, 0x00], // 4 red
    [0xAA, 0x00, 0xAA], // 5 magenta
    [0xAA, 0x55, 0x00], // 6 brown
    [0xAA, 0xAA, 0xAA], // 7 light grey
    [0x55, 0x55, 0x55], // 8 dark grey
    [0x55, 0x55, 0xFF], // 9 light blue
    [0x55, 0xFF, 0x55], // 10 light green
    [0x55, 0xFF, 0xFF], // 11 light cyan
    [0xFF, 0x55, 0x55], // 12 light red
    [0xFF, 0x55, 0xFF], // 13 light magenta
    [0xFF, 0xFF, 0x55], // 14 yellow
    [0xFF, 0xFF, 0xFF], // 15 white
];

pub fn rgb(c: u8) -> [u8; 3] {
    PALETTE[(c & 0x0F) as usize]
}
