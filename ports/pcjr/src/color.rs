//! Colours. The PCjr engine draws in BIOS mode 9 and leaves the palette registers at
//! their defaults (it never sets them), so colour numbers 0-15 are the standard IBM
//! colours. Pictures and views both use these numbers; the Apple version maps
//! pictures through a table at draw time (ports/apple2/src/pic.rs, `COLOR_TABLE`) and
//! stored its views already converted.

/// Picture colour index -> stored value: unchanged on the PCjr.
pub const IDENTITY: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

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
