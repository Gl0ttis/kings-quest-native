//! The work each pass does, counted for the speed model.
//!
//! The PC's only speed control is the wait before the actors move (`$3FE7`): they take
//! a step at most once a tick. A pass takes the time of its work, then of its waits
//! (which the port takes as the original does, to the tick that ends them). The port
//! counts what the work goes on: the checkpoints passed (each a routine entered or a
//! loop turned: a rule tested, an action run, an actor updated), the bytes and pixels
//! its drawing routines handle, the characters the BIOS draws, the timer ticks it
//! serves, the sectors it reads (and the drive's time for them) and the picture's
//! plots. Fitted constants turn the counts into cycles (see `docs/engine.md`, "Speed").

use super::clock::CHECKPOINTS;

// The constants: CPU cycles per count, fitted by tools/costfit.py against the original's
// work in refemu-pc (its cycles less those it spent waiting: kqframes-pc --waits) over
// the max-score and magic-ring routes (42,065 passes), every one kept >= 0. On the other
// routes, which the fit didn't see (39,222 passes): passes without a disk read or a
// picture within 1.51% (median) and 3.82% (90%) of the original's work. The PC's waits
// aren't work: the port waits as the original does, to the tick that ends the wait
// (most passes wait before the actors move, `$3FE7`). The drive's time is the drive's
// ([`Drive`]); the sectors' and the picture's end were fitted with it in the engine's
// timeline (refitted until they settled: the drive's time depends on when each read
// starts). The drawing of a room picture is timed on its own (tools/drawfit.py, from
// every byte the original writes to the CGA as it draws, in the max-score run's 172
// pictures): the port shows it at that pace, each picture's drawing within 1.3%
// (median) and 2.7% (90%) of the original's, and part way within 0.5 and 1.5 points.
// The whole runs (examples/speedcmp.rs, the waits included): within 0.2-0.4% of the
// original's time on the walk, the gold key, the giant, the maximum score and the
// magic ring.
//
// An ordinary pass: its rules, its actors and their drawing, the characters, the ticks.
const ACTOR: f64 = 10_460.0; // per actor updated ($2AAD)
const RULE: f64 = 574.95; // per rule tested ($115A)
const CONDITION: f64 = 0.0;
const BACKGROUND: f64 = 26.51; // per byte saved or put back
const CEL_WRITTEN: f64 = 12.83; // per cel pixel written
const CEL_NIBBLE: f64 = 260.65; // per nibble of a packed cel unpacked
const BLIT: f64 = 40.48; // per byte copied to the screen ($1875)
const CHAR: f64 = 3_307.0; // per character the BIOS draws
const SCROLLED: f64 = 12_196.0; // per text row scrolled
const TICK: f64 = 2_792.0; // per timer tick served
const JOYSTICK: f64 = 12_420.0; // per joystick read that times out (256 turns of its loop)
const JOYSTICK_TURN: f64 = JOYSTICK / 256.0; // per turn, a connected stick's read
const SOUND: f64 = 1_896.0; // per note sent to the speaker
const BELOW: f64 = 111.29; // per row looked down for a priority under 15
                           // Loading: the disk (besides the drive's time) and the picture.
