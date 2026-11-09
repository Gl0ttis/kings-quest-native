//! The two video pages and the picture drawing (`$0615`, `$064C`, `$34F5`, `$3534`).
//!
//! - **The displayed page** (segment 1800h): the colours, each picture pixel a byte
//!   with the colour in both nibbles.
//! - **The work page** (segment 1000h): while a picture is drawn, its priorities (in
//!   both nibbles); after `$064C`, the colour in the high nibble and the priority in the
//!   low one. The actors are drawn into it, and changed rectangles copied from it to
//!   the displayed page.
//!
//! Both are 160 × 168, the picture's size. The painter (pic.rs) addresses them in the
//! PCjr's mode-9 layout; [`Screen`] maps that to rows and columns.

use super::clock;
use super::cost::Cost;
use super::World;
use crate::pic::{self, Mem, Work};
use std::rc::Rc;

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 168;

#[derive(Clone)]
pub struct Screen {
    pub work: Vec<u8>,
    pub shown: Vec<u8>,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { work: vec![0; WIDTH * HEIGHT], shown: vec![0; WIDTH * HEIGHT] }
    }
}

/// A mode-9 page offset as (x, y) in the picture, if it is in it.
fn at(off: u16) -> Option<usize> {
    let bank = (off / 0x2000) as usize;
    let r = (off % 0x2000) as usize;
    let (row, x) = (r / WIDTH, r % WIDTH);
    let y = row * 4 + bank;
    (y < HEIGHT).then_some(y * WIDTH + x)
}

/// The pages and the picture being drawn, for the painter. Outside the picture,
/// reads give 0 (never fillable) and writes are dropped: the frame round every room
/// picture keeps the drawing inside.
struct Pages<'a> {
    s: &'a mut Screen,
    data: &'a [u8],
    /// For a front end: the displayed page's writes, and how far the drawing had got.
    log: Option<&'a mut DrawLog>,
}

/// A picture's drawing on the displayed page, for showing it as it goes.
#[derive(Default)]
struct DrawLog {
    /// Each write: the pixel and its value.
    writes: Vec<(usize, u8)>,
    /// Marks about a quarter of a frame's time apart: the writes done by then, and the
    /// work and plots so far.
    marks: Vec<(usize, Work, u32)>,
}

impl Mem for Pages<'_> {
    fn read(&self, seg: u32, off: u16) -> u8 {
        let plane = if seg == 0x1800 { &self.s.shown } else { &self.s.work };
        at(off).map_or(0, |i| plane[i])
    }
    fn write(&mut self, seg: u32, off: u16, v: u8) {
        if let Some(i) = at(off) {
            if seg == 0x1800 {
                self.s.shown[i] = v;
                if let Some(l) = self.log.as_mut() {
                    l.writes.push((i, v));
                }
            } else {
                self.s.work[i] = v;
            }
        }
    }
    fn data(&self, i: usize) -> Option<u8> {
        self.data.get(i).copied()
    }
    fn progress(&mut self, work: &Work, plots: u32) {
        let Some(l) = self.log.as_mut() else { return };
        let last = l.marks.last().map_or(0.0, |m| Cost::drawing_cycles(&m.1));
        if Cost::drawing_cycles(work) - last >= clock::CYCLES_PER_FRAME as f64 / 4.0 {
            l.marks.push((l.writes.len(), *work, plots));
        }
    }
}

impl Screen {
    /// Bytes read into segment 1000h from its start: the work page, then the displayed
    /// page (the Play diskette's copy goes through them).
    pub fn load_pages(&mut self, data: &[u8]) {
        for (o, &b) in data.iter().enumerate().take(0x10000) {
            let (plane, off) = if o < 0x8000 { (&mut self.work, o) } else { (&mut self.shown, o - 0x8000) };
            if let Some(i) = at(off as u16) {
                plane[i] = b;
            }
        }
    }
}

