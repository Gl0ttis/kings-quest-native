//! Running the original a pass at a time, for tools that decide what to press as it goes
//! (the route autopilot, `kqroute-pcjr`). It runs as refemu-pcjr's main loop does: the
//! checkpoints counted, the event log and the trace written, the disk swaps answered
//! and RETURN pressed at the title. It stops at the start of each pass, and at a
//! prompt that waits for keys outside the game cycle (the save dialog).

use crate::cpu::{self, Cpu, CS};
use crate::machine::{Disk, Machine};
use std::rc::Rc;

/// The start of a pass: `$14F2`, the game cycle.
const MAIN_LOOP: (u16, u16) = (0x0060, 0x14F2);
/// The engine's data segment.
const DATA_SEG: u16 = 0x0543;

/// Why the run stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum Stopped {
    /// At the start of a pass.
    Pass,
    /// At a prompt, waiting for keys.
    Prompt,
}

#[derive(Clone)]
pub struct Runner {
    pub m: Machine,
    pub cpu: Cpu,
    checkpoints: Rc<Vec<bool>>,
    /// Passes started.
    pub passes: u64,
    /// The trace, refemu-pcjr's columns.
    pub trace: String,
    last_pass_cycle: Option<u64>,
    wait_since: Option<u64>,
    /// The instruction at the current position was counted already (the run stopped
    /// there).
    resume: bool,
    /// Replaying an event log: the keys and disk swaps to make at each checkpoint (by
    /// number), instead of answering the prompts.
    replay: Option<std::collections::VecDeque<(u64, Replayed)>>,
    /// Print the checkpoints numbered FROM-TO as they pass (KQ_CPS=FROM-TO).
    cps: Option<(u64, u64)>,
    /// When Some, the cycles spent at each 16-byte block of the engine's code
    /// (CS 0060h; the last bucket for any other code), added up until taken.
    pub profile: Option<Vec<u64>>,
}

/// A key press or a disk swap from an event log.
#[derive(Clone, Copy, Debug)]
pub enum Replayed {
    Key(u16),
    Disk(usize),
}

impl Runner {
    /// Power on with the original (an `.mfm` dump or the `.img`) and a blank Play
    /// diskette.
    pub fn new(raw: &[u8]) -> Result<Runner, String> {
        let mut disk = if raw.starts_with(b"HXCMFM\0") {
            Disk::from_tracks(&kq1_formats::mfm::decode(raw)?)
        } else {
            let mut d = Disk::default();
            for (i, s) in raw.chunks(512).enumerate() {
                d.sectors.insert(((i / 18) as u8, (i / 9 % 2) as u8, (i % 9 + 1) as u8, 2), s.to_vec());
            }
            d
        };
        disk.write_protected = true;
        let mut m = Machine::new(disk);
        m.disks.push(Disk::default());
        m.events = Some(String::new());
        let mut cpu = Cpu::new();
        m.boot(&mut cpu)?;
        let mut checkpoints = vec![false; 0x10000];
        for &a in &kq1_pcjr::engine::clock::CHECKPOINTS {
            checkpoints[a as usize] = true;
        }
        Ok(Runner {
            m,
            cpu,
            checkpoints: Rc::new(checkpoints),
            passes: 0,
            trace: String::from("pass,cycles,delta,room,ego_x,ego_y,ego_dir,score,vars_hash\n"),
            last_pass_cycle: None,
            wait_since: None,
            resume: false,
            replay: None,
            profile: None,
            cps: std::env::var("KQ_CPS").ok().and_then(|v| {
                let (a, b) = v.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            }),
        })
    }

