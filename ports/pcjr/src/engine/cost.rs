//! The work each pass does, counted for the speed model.
//!
//! The original has no speed control: a pass takes as long as the 8088 needs. The port
//! counts what the original's time goes on: the checkpoints passed (each a routine
//! entered or a loop turned: a rule tested, an action run, an actor updated), the bytes
//! and pixels its drawing routines handle, the characters the BIOS draws, the timer
//! ticks it serves, the sectors it reads and the picture's plots. Fitted constants turn
//! the counts into cycles (see `docs/engine.md`, "Speed").

use super::clock::CHECKPOINTS;

// The constants: CPU cycles per count, fitted by tools/costfit.py over the max-score and
// magic-ring routes (43,534 passes), every one kept >= 0. On the gold-key and
// giant-sling routes, which the fit didn't see: passes without a disk read or a
// picture within 1.56% (median) and 3.75% (90%) of the original's cycles in refemu,
// the loading passes within 0.71% and 2.35%, the whole runs 0.12% short. Busy waits
// for the timer aren't work: the paced clock takes them to the tick that ends them.
//
// An ordinary pass: its rules, its actors and their drawing, the characters, the ticks.
const ACTOR: f64 = 13_704.0; // per actor updated ($1665)
const RULE: f64 = 805.3; // per rule tested ($47F8)
const CONDITION: f64 = 1.06;
const BACKGROUND: f64 = 32.78; // per byte saved or put back
const CEL_WRITTEN: f64 = 19.82; // per cel pixel written
const CEL_NIBBLE: f64 = 375.0; // per nibble of a packed cel unpacked ($3418)
const BLIT: f64 = 60.13; // per byte copied to the displayed page
const CHAR: f64 = 3_374.0; // per character the BIOS draws
const SCROLLED: f64 = 13_816.0; // per text row scrolled
const TICK: f64 = 2_140.0; // per timer tick served
const JOYSTICK: f64 = 6_975.0; // per joystick read (it times out)
const SOUND: f64 = 249.3; // per byte sent to the sound chip
const BELOW: f64 = 156.8; // per row looked down for a priority under 15
                          // Loading: the disk and the picture.
const SECTOR: f64 = 111_365.0;
const PICTURE: f64 = 1_788_243.0; // per room picture: clearing, the frame, the colours merged
const ABS_STEP: f64 = 1_284.0; // per step of the absolute line (F6)
const TO_STEP: f64 = 1_486.0; // per step of the other lines (a multiply and a divide)
const LINE: f64 = 1_184.0;
const FILL_BYTE: f64 = 69.91;
const FILL_RUN: f64 = 1_625.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cost {
    /// Visits of each checkpoint (by its place in `CHECKPOINTS`).
    pub checkpoints: [u32; CHECKPOINTS.len()],
    /// Conditions tested by the rule matcher (each a variable against a value, or an
    /// "ego in" rectangle).
    pub conditions: u32,
    /// Bytes of background saved or put back under the actors (`$31ED`, `$31AA`).
    pub background: u32,
    /// Cel pixels gone through by the actor drawing (`$322F`), and those written.
    pub cel_pixels: u32,
    pub cel_written: u32,
    /// The packed cels' nibbles, each draw unpacking its cel (`$3418`).
    pub cel_nibbles: u32,
    /// Bytes copied from the work page to the displayed page (`$3477`).
    pub blit: u32,
    /// Characters drawn by the BIOS, and text rows scrolled.
    pub chars: u32,
    pub scrolled: u32,
    /// Timer ticks served (`$0808`); of them, those that read the joystick (none is
    /// connected: the read times out); sound bytes sent to the chip.
    pub ticks: u32,
    pub joystick: u32,
    pub sound: u32,
    /// Rows looked down for a priority under 15 while drawing actors (`$32BF`).
    pub below: u32,
    pub sectors: u32,
    /// Picture pixels plotted by lines and fills, and the drawing's work in detail.
    pub plots: u32,
    pub pic: crate::pic::Work,
    /// Ticks that ended a busy wait (`$462F` and the like): the time to them is the
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
        let p = &self.pic;
        ACTOR * cp(0x1665)
            + RULE * cp(0x47F8)
            + PICTURE * cp(0x34F5)
            + CONDITION * self.conditions as f64
            + BACKGROUND * self.background as f64
            + CEL_WRITTEN * self.cel_written as f64
            + CEL_NIBBLE * self.cel_nibbles as f64
            + BLIT * self.blit as f64
            + CHAR * self.chars as f64
            + SCROLLED * self.scrolled as f64
            + TICK * self.ticks as f64
            + JOYSTICK * self.joystick as f64
            + SOUND * self.sound as f64
            + BELOW * self.below as f64
            + SECTOR * self.sectors as f64
            + ABS_STEP * p.abs_steps as f64
            + TO_STEP * p.to_steps as f64
            + LINE * p.lines as f64
            + FILL_BYTE * p.fill_bytes as f64
            + FILL_RUN * p.fill_runs as f64
    }

    /// Count a visit of the checkpoint at `at`.
    pub fn checkpoint(&mut self, at: u16) {
        if let Some(i) = CHECKPOINTS.iter().position(|&a| a == at) {
            self.checkpoints[i] += 1;
        }
    }
}
