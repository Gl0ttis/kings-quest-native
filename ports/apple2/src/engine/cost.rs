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
/// The drawing itself, run by run (tools/runfit.py: 125 pictures of the max-score run,
/// every plot timed in refemu): per plot and per line segment, per plot and per fill
/// seed. Whole drawings within 1.1% (median), 3.3% (90%). These pace the replay of a
/// picture being drawn; a pass's total still uses PLOT, which also covers the clearing
/// and set-up before the drawing.
pub const LINE_PLOT: f64 = 239.5;
pub const LINE_SEGMENT: f64 = 308.0;
pub const FILL_PLOT: f64 = 330.2;
pub const FILL_SEED: f64 = 10_702.0;
/// From the work counted before a picture is drawn to its first plot: clearing the
/// screen and setting up (median over the 126 room loads of the max-score run; 90% of
/// them within 0.92M-1.67M).
pub const DRAW_START: f64 = 1_130_000.0;
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
    /// Of `plots`, those plotted by fills.
    pub fill_plots: u64,
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
    /// The drawing's runs: (plots before it, a fill?), one per drawing command.
    pub runs: Vec<(u32, bool)>,
    /// Measured (load, draw) seconds, in place of the speed model (the title screens).
    pub timing: Option<(f64, f64)>,
}

impl PicDraw {
    /// The buffer after the first `n` plots.
    pub fn partial(&self, n: u32) -> Vec<u8> {
        crate::pic::draw_partial(self.onto.clone(), &self.data, n)
    }
    /// Seconds from the start of the pass to the start and end of the drawing: it starts
    /// DRAW_START after the work counted before it, and takes `draw_cycles`.
    pub fn span(&self, fast_loads: bool) -> (f64, f64) {
        if let Some((load, draw)) = self.timing {
            return if fast_loads { (0.0, 0.0) } else { (load, load + draw) };
        }
        if fast_loads {
            let t = self.before.seconds_fast_loads();
            (t, t)
        } else {
            let t = self.before.seconds() + DRAW_START / CPU_HZ;
            (t, t + self.draw_cycles() / CPU_HZ)
        }
    }

    /// The drawing's duration in cycles, from its runs.
    pub fn draw_cycles(&self) -> f64 {
        self.run_costs().map(|(_, n, fixed, per)| fixed + per * n as f64).sum()
    }

    /// (plots before, plots in, fixed cycles, cycles per plot) for each run.
    fn run_costs(&self) -> impl Iterator<Item = (u32, u32, f64, f64)> + '_ {
        self.runs.iter().enumerate().map(|(i, &(start, fill))| {
            let end = self.runs.get(i + 1).map_or(self.plots, |r| r.0);
            let (fixed, per) = if fill { (FILL_SEED, FILL_PLOT) } else { (LINE_SEGMENT, LINE_PLOT) };
            (start, end - start, fixed, per)
        })
    }

    /// The plots done when `frac` (0 to 1) of the drawing's time has passed: each run
    /// spends its fixed cost first, then plots at its rate.
    pub fn plots_at(&self, frac: f64) -> u32 {
        if frac >= 1.0 || self.runs.is_empty() {
            return if frac >= 1.0 { self.plots } else { (frac.max(0.0) * self.plots as f64) as u32 };
        }
        let mut left = frac.max(0.0) * self.draw_cycles();
        for (start, n, fixed, per) in self.run_costs() {
            let cost = fixed + per * n as f64;
            if left < cost {
                let into = ((left - fixed).max(0.0) / per) as u32;
                return start + into.min(n);
            }
            left -= cost;
        }
        self.plots
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
    /// (kq's fast loads). Those are almost all of a room change's 10 s or so; the
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
        let want = self.res.disks.get(kind).and_then(|v| v.get(n as usize)).copied().unwrap_or(0);
        self.change_disk(want);
    }

    /// $6009 -> $6075: before a read from disk `want` (volume; 0 = the shared tracks), if
    /// another disk is in the drive, the original asks for it and waits for RETURN:
    /// "Please insert side A/B/C and press RETURN." ($6CDC, the letter from $6CD0,
    /// $6CF1), or for the save disk (volume 10) its own message ($6D05). The port shows
    /// them only with `swap_prompts`.
    pub(crate) fn change_disk(&mut self, want: u8) {
        if want == 0 || want == self.drive {
            return;
        }
        self.note(format!("disk swap: {} -> {want}", self.drive));
        if self.swap_prompts {
            // $6077: the disk number goes into $702C too, so the next pass redraws the
            // prompt under the message.
            self.text.need_prompt = true;
            let mut msg = Vec::new();
            if want == 10 {
                msg.extend(self.engine_text(0x6D05));
            } else {
                msg.extend(self.engine_text(0x6CDC));
                let letter = self.engine_word(0x6CD0 + 2 * (want as u16 - 1));
                msg.extend(self.engine_text(letter));
                msg.extend(self.engine_text(0x6CF1));
            }
            self.put_str(&msg);
            while self.io_wait_key() != 0x0D {}
        }
        self.drive = want;
    }

    fn cel_ref(&self, s: usize) -> Option<&crate::view::Cel> {
        self.actor_cel(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_pace_follows_the_runs() {
        // A line segment of 10 plots, then a fill seed of 100.
        let d = PicDraw {
            before: Cost::default(),
            onto: Vec::new(),
            data: std::rc::Rc::new(Vec::new()),
            plots: 110,
            runs: vec![(0, false), (10, true)],
            timing: None,
        };
        let line = LINE_SEGMENT + 10.0 * LINE_PLOT;
        let total = line + FILL_SEED + 100.0 * FILL_PLOT;
        assert!((d.draw_cycles() - total).abs() < 1e-6);
        assert_eq!(d.plots_at(0.0), 0);
        assert_eq!(d.plots_at(line / total), 10);
        // During the fill seed's fixed cost nothing new is plotted.
        assert_eq!(d.plots_at((line + FILL_SEED / 2.0) / total), 10);
        assert_eq!(d.plots_at(1.0), 110);
        assert!((1..100).all(|i| d.plots_at(i as f64 / 100.0) <= d.plots_at((i + 1) as f64 / 100.0)));
    }
}
