//! Original-speed model (port plan decision 1, option b).
//!
//! The original has no speed control: a pass of its main loop takes as long as the 6502
//! needs. The engine counts the same work the original does in each pass, and these
//! per-event costs, fitted against the reference emulator (refemu/README.md), turn the
//! counts into the original's pass duration.
//!
//! Fit over the maximum-score playthrough (routes/max_score.route; 17,299 ordinary
//! passes, port counts against the original's cycles in refemu): median error 1.1%, 90%
//! of passes within 3.2%, the whole run 0.4% short. BASE is the fit's intercept; the rule-byte cost carries most
//! of the fixed per-pass work. Loading passes fit within 3.9% (median); the rest of
//! their spread is the disk's head position and rotation, which is not modelled.

use super::{f, World, MAX_ACTORS};

pub const CPU_HZ: f64 = 1_020_484.0;
pub const BASE: f64 = -6_560.0;
/// Per byte read by the rule matcher ($6358).
pub const RULE_BYTE: f64 = 57.3;
/// Per nibble of cel data decoded while drawing a sprite (aux $1194).
pub const CEL_NIBBLE: f64 = 152.7;
/// Per pixel copied from the picture buffer to the screen (aux $145A).
pub const SCREEN_PIXEL: f64 = 128.4;
/// Per character drawn in graphics mode (aux $14A4).
pub const CHAR: f64 = 2_284.0;
/// Per sector read from disk (RWTS $BD00), including rotational delay.
pub const SECTOR: f64 = 69_300.0;
/// Per picture pixel plotted by lines and fills (aux $102F).
pub const PLOT: f64 = 397.0;
/// Fixed extra per room change (head seeks and bookkeeping).
pub const ROOM_CHANGE: f64 = 320_800.0;

#[derive(Default, Clone, Debug)]
pub struct Cost {
    pub rule_bytes: u64,
    pub cel_nibbles: u64,
    pub screen_pixels: u64,
    pub chars: u64,
    pub sectors: u64,
    pub plots: u64,
    pub room_changes: u64,
    /// Characters drawn by the prompt redraw that the port does at the end of a pass but
    /// the original does at the start of the next one ($4029-$4036); counted there.
    pub next_chars: u64,
}

/// A picture drawn during a pass. The original plots straight to the screen, so a room
/// change shows the old room while the disk turns, then the new picture appearing line
/// by line and fill by fill; front ends replay that from these.
#[derive(Clone, Debug)]
pub struct PicDraw {
    /// The work done in the pass before the drawing started.
    pub before: Cost,
    /// The buffer drawn onto (cleared to white, or the picture already there).
    pub onto: Vec<u8>,
    pub data: std::rc::Rc<Vec<u8>>,
    pub plots: u32,
    /// Measured (load, draw) seconds, in place of the speed model (the title screens).
    pub timing: Option<(f64, f64)>,
}

impl PicDraw {
    /// The buffer after the first `n` plots.
    pub fn partial(&self, n: u32) -> Vec<u8> {
        crate::pic::draw_partial(self.onto.clone(), &self.data, n)
    }
    /// Seconds from the start of the pass to the start and end of the drawing.
    pub fn span(&self, fast_loads: bool) -> (f64, f64) {
        if let Some((load, draw)) = self.timing {
            return if fast_loads { (0.0, 0.0) } else { (load, load + draw) };
        }
        if fast_loads {
            let t = self.before.seconds_fast_loads();
            (t, t)
        } else {
            let t = self.before.seconds();
            (t, t + PLOT * self.plots as f64 / CPU_HZ)
        }
    }
}

impl Cost {
    pub fn cycles(&self) -> f64 {
        (BASE
            + RULE_BYTE * self.rule_bytes as f64
            + CEL_NIBBLE * self.cel_nibbles as f64
            + SCREEN_PIXEL * self.screen_pixels as f64
            + CHAR * self.chars as f64
            + SECTOR * self.sectors as f64
            + PLOT * self.plots as f64
            + ROOM_CHANGE * self.room_changes as f64)
            .max(0.0)
    }

    /// The original's duration for this work, in seconds.
    pub fn seconds(&self) -> f64 {
        self.cycles() / CPU_HZ
    }

    /// The duration without the disk reads, room-change bookkeeping and picture drawing
    /// (kqplay's fast loads). Those are almost all of a room change's 10 s or so; the
    /// rest of every pass keeps the original's pace. Only the real time changes: the
    /// game's state depends on passes, never on time (the Apple has no clock).
    pub fn seconds_fast_loads(&self) -> f64 {
        Cost { sectors: 0, room_changes: 0, plots: 0, ..self.clone() }.seconds()
    }
}

impl World {
    /// The work counted since the last call (normally one pass).
    pub fn take_cost(&mut self) -> Cost {
        let mut c = std::mem::take(&mut self.cost);
        self.cost.chars = std::mem::take(&mut c.next_chars);
        c
    }

    /// Decoding the actor's current cel.
    pub(crate) fn cost_cel(&mut self, s: usize) {
        if let Some(n) = self.cel_ref(s).map(|c| c.nibbles) {
            self.cost.cel_nibbles += n as u64;
        }
    }

    /// aux $13C2: the screen area updated for an actor covers its old and new cels:
    /// max(old, new width) + |dx| (clipped at x = $96) by max(old, new height) + the
    /// distance between the two tops ($162B gives each cel's top-left corner).
    pub(crate) fn cost_screen(&mut self, s: usize) {
        let Some((w1, h1)) = self.cel_dims(s) else { return };
        let (w0, h0) = if self.drawn_w[s] == 0 { (w1, h1) } else { (self.drawn_w[s], self.drawn_h[s]) };
        let a = &self.actors[s];
        let (x0, x1) = (a[f::X] as i32, a[f::NEW_X] as i32);
        let top0 = a[f::Y] as i32 - h0 as i32 + 1;
        let top1 = a[f::NEW_Y] as i32 - h1 as i32 + 1;
        let left = x0.min(x1);
        let width = ((x0 - x1).abs() + w0.max(w1) as i32).min(0x96 - left).max(0);
        let height = (top0 - top1).abs() + h0.max(h1) as i32;
        self.cost.screen_pixels += (width * height).max(0) as u64;
    }

    /// The full redraw that follows a successful draw ($5425 -> $4E2C/$4E42): every
    /// on-screen actor is decoded and copied again.
    pub(crate) fn cost_redraw_all(&mut self) {
        for s in 0..(self.g.actor_count as usize).min(MAX_ACTORS) {
            if self.actors[s][f::ROOM] == self.vars[1] && self.actors[s][f::DRAWN] != 0 {
                self.cost_cel(s);
                self.cost_screen(s);
            }
        }
    }

    pub(crate) fn cost_load(&mut self, kind: usize, n: u8) {
        if let Some(&k) = self.res.sectors.get(kind).and_then(|v| v.get(n as usize)) {
            self.cost.sectors += k as u64;
        }
    }

    fn cel_ref(&self, s: usize) -> Option<&crate::view::Cel> {
        let v = self.res.views.get(self.actor_view[s]? as usize)?.as_ref()?;
        let a = &self.actors[s];
        v.loops.get(a[f::LOOP] as usize)?.cels.get(a[f::CEL] as usize)
    }
}
