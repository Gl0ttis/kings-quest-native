//! The engines' clock: when the timer interrupt and the keys come, against the
//! checkpoints both the original (in the port's refemu) and the port count. Each port's
//! `engine::clock` module has its checkpoints and the sites where a tick that
//! interrupted some code fires (and re-exports this).

use std::collections::VecDeque;

/// Something that happens to the machine from outside the game's code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The timer interrupt, and the code-segment address it interrupted (FFFF outside
    /// the game's code).
    Tick(u16),
    /// A key reaches the BIOS keyboard buffer (scan << 8 | character).
    Key(u16),
    /// A disk is put in the drive (0 the Master, 1 the Play diskette).
    Disk(u8),
}

/// What fires at a checkpoint.
pub trait Clock {
    /// Checkpoint number `n` (counting from 1) has been reached, `cycles` estimated CPU
    /// cycles into the run: the events of the interval it starts, in order.
    fn at(&mut self, n: u64, cycles: u64) -> Vec<Event>;
    /// In a busy wait whose interval's events are spent: more, or none if the clock
    /// has nothing more to give (a replay that has diverged).
    fn waiting(&mut self, n: u64, cycles: u64) -> Vec<Event>;
    /// `cycles` reached, between checkpoints: what is due now. Asked in a timed delay
    /// that polls the keyboard (the title's), and before the draw pass sets its flag. A
    /// replay gives nothing: its events come at the checkpoints (and sites), as
    /// refemu logged them. A clock for play gives the keys typed and the ticks.
    fn poll(&mut self, _cycles: u64) -> Vec<Event> {
        Vec::new()
    }
    /// The joystick, when the timer reads it: None when none is connected (the read
    /// times out), as with the original in refemu, which has none.
    fn joystick(&mut self) -> Option<Joystick> {
        None
    }
    /// The speed model has counted `cycles` of loading in all (disk sectors and
    /// picture drawing; part of the cycles passed to [`Clock::at`]). A clock for play
    /// can skip their time (fast loads).
    fn loading(&mut self, _cycles: u64) {}
    /// Between checkpoints, `cycles` reached (a picture shown as it is drawn): a clock
    /// for play waits for its time. No events come: those of the interval come at its
    /// checkpoints, as always.
    fn pace(&mut self, _cycles: u64) {}
}

/// The joystick as the game's read finds it: for each axis, the turns of its loop until
/// the axis's one-shot ended (more with the stick further right or down); whether each
/// button is down. No joystick is none. What the turns mean is each engine's (the PCjr's
/// compares fixed counts, `$0563`; the PC's compares them with its centre's, taken at
/// its first read, `$141B`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Joystick {
    pub x: u8,
    pub y: u8,
    pub buttons: [bool; 2],
}

impl Joystick {
    /// The turns for a stick at rest.
    pub const CENTRE: u8 = 12;
    /// From positions 0.0-1.0 across each axis (0.5 at rest): the turns grow with the
    /// stick's resistance, 25 across, so that a quarter of the way from the middle
    /// either way the direction changes.
    pub fn from_position(x: f64, y: f64, buttons: [bool; 2]) -> Joystick {
        let turns = |p: f64| (p.clamp(0.0, 1.0) * 25.0).round() as u8;
        Joystick { x: turns(x), y: turns(y), buttons }
    }
}

/// Events logged by the port's refemu (`--events`), replayed at the same checkpoints.
pub struct Replay {
    events: VecDeque<(u64, Event)>,
}

impl Replay {
    /// Parse a log: one event a line, `T n ip`, `K n hex` or `D n disk`; other lines
    /// (comments, pass markers, checkpoints) are skipped.
    pub fn parse(log: &str) -> Replay {
        let events = log
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let kind = f.next()?;
                let n: u64 = f.next()?.parse().ok()?;
                let e = match kind {
                    "T" => Event::Tick(f.next().and_then(|h| u16::from_str_radix(h, 16).ok()).unwrap_or(0xFFFF)),
                    "K" => Event::Key(u16::from_str_radix(f.next()?, 16).ok()?),
                    "D" => Event::Disk(f.next()?.parse().ok()?),
                    _ => return None,
                };
                Some((n, e))
            })
            .collect();
        Replay { events }
    }

    /// Events not yet replayed.
    pub fn remaining(&self) -> usize {
        self.events.len()
    }
}

impl Clock for Replay {
    fn at(&mut self, n: u64, _cycles: u64) -> Vec<Event> {
        // Events logged at an earlier checkpoint than this one were missed: the port
        // passed fewer checkpoints than the original. They come now, late.
        let mut out = Vec::new();
        while let Some(&(m, e)) = self.events.front() {
            if m > n {
                break;
            }
            out.push(e);
            self.events.pop_front();
        }
        out
    }

    fn waiting(&mut self, _n: u64, _cycles: u64) -> Vec<Event> {
        Vec::new()
    }
}

/// CPU cycles between timer ticks: 4,772,727 Hz / 18.2065 Hz (65,536 × 4).
pub const CYCLES_PER_TICK: u64 = 262_144;
/// The 8088's clock (Hz), and its cycles in 1/60 s (a front end's frame).
pub const CPU_HZ: f64 = 4_772_727.0;
pub const CYCLES_PER_FRAME: u64 = 79_545;

/// Ticks from the port's own estimate of the original's speed, with keys and disk
/// swaps given by the front end.
#[derive(Default)]
pub struct Paced {
    /// Ticks fired so far.
    pub ticks: u64,
    /// Keys and swaps waiting to be delivered at the next checkpoint.
    pub pending: VecDeque<Event>,
}

impl Clock for Paced {
    fn at(&mut self, _n: u64, cycles: u64) -> Vec<Event> {
        let mut out: Vec<Event> = self.pending.drain(..).collect();
        while (self.ticks + 1) * CYCLES_PER_TICK <= cycles {
            self.ticks += 1;
            out.push(Event::Tick(0xFFFF));
        }
        out
    }

    fn waiting(&mut self, _n: u64, _cycles: u64) -> Vec<Event> {
        self.ticks += 1;
        let mut out: Vec<Event> = self.pending.drain(..).collect();
        out.push(Event::Tick(0xFFFF));
        out
    }

    fn poll(&mut self, cycles: u64) -> Vec<Event> {
        self.at(0, cycles)
    }
}
