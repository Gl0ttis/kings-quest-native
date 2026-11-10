//! refemu-pc as a library: the 8086 core, the IBM PC with the CGA, and a runner that
//! stops at each pass, for the tools built on it (`kqroute-pc`).

/// The 8086 core, shared with the other reference machine (crates/kq1-x86).
pub use kq1_x86 as cpu;
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

/// The autopilot's view of the original (crates/kq1-autopilot, `kqroute`).
impl kq1_autopilot::Original for runner::Runner {
    fn ds(&self, off: u16) -> u8 {
        runner::Runner::ds(self, off)
    }
    fn ds_w(&self, off: u16) -> u16 {
        runner::Runner::ds_w(self, off)
    }
    fn v(&self, n: u8) -> u8 {
        runner::Runner::v(self, n)
    }
    fn priority(&self, x: usize, y: usize) -> u8 {
        runner::Runner::priority(self, x, y)
    }
    fn press(&mut self, k: u16) {
        runner::Runner::press(self, k)
    }
    fn bios_keys(&self) -> &std::collections::VecDeque<u16> {
        &self.m.keys
    }
    fn passes(&self) -> u64 {
        self.passes
    }
    fn run(&mut self, prompt_wait: u64, max_cycles: u64) -> Result<kq1_autopilot::Stopped, String> {
        runner::Runner::run(self, prompt_wait, max_cycles).map(stopped)
    }
    fn follow(&mut self, log: &str) -> Result<kq1_autopilot::Stopped, String> {
        runner::Runner::follow(self, log).map(stopped)
    }
    fn events(&self) -> &str {
        runner::Runner::events(self)
    }
    fn trace(&self) -> &str {
        &self.trace
    }
}

fn stopped(s: runner::Stopped) -> kq1_autopilot::Stopped {
    match s {
        runner::Stopped::Pass => kq1_autopilot::Stopped::Pass,
        runner::Stopped::Prompt => kq1_autopilot::Stopped::Prompt,
    }
}
