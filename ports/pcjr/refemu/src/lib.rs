//! refemu-pcjr as a library: the 8086 core, the PCjr, and a runner that stops at each
//! pass, for the tools built on it (`kqroute-pcjr`).

/// The 8086 core, shared with the other reference machine (crates/kq1-x86).
pub use kq1_x86 as cpu;
pub mod machine;
pub mod runner;

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
