//! When the timer interrupt fires.
//!
//! On the PCjr the timer interrupt (INT 08h, chained to the game's INT 1Ch handler
//! `$1A18`) comes 18.2 times a second, between any two instructions. The handler reads
//! the keys into the game's key ring, prints queued messages a word at a time and plays
//! the music, so the game's state depends on where in its code each tick falls.
//!
//! **Checkpoints.** Both sides count the checkpoints passed, in order: the routine
//! entries and loop heads in [`CHECKPOINTS`]. refemu-pcjr (`--events`) logs each tick,
//! key press and disk swap with the count at that moment, and for a tick the address
//! it interrupted.
//!
//! **Where in an interval.** Between two checkpoints, the port fires a tick:
//! - at a disk read, if the tick interrupted the read ([`Site::Disk`]);
//! - at the start of the game cycle, before it reads the keypad's direction
//!   ([`Site::CycleStart`]);
//! - before an action that writes a variable, if the tick interrupted its dispatch or
//!   its handler, which writes last ([`Site::Op`]): `set 25` when a room starts lets
//!   the timer read the keys, so a tick during it must not;
//! - in a busy wait (a loop on a flag the timer changes), one by one until the wait
//!   is over;
//! - otherwise at the next checkpoint, after the code between.
//!
//! Events keep their order. So the port's logic can be compared with the original's
//! exactly, whatever the timing. For play, [`Paced`] makes the ticks from the port's
//! own estimate of the original's speed.

use std::collections::VecDeque;

/// Code-segment addresses that are checkpoints: routine entries, and the heads of the
/// rule and action loops (`$115A`, `$2F03`), whose visits the port repeats exactly.
/// None is the head of a busy wait (as `$0F91` and `PCjr $4191` are), whose count of turns
/// the port can't know; `$0785` loops once per key read, which it can.
pub const CHECKPOINTS: [u16; 22] = [
    0x12A0, // poll_key_bios: INT 16h, bypassing the ring
    0x2209, // next_parse
    0x24B1, // read_line
    0x2816, // wait_key: one game cycle per loop
    0x293A, // game_cycle: one pass
    0x2AAD, // update_actor
    0x2EB4, // key_ring_pop
    0x2EF4, // run_action
    0x2F03, // run_action: each opcode
    0x3FB7, // countdowns and the draw pass
    0x48E3, // draw_room_picture
    0x4CA3, // save_restore_dialog
    0x078E, // save_slot
    0x0867, // load_slot
    0x08D4, // key_poll (INT 16h AH=1)
    0x08DF, // key_wait (INT 16h AH=0)
    0x0785, // wait_enter
    0x1143, // match_parser_rules
    0x1150, // match_cycle_rules
    0x115A, // the matcher: each rule
    0x5018, // title_wait
    0x1FB6, // read_sectors
];

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

/// Where in an interval the port fires the ticks that interrupted that code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// A disk read (`$1FB6`-`$2093`).
    Disk,
    /// The game cycle's start (`$293A`-`$2946`), before it reads the keypad's direction
    /// (DS:187F) at `$2946`: a key the timer takes there steers this pass. (A tick's
    /// address is the instruction it interrupted, still to run: at `$2946`, the read.)
    CycleStart,
    /// Action `n`'s dispatch (`$2F03`-`$2F1B`, `$38C1`-`$38D3`) and its handler, up to
    /// the jump back to `$2F03`, for the actions that write a variable (08-12, 35, 36:
    /// set, reset, increment, decrement, assign, and the room tests); and for print (0A),
    /// up to its first message queued (`$2FEC`-`$2FF5`, the queueing itself below).
    Op(u8),
    /// Queueing a message (`$1C72`-`$1C88`, up to its `cli`): a tick there finds the
    /// queue as it was, which the timer's print countdown runs on only when it isn't
    /// empty.
    QueueAdd,
    /// wait_key's timer countdown (v2, v9-v12) and its tests up to v23's (`$288D`-
    /// `$28E8`): a tick there that prints the last queued message (v23 = 0) lets this
    /// loop return to the line editor.
    WaitKeyTimers,
}

impl Site {
    pub fn contains(self, ip: u16) -> bool {
        match self {
            Site::Disk => (0x1FB6..0x2094).contains(&ip),
            Site::CycleStart => (0x293A..=0x2946).contains(&ip),
            Site::WaitKeyTimers => (0x288D..=0x28E8).contains(&ip),
            Site::QueueAdd => (0x1C72..=0x1C88).contains(&ip),
            Site::Op(op) => {
                let handler = match op {
                    0x0A => 0x2FEC..0x2FF6,
                    0x08 => 0x2FC4..0x2FD5,
                    0x09 => 0x2FD8..0x2FE9,
                    0x10 => 0x30E9..0x310F,
                    0x11 => 0x3112..0x3136,
                    0x12 => 0x3139..0x3158,
                    0x35 => 0x3656..0x3669,
                    0x36 => 0x366C..0x369A,
                    _ => return false,
                };
                (0x2F03..0x2F1C).contains(&ip) || (0x38C1..0x38D4).contains(&ip) || handler.contains(&ip)
            }
        }
    }
}

/// What fires at a checkpoint.
pub trait Clock {
    /// Checkpoint number `n` (counting from 1) has been reached, `cycles` estimated CPU
    /// cycles into the run: the events of the interval it starts, in order.
    fn at(&mut self, n: u64, cycles: u64) -> Vec<Event>;
    /// In a busy wait whose interval's events are spent: more, or none if the clock
    /// has nothing more to give (a replay that has diverged).
    fn waiting(&mut self, n: u64, cycles: u64) -> Vec<Event>;
    /// In a timed delay that polls the keyboard (the title's), `cycles` reached: what
    /// is due now. A replay gives nothing: its events come at the next checkpoint, as
    /// refemu-pcjr logged them. A clock for play gives the keys typed and the ticks.
    fn poll(&mut self, _cycles: u64) -> Vec<Event> {
        Vec::new()
    }
    /// The joystick, when the timer reads it: None when none is connected (the read
    /// times out), as with the original in refemu-pcjr, which has none.
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

/// The PCjr joystick as the game's read finds it (`PCjr $0563`): for each axis, the turns
/// of its loop until the axis's one-shot ended (6 or fewer left or up, 19 or more right
/// or down, 41 or more a time-out); whether each button is down.
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

/// Events logged by refemu-pcjr (`--events`), replayed at the same checkpoints.
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