const SECTOR: f64 = 76_862.0;
// A room picture: the clear and the frame, before its first byte; after its last, the
// colours merged into the buffer (`$14DC`) and what follows, the picture shown
// meanwhile.
const PICTURE_START: f64 = 171_962.0;
const PICTURE_END: f64 = 100_723.0 + 2_711_663.0;
const ABS_STEP: f64 = 808.38; // per step of the absolute line (F6)
const TO_STEP: f64 = 933.24; // per step of the other lines (a multiply and a divide)
const LINE: f64 = 579.31;
const FILL_BYTE: f64 = 26.77;
const FILL_SCAN: f64 = 44.72;
const FILL_RUN: f64 = 2_413.0;
const FILL_SEED: f64 = 0.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cost {
    /// Visits of each checkpoint (by its place in `CHECKPOINTS`).
    pub checkpoints: [u32; CHECKPOINTS.len()],
    /// Conditions tested by the rule matcher (each a variable against a value, or an
    /// "ego in" rectangle).
    pub conditions: u32,
    /// Bytes of background saved or put back under the actors (`$45DC`, `$4599`).
    pub background: u32,
    /// Cel pixels gone through by the actor drawing (`$461E`), and those written.
    pub cel_pixels: u32,
    pub cel_written: u32,
    /// The packed cels' nibbles, each draw unpacking its cel (`$4807`).
    pub cel_nibbles: u32,
    /// Bytes copied from the buffer to the screen (`$4865`).
    pub blit: u32,
    /// Characters drawn by the BIOS, and text rows scrolled.
    pub chars: u32,
    pub scrolled: u32,
    /// Timer ticks served (`$1A18`); of them, those that read the joystick and it
    /// timed out (none connected); with a stick, the turns of the read's loop until
    /// both axes settled; notes sent to the speaker (one a music record).
    pub ticks: u32,
    pub joystick: u32,
    pub joystick_turns: u32,
    pub sound: u32,
    /// Rows looked down for a priority under 15 while drawing actors (`$46AE`).
    pub below: u32,
    pub sectors: u32,
    /// Picture pixels plotted by lines and fills, and the drawing's work in detail.
    pub plots: u32,
    pub pic: crate::pic::Work,
    /// Room pictures finished (their colours merged): their end's cost comes after
    /// the drawing, which a front end shows as it goes.
    pub pictures_done: u32,
    /// The drive's time for the sectors read and written ([`Drive`]), in cycles.
    pub drive: u64,
    /// Ticks that ended a busy wait (`$0F91` and the like): the time to them is the
    /// wait, not work.
    pub wait_ticks: u32,
}

impl Cost {
    /// The column names of [`Cost::row`], for a CSV.
    pub fn header() -> String {
        let mut h: Vec<String> = CHECKPOINTS.iter().map(|a| format!("c{a:04X}")).collect();
        h.extend(
            [
                "conditions",
                "background",
                "cel_pixels",
                "cel_written",
                "cel_nibbles",
                "blit",
                "chars",
                "scrolled",
                "ticks",
                "joystick",
                "sound",
                "below",
                "sectors",
                "drive",
                "plots",
                "abs_steps",
                "to_steps",
                "lines",
                "fill_bytes",
                "fill_scans",
                "fill_runs",
                "fill_seeds",
                "wait_ticks",
            ]
            .map(String::from),
        );
        h.join(",")
    }

    pub fn row(&self) -> String {
        let mut v: Vec<String> = self.checkpoints.iter().map(|n| n.to_string()).collect();
        v.extend(
            [
                self.conditions,
                self.background,
                self.cel_pixels,
                self.cel_written,
                self.cel_nibbles,
                self.blit,
                self.chars,
                self.scrolled,
                self.ticks,
                self.joystick,
                self.sound,
                self.below,
                self.sectors,
                self.drive as u32,
                self.plots,
                self.pic.abs_steps,
                self.pic.to_steps,
                self.pic.lines,
                self.pic.fill_bytes,
                self.pic.fill_scans,
                self.pic.fill_runs,
                self.pic.fill_seeds,
                self.wait_ticks,
            ]
            .map(|n| n.to_string()),
        );
        v.join(",")
    }

    /// The cycles of the characters drawn and the rows scrolled (counted by the text
    /// screen until the pass ends).
    pub fn text_cycles(chars: u32, scrolled: u32) -> f64 {
        CHAR * chars as f64 + SCROLLED * scrolled as f64
    }

    /// The original's cycles for this much work.
    pub fn cycles(&self) -> f64 {
        let cp = |a: u16| CHECKPOINTS.iter().position(|&x| x == a).map_or(0, |i| self.checkpoints[i]) as f64;
        ACTOR * cp(0x2AAD)
            + RULE * cp(0x115A)
            + CONDITION * self.conditions as f64
            + BACKGROUND * self.background as f64
            + CEL_WRITTEN * self.cel_written as f64
            + CEL_NIBBLE * self.cel_nibbles as f64
            + BLIT * self.blit as f64
            + CHAR * self.chars as f64
            + SCROLLED * self.scrolled as f64
            + TICK * self.ticks as f64
            + JOYSTICK * self.joystick as f64
            + JOYSTICK_TURN * self.joystick_turns as f64
            + SOUND * self.sound as f64
            + BELOW * self.below as f64
            + self.loading_cycles()
    }