impl World {
    /// `$353F`: draw `data` onto the pages. The original draws on the displayed page,
    /// so with a front end watching (`on_frame`), the drawing is shown as it goes: the
    /// displayed page's writes are played back in time, its work charged as it is done.
    fn interpret(&mut self, data: &[u8]) {
        let mut log = self.on_frame.is_some().then(DrawLog::default);
        let before = log.as_ref().map(|_| self.screen.shown.clone());
        let mut p = Pages { s: &mut self.screen, data, log: log.as_mut() };
        let r = pic::draw(&mut p, &mut self.g.pic);
        if let (Some(mut log), Some(before)) = (log, before) {
            log.marks.push((log.writes.len(), r.work, r.plots));
            if let (true, Some(t)) = (self.drawing_room, self.drawing_trace.as_mut()) {
                if let Some(pic) = t.last_mut() {
                    // The frame and the picture: one drawing, their work added up.
                    let (n0, w0) = pic.last().copied().unwrap_or_default();
                    pic.extend(log.marks.iter().map(|&(n, w, _)| (n0 + n, w0.plus(&w))));
                }
            }
            self.screen.shown = before;
            self.show_drawing(&log);
        } else {
            self.add_drawing_work(&r.work, r.plots);
        }
    }

    fn add_drawing_work(&mut self, w: &Work, plots: u32) {
        self.cost.plots += plots;
        let c = &mut self.cost.pic;
        c.abs_steps += w.abs_steps;
        c.to_steps += w.to_steps;
        c.lines += w.lines;
        c.fill_bytes += w.fill_bytes;
        c.fill_scans += w.fill_scans;
        c.fill_runs += w.fill_runs;
        c.fill_seeds += w.fill_seeds;
    }

    /// The logged drawing onto the displayed page, mark by mark: the writes up to it,
    /// its work charged (the same in all as drawing it at once), and the front end's
    /// frame when one is due, at its time.
    fn show_drawing(&mut self, log: &DrawLog) {
        let (mut done, mut work, mut plots) = (0, Work::default(), 0);
        for &(upto, w, p) in &log.marks {
            for &(i, v) in &log.writes[done..upto] {
                self.screen.shown[i] = v;
            }
            done = upto;
            let delta = Work {
                abs_steps: w.abs_steps - work.abs_steps,
                to_steps: w.to_steps - work.to_steps,
                lines: w.lines - work.lines,
                fill_bytes: w.fill_bytes - work.fill_bytes,
                fill_scans: w.fill_scans - work.fill_scans,
                fill_runs: w.fill_runs - work.fill_runs,
                fill_seeds: w.fill_seeds - work.fill_seeds,
            };
            self.add_drawing_work(&delta, p - plots);
            (work, plots) = (w, p);
            self.charge();
            self.clock.pace(self.cycles);
            self.frame_due();
        }
    }

    /// `$34F5`: op 06: the loaded room picture: both pages cleared, the frame, the
    /// picture, then its colours merged into the work page. Skipped while restoring.
    pub fn draw_room_picture(&mut self) {
        self.checkpoint(0x34F5);
        if self.g.restoring {
            self.cost.pictures_done += 1;
            return;
        }
        if let Some(t) = self.drawing_trace.as_mut() {
            t.push(Vec::new());
            self.drawing_room = true;
        }
        self.screen.shown.fill(0xFF);
        self.screen.work.fill(0x11);
        // The picture covers any characters above the text window.
        for row in &mut self.text.graphics.visible[..21] {
            *row = [false; 40];
        }
        let frame = self.res.ds[pic::FRAME..pic::FRAME + 11].to_vec();
        self.interpret(&frame);
        self.draw_loaded_picture();
        self.drawing_room = false;
        self.merge_colours();
        self.cost.pictures_done += 1;
    }

    /// `$3534`: the loaded picture, over what is there.
    pub fn draw_loaded_picture(&mut self) {
        let data: Rc<Vec<u8>> = self.g.picture.clone().unwrap_or_default();
        self.interpret(&data);
    }

    /// `$064C`: the displayed page's colours into the work page's high nibbles.
    pub fn merge_colours(&mut self) {
        for (w, s) in self.screen.work.iter_mut().zip(&self.screen.shown) {
            *w = (s & 0xF0) | (*w & 0x0F);
        }
    }
}
