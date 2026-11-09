//! The scripts: the Apple version's layout (ports/apple2/src/script.rs), with the
//! PCjr's operand counts (ports/pcjr/src/script.rs), with which every script parses.
//! (The PC engine's own table of handlers is still to read.)

pub const OPERANDS: [u8; 0x44] = {
    let mut t = kq1_formats::script::OPERANDS;
    t[0x05] = 1;
    t[0x25] = 1;
    t[0x31] = 0;
    t
};