    /// Replay refemu-pcjr's event log (`--events`): its keys (`K n key`) and disk swaps
    /// (`D n disk`) are made at the checkpoints they were made at; the timer ticks follow.
    pub fn replay(&mut self, events: &str) {
        let q = events
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let kind = f.next()?;
                let n: u64 = f.next()?.parse().ok()?;
                let v = f.next()?;
                match kind {
                    "K" => Some((n, Replayed::Key(u16::from_str_radix(v, 16).ok()?))),
                    "D" => Some((n, Replayed::Disk(v.parse().ok()?))),
                    _ => None,
                }
            })
            .collect();
        self.replay = Some(q);
    }

    /// Follow a log this tool wrote (kqroute-pcjr `--events`) exactly: type the keys
    /// that come next in it (those typed where the run now stands), then run to the next
    /// stop (a pass, a prompt), as kqroute-pcjr ran. Stopping where it stopped and typing
    /// there repeats its timing, which pressing the keys at their checkpoints doesn't
    /// (a prompt's key would come early). Errors if the run leaves the log.
    pub fn follow(&mut self, log: &str) -> Result<Stopped, String> {
        if !log.as_bytes().starts_with(self.events().as_bytes()) {
            return Err(format!("the run left the log at pass {}", self.passes));
        }
        let mut p = self.events().len();
        while p < log.len() {
            let end = log[p..].find('\n').map_or(log.len(), |i| p + i + 1);
            let mut f = log[p..end].split_whitespace();
            match (f.next(), f.nth(1)) {
                (Some("K"), Some(k)) => self.press(u16::from_str_radix(k, 16).map_err(|e| e.to_string())?),
                _ => break,
            }
            p = end;
        }
        self.run(1_500_000, 200_000_000)
    }

    /// The screen as the player sees it, as the port's `World::frame` gives it: 1 and
    /// the 40 × 25 characters when a text mode is up; else 0, the displayed page's
    /// picture area (160 bytes × 168 rows) and the characters.
    pub fn frame(&self) -> Vec<u8> {
        let text_mode = !matches!(self.m.video_mode, 9 | 10);
        let mut f = Vec::with_capacity(1 + 160 * 168 + 1000);
        f.push(text_mode as u8);
        if !text_mode {
            let base = self.m.crt_page as usize * 0x4000;
            for y in 0..168 {
                let row = base + (y & 3) * 0x2000 + (y >> 2) * 160;
                f.extend_from_slice(&self.m.ram[row..row + 160]);
            }
        }
        f.extend(self.m.text.iter().flatten());
        f
    }

    /// Stop replaying: the rest of the log is dropped.
    pub fn end_replay(&mut self) {
        self.replay = None;
    }

    /// A byte of the engine's data segment.
    pub fn ds(&self, off: u16) -> u8 {
        self.m.ram[cpu::lin(DATA_SEG, off) as usize]
    }
    pub fn ds_w(&self, off: u16) -> u16 {
        self.ds(off) as u16 | (self.ds(off.wrapping_add(1)) as u16) << 8
    }
    /// Variable `n`.
    pub fn v(&self, n: u8) -> u8 {
        self.ds(0x1003 + n as u16)
    }
    /// The priority at (x, y) in the work page (segment 1000h, low nibble).
    pub fn priority(&self, x: usize, y: usize) -> u8 {
        self.m.ram[0x10000 + (y & 3) * 0x2000 + (y >> 2) * 160 + x] & 0x0F
    }

    /// A key into the BIOS buffer now (logged).
    pub fn press(&mut self, k: u16) {
        self.m.push_key(k);
    }

    /// Run to the start of the next pass, or to a prompt waiting for keys (after
    /// `prompt_wait` cycles idle), within `max_cycles`.
    pub fn run(&mut self, prompt_wait: u64, max_cycles: u64) -> Result<Stopped, String> {
        let limit = self.cpu.cycles + max_cycles;
        loop {
            if self.cpu.cycles > limit {
                return Err(format!("no pass for {max_cycles} cycles at {:04X}:{:04X}", self.cpu.s[CS], self.cpu.ip));
            }
            if let Some(f) = &self.cpu.fault {
                return Err(f.clone());
            }
            let resumed = std::mem::take(&mut self.resume);
            let at_checkpoint = self.cpu.s[CS] == 0x60 && self.checkpoints[self.cpu.ip as usize];
            if at_checkpoint && !resumed {
                self.m.checkpoint += 1;
                if self.cps.is_some_and(|(a, b)| (a..=b).contains(&self.m.checkpoint)) {
                    eprintln!("checkpoint {} at {:04X}, v25 {}", self.m.checkpoint, self.cpu.ip, self.v(25));
                }
            }
            if let Some(q) = self.replay.as_mut() {
                // Keys pressed before the run started (the title's) count as at 0.
                while q.front().is_some_and(|e| e.0 <= self.m.checkpoint) {
                    match q.pop_front().unwrap().1 {
                        Replayed::Key(k) => self.m.push_key(k),
                        Replayed::Disk(d) => {
                            self.m.insert(d);
                            self.m.blocked_reads = 0;
                        }
                    }
                }
            }
            if (self.cpu.s[CS], self.cpu.ip) == MAIN_LOOP && !resumed {
                self.passes += 1;
                let n = self.passes;
                if let Some(e) = self.m.events.as_mut() {
                    e.push_str(&format!("P {} {n}\n", self.m.checkpoint));
                }
                self.wait_since = None;
                self.m.idle_polls = 0;
                let delta = self.last_pass_cycle.map_or(0, |l| self.cpu.cycles - l);
                self.last_pass_cycle = Some(self.cpu.cycles);
                let vars: u64 = (0..0xDFu16)
                    .map(|v| self.ds(0x1003 + v) as u64)
                    .fold(1469598103934665603u64, |h, b| (h ^ b).wrapping_mul(1099511628211));
                self.trace += &format!(
                    "{n},{},{delta},{},{},{},{},{},{vars:016x}\n",
                    self.cpu.cycles,
                    self.v(1),
                    self.ds(0x11F9),
                    self.ds(0x11FA),
                    self.ds(0x11F0),
                    self.v(19)
                );
                self.resume = true;
                return Ok(Stopped::Pass);
            }
            // Waiting for a key outside the game cycle: the disk swaps are answered as
            // refemu-pcjr answers them; RETURN at the title; otherwise the caller types.
            if self.replay.is_none()
                && self.m.idle_polls > 0
                && self.m.keys.is_empty()
                && (at_checkpoint || self.m.blocked_reads > 0 || self.passes == 0)
            {
                let since = *self.wait_since.get_or_insert(self.cpu.cycles);
                let waited = self.cpu.cycles - since;
                let screen: String = self.m.text.iter().map(|r| String::from_utf8_lossy(r).to_string()).collect();
                let swap = if screen.contains("insert your   Play") {
                    Some(1)
                } else if screen.contains("insert your  Master") {
                    Some(0)
                } else {
                    None
                };
                if let Some(d) = swap.filter(|_| self.m.blocked_reads > 0) {
                    self.m.insert(d);
                    self.m.push_key(0x1C0D);
                    self.wait_since = None;
                    self.m.idle_polls = 0;
                    self.m.blocked_reads = 0;
                    self.m.step(&mut self.cpu);
                    continue;
                }
                if self.passes == 0 && waited > 3_000_000 {
                    self.m.push_key(0x1C0D);
                    self.wait_since = None;
                    self.m.idle_polls = 0;
                    self.m.blocked_reads = 0;
                } else if self.passes > 0 && waited > prompt_wait && (self.m.blocked_reads > 0 || at_checkpoint) {
                    self.wait_since = None;
                    self.m.idle_polls = 0;
                    self.resume = at_checkpoint;
                    return Ok(Stopped::Prompt);
                }
            }
            if let Some(p) = self.profile.as_mut() {
                let (cs, ip, before) = (self.cpu.s[CS], self.cpu.ip, self.cpu.cycles);
                self.m.step(&mut self.cpu);
                let b = if cs == 0x60 { ip as usize >> 4 } else { 0x1000 };
                p[b] += self.cpu.cycles - before;
            } else {
                self.m.step(&mut self.cpu);
            }
        }
    }

    /// The event log so far.
    pub fn events(&self) -> &str {
        self.m.events.as_deref().unwrap_or("")
    }
}
