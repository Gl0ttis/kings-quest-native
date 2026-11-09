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
