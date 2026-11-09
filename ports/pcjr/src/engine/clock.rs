//! When the timer interrupt fires.
//!
//! On the PCjr the timer interrupt (INT 08h, chained to the game's INT 1Ch handler
//! `$0808`) comes 18.2 times a second, between any two instructions. The handler reads
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
/// rule and action loops (`$47F8`, `$1ABB`), whose visits the port repeats exactly.
/// None is the head of a busy wait (as `$462F` and `$4191` are), whose count of turns
/// the port can't know; `$3EC2` loops once per key read, which it can.
pub const CHECKPOINTS: [u16; 22] = [
    0x0B92, // poll_key_bios: INT 16h, bypassing the ring
    0x0DC1, // next_parse
    0x1069, // read_line
    0x13CE, // wait_key: one game cycle per loop
    0x14F2, // game_cycle: one pass
    0x1665, // update_actor
    0x1A6C, // key_ring_pop
    0x1AAC, // run_action
    0x1ABB, // run_action: each opcode
    0x2C3C, // countdowns and the draw pass
    0x34F5, // draw_room_picture
    0x38B8, // save_restore_dialog
    0x3ABC, // save_slot
    0x3B99, // load_slot
    0x3C0A, // key_poll (INT 16h AH=1)
    0x3C15, // key_wait (INT 16h AH=0)
    0x3EC2, // wait_enter
    0x47E1, // match_parser_rules
    0x47EE, // match_cycle_rules
    0x47F8, // the matcher: each rule
    0x4A7A, // title_wait
    0x4D93, // read_sectors
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
    /// A disk read (`$4D93`-`$4E2F`).
    Disk,
    /// The game cycle's start (`$14F2`-`$14FD`), before it reads the keypad's direction
    /// (DS:1279) at `$14FE`: a key the timer takes there steers this pass.
    CycleStart,
    /// Action `n`'s dispatch (`$1ABB`-`$1AD3`, `$2479`-`$248B`) and its handler, up to
    /// the jump back to `$1ABB`, for the actions that write a variable (08-12, 35, 36:
    /// set, reset, increment, decrement, assign, and the room tests).
    Op(u8),
}

impl Site {
    pub fn contains(self, ip: u16) -> bool {
        match self {
            Site::Disk => (0x4D93..0x4E30).contains(&ip),
            Site::CycleStart => (0x14F2..0x14FE).contains(&ip),
            Site::Op(op) => {
                let handler = match op {
                    0x08 => 0x1B7C..0x1B8D,
                    0x09 => 0x1B90..0x1BA1,
                    0x10 => 0x1CA1..0x1CC7,
                    0x11 => 0x1CCA..0x1CEE,
                    0x12 => 0x1CF1..0x1D10,
                    0x35 => 0x220E..0x2221,
                    0x36 => 0x2224..0x2252,
                    _ => return false,
                };
                (0x1ABB..0x1AD4).contains(&ip) || (0x2479..0x248C).contains(&ip) || handler.contains(&ip)
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
}
