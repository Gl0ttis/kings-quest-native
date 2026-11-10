//! refemu-pc as a library: the 8086 core, the IBM PC with the CGA, and a runner that
//! stops at each pass, for the tools built on it (`kqroute-pc`).

pub mod cpu;
pub mod machine;
pub mod runner;

/// The user's BIOS image for the font: `bios.rom` in the disk image's folder (as
/// 86Box's IBM PC ROM, `roms/machines/ibmpc82/BIOS_5150_27OCT82_1501476_U33.BIN`, put
/// there under that name). Never part of the repository.
pub fn bios_next_to(disk: &str) -> Option<Vec<u8>> {
    let dir = std::path::Path::new(disk).parent()?;
    std::fs::read(dir.join("bios.rom")).ok()
}

/// The speaker's events in order, from the machine's writes to its ports (`Machine::sound`):
/// `D xxxx` when the timer's channel 2 gets a divisor (mode byte B6h to port 43h, then
/// its low and high bytes to 42h), `G 1` / `G 0` at each write to port 61h, its two low
/// bits both set or not. One a line, numbers only (reference/*.speaker).
pub fn speaker_log(sound: &[(u64, u16, u8)]) -> String {
    let mut out = String::new();
    let mut lo: Option<u8> = None;
    for &(_, port, v) in sound {
        match port {
            0x43 => lo = None,
            0x42 => match lo.take() {
                None => lo = Some(v),
                Some(l) => {
                    out += &format!(
                        "D {:04X}
",
                        u16::from_le_bytes([l, v])
                    )
                }
            },
            0x61 => {
                out += &format!(
                    "G {}
",
                    (v & 3 == 3) as u8
                )
            }
            _ => {}
        }
    }
    out
}