    /// Of [`Cost::cycles`], those of loading a room: the sectors read and the
    /// pictures drawn (a player's fast loads skip their time).
    pub fn loading_cycles(&self) -> f64 {
        let cp = |a: u16| CHECKPOINTS.iter().position(|&x| x == a).map_or(0, |i| self.checkpoints[i]) as f64;
        PICTURE_START * cp(0x48E3)
            + PICTURE_END * self.pictures_done as f64
            + SECTOR * self.sectors as f64
            + self.drive as f64
            + Cost::drawing_cycles(&self.pic)
    }

    /// The cycles of a picture's lines and fills (`work`), without its fixed part.
    pub fn drawing_cycles(p: &crate::pic::Work) -> f64 {
        ABS_STEP * p.abs_steps as f64
            + TO_STEP * p.to_steps as f64
            + LINE * p.lines as f64
            + FILL_BYTE * p.fill_bytes as f64
            + FILL_SCAN * p.fill_scans as f64
            + FILL_RUN * p.fill_runs as f64
            + FILL_SEED * p.fill_seeds as f64
    }

    /// Count a visit of the checkpoint at `at`.
    pub fn checkpoint(&mut self, at: u16) {
        if let Some(i) = CHECKPOINTS.iter().position(|&a| a == at) {
            self.checkpoints[i] += 1;
        }
    }
}

/// The floppy drive's timing (the machine's, as refemu-pc models it): the motor's start
/// if it has stopped, the head's steps and settling, the wait for the first sector to
/// come round, and the sectors passing the head. A 5.25-inch drive at 300 rpm, 250
/// kbit/s (6,250 bytes a turn); the step and settle times are the game's own disk
/// table's (6 ms a track, 15 ms), the motor's start the BIOS's (250 ms, after 2 s
/// unused). The game reads and writes a track side at a time (`$1FC6`: up to the end
/// of the first sector's side, then whole sides, then the rest).
#[derive(Clone, Debug, Default)]
pub struct Drive {
    head: u8,
    last: Option<u64>,
}

impl Drive {
    /// The cycles to read or write `n` sectors from logical sector `lba` (9 a side, 2
    /// sides), starting at cycle `now`.
    pub fn time(&mut self, mut now: u64, lba: usize, n: usize) -> u64 {
        let (mut at, mut left, mut total) = (lba, n, 0);
        loop {
            let first = at % 9;
            let k = (9 - first).min(left);
            // A call for no sectors still waits for the sector to come round.
            let t = self.call(now, (at / 18) as u8, first as u8 + 1, k.max(1) as u8);
            now += t;
            total += t;
            at += k;
            left -= k;
            if left == 0 {
                return total;
            }
        }
    }

    /// One BIOS call: `n` sectors from record `first` of cylinder `cyl`.
    fn call(&mut self, now: u64, cyl: u8, first: u8, n: u8) -> u64 {
        let ms = |t: f64| (t * super::clock::CPU_HZ / 1000.0) as u64;
        let turn = ms(200.0);
        let byte = turn as f64 / 6250.0;
        let mut t = 0;
        if self.last.is_none_or(|l| now.saturating_sub(l) > ms(2000.0)) {
            t += ms(250.0);
        }
        let steps = (cyl as i32 - self.head as i32).unsigned_abs() as u64;
        if steps > 0 {
            t += steps * ms(6.0) + ms(15.0);
        }
        self.head = cyl;
        // The track: the index gap (146 bytes), then 9 sectors of 614 bytes with their
        // headers and gaps; a read starts at its first sector's ID.
        let start = (146.0 + (first.max(1) - 1) as f64 * 614.0) * byte;
        let angle = ((now + t) % turn) as f64;
        let wait = (start - angle).rem_euclid(turn as f64);
        t += (wait + n as f64 * 614.0 * byte) as u64;
        self.last = Some(now + t);
        t
    }
}
