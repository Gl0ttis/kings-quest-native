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

use super::World;
use crate::pic::{self, Mem};
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
            } else {
                self.s.work[i] = v;
            }
        }
    }
    fn data(&self, i: usize) -> Option<u8> {
        self.data.get(i).copied()
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
    /// `$353F`: draw `data` onto the pages.
    fn interpret(&mut self, data: &[u8]) {
        let mut p = Pages { s: &mut self.screen, data };
        let r = pic::draw(&mut p, &mut self.g.pic);
        self.cost.plots += r.plots;
        let (c, w) = (&mut self.cost.pic, r.work);
        c.abs_steps += w.abs_steps;
        c.to_steps += w.to_steps;
        c.lines += w.lines;
        c.fill_bytes += w.fill_bytes;
        c.fill_scans += w.fill_scans;
        c.fill_runs += w.fill_runs;
        c.fill_seeds += w.fill_seeds;
    }

    /// `$34F5`: op 06: the loaded room picture: both pages cleared, the frame, the
    /// picture, then its colours merged into the work page. Skipped while restoring.
    pub fn draw_room_picture(&mut self) {
        self.checkpoint(0x34F5);
        if self.g.restoring {
            return;
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
        self.merge_colours();
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
