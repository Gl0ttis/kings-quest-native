//! The picture and the screen (`$14B1`, `$48E3`, `$491E`), in two planes, as the PCjr
//! port keeps them:
//!
//! - **The displayed plane:** what the screen shows of the picture, a colour a pixel
//!   (the frame turns each into its dot pattern, as the PC's CGA holds it: `World::frame`).
//! - **The work plane:** the PC's picture buffer (segment 1400h, its byte the pattern and
//!   the priority), here the colour in the high nibble and the priority in the low one;
//!   while a picture is drawn, its priorities. The actors are drawn into it, and changed
//!   rectangles copied from it to the screen (`$1875`).
//!
//! Both are 160 × 168, the picture's size. The painter (pic.rs) addresses them in the
//! buffer's layout (four banks, 160 bytes a row); [`Screen`] maps that to rows and
//! columns.

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
    /// The second edition's RGB colours, when chosen: the planes then hold each pixel's
    /// dot pattern, as the original's picture buffer does, not its colour.
    pub rgb: Option<Rgb>,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { work: vec![0; WIDTH * HEIGHT], shown: vec![0; WIDTH * HEIGHT], rgb: None }
    }
}

/// The second edition's RGB patterns (its own data). The picture's: three bytes a
/// colour from DS:12C8 (`$17F8`), the composite pattern and then the RGB patterns for
/// odd and even rows (the plot takes the first on an odd row, `$4C3D`). The actors':
/// one a colour, DS:1308 (`$1890`, the views' colours turned into patterns as they are
/// loaded). Each byte holds its pattern in both halves.
#[derive(Clone, Copy, Debug)]
pub struct Rgb {
    pub picture: [[u8; 2]; 16],
    pub cel: [u8; 16],
}

impl Rgb {
    pub fn from_data(ds: &[u8]) -> Rgb {
        Rgb {
            picture: std::array::from_fn(|c| [ds[0x12C8 + 3 * c + 1] & 0x0F, ds[0x12C8 + 3 * c + 2] & 0x0F]),
            cel: std::array::from_fn(|c| ds[0x1308 + c] & 0x0F),
        }
    }

    /// The pattern the picture's colour `c` is drawn in on row `y`.
    pub fn picture(&self, c: u8, y: usize) -> u8 {
        self.picture[(c & 15) as usize][if y & 1 == 1 { 0 } else { 1 }]
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
    /// The writes counted as the PC's CGA takes them: a byte holds two of the
    /// picture's pixels, and both written one after the other are one write. At each
    /// mark (for timing the drawing against the original's, `drawing_trace`).
    bytes: usize,
    last_byte: Option<usize>,
    mark_bytes: Vec<usize>,
}

impl Mem for Pages<'_> {
    fn read(&self, seg: u32, off: u16) -> u8 {
        let plane = if seg == 0x1800 { &self.s.shown } else { &self.s.work };
        at(off).map_or(0, |i| plane[i])
    }
    fn write(&mut self, seg: u32, off: u16, v: u8) {
        if let Some(i) = at(off) {
            if seg == 0x1800 {
                // RGB: the pattern of the colour on this row (`$4C3D`), in both halves.
                let v = match &self.s.rgb {
                    Some(t) => {
                        let p = t.picture(v, i / WIDTH);
                        p << 4 | p
                    }
                    None => v,
                };
                self.s.shown[i] = v;
                if let Some(l) = self.log.as_mut() {
                    l.writes.push((i, v));
                    let byte = i / 2;
                    if l.last_byte != Some(byte) {
                        l.bytes += 1;
                    }
                    l.last_byte = Some(byte);
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
            l.mark_bytes.push(l.bytes);
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
    /// `$492A`: draw `data` onto the pages. The original draws on the displayed page,
    /// so with a front end watching (`on_frame`), the drawing is shown as it goes: the
    /// displayed page's writes are played back in time, its work charged as it is done.
    fn interpret(&mut self, data: &[u8]) {
        let mut log = self.on_frame.is_some().then(DrawLog::default);
        let before = log.as_ref().map(|_| self.screen.shown.clone());
        let mut p = Pages { s: &mut self.screen, data, log: log.as_mut() };
        let r = pic::draw(&mut p, &mut self.g.pic);
        if let (Some(mut log), Some(before)) = (log, before) {
            log.marks.push((log.writes.len(), r.work, r.plots));
            log.mark_bytes.push(log.bytes);
            if let (true, Some(t)) = (self.drawing_room, self.drawing_trace.as_mut()) {
                if let Some(pic) = t.last_mut() {
                    // The frame and the picture: one drawing, their work added up.
                    let (n0, w0) = pic.last().copied().unwrap_or_default();
                    pic.extend(log.marks.iter().zip(&log.mark_bytes).map(|(&(_, w, _), &n)| (n0 + n, w0.plus(&w))));
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

    /// `$48E3`: op 06: the loaded room picture: both pages cleared, the frame, the
    /// picture, then its colours merged into the work page. Skipped while restoring.
    pub fn draw_room_picture(&mut self) {
        self.checkpoint(0x48E3);
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
        let at = self.res.d(pic::FRAME);
        let frame = self.res.ds[at..at + 11].to_vec();
        self.interpret(&frame);
        self.draw_loaded_picture();
        self.drawing_room = false;
        self.merge_colours();
        self.cost.pictures_done += 1;
    }

    /// `$491E`: the loaded picture, over what is there.
    pub fn draw_loaded_picture(&mut self) {
        let data: Rc<Vec<u8>> = self.g.picture.clone().unwrap_or_default();
        self.interpret(&data);
    }

    /// `$14DC`: the picture just drawn on the screen (B800h: the PC draws a room's picture
    /// onto the screen itself) merged with its priorities into the buffer (1400h): here,
    /// the displayed plane's colours into the work plane's high nibbles.
    pub fn merge_colours(&mut self) {
        for (w, s) in self.screen.work.iter_mut().zip(&self.screen.shown) {
            *w = (s & 0xF0) | (*w & 0x0F);
        }
    }
}
