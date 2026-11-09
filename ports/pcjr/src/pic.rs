//! The PCjr picture painter: the engine's picture interpreter (`$353F`) and its line,
//! plot and fill routines, modelled literally on the two video pages they draw into.
//! The commands are the Apple version's (ports/apple2/docs/picture-format.md); the
//! drawing is the PCjr's own (docs/picture-format.md).
//!
//! The pages: the visual picture at 1800:0 and the priorities at 1000:0, each in mode
//! 9's layout (four banks of `0x2000`, 160 bytes a row; row y at `(y & 3) * 0x2000 +
//! (y >> 2) * 160`). A picture pixel is one byte holding its colour in both halves.

/// The picture: 160 × 168.
pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 168;

/// The frame the engine draws before every room picture (`$34FF`: the sequence at
/// `DS:$1981`, in the engine's data segment): a black, priority-0 rectangle round the
/// edge. Fills stop at it.
pub const FRAME: usize = 0x1981;

/// What the drawing did, for the speed model: line steps (the absolute line's, by
/// error terms; the others', by a multiply and a divide each), lines drawn, and the
/// fills' bytes written, bytes scanned for runs, runs filled and seeds tried.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Work {
    pub abs_steps: u32,
    pub to_steps: u32,
    pub lines: u32,
    pub fill_bytes: u32,
    pub fill_scans: u32,
    pub fill_runs: u32,
    pub fill_seeds: u32,
}

impl Work {
    /// This and `o` added up.
    pub fn plus(&self, o: &Work) -> Work {
        Work {
            abs_steps: self.abs_steps + o.abs_steps,
            to_steps: self.to_steps + o.to_steps,
            lines: self.lines + o.lines,
            fill_bytes: self.fill_bytes + o.fill_bytes,
            fill_scans: self.fill_scans + o.fill_scans,
            fill_runs: self.fill_runs + o.fill_runs,
            fill_seeds: self.fill_seeds + o.fill_seeds,
        }
    }
}

pub struct Picture {
    /// 168 rows of 160: priority << 4 | colour (the Apple painter's layout).
    pub buf: Vec<u8>,
    /// Pixels plotted by lines and fills.
    pub plots: u32,
    /// Bytes of the picture read, up to and including its end.
    pub end: usize,
    pub warnings: Vec<String>,
    pub work: Work,
}

impl Picture {
    pub fn visual(&self, x: usize, y: usize) -> u8 {
        self.buf[y * WIDTH + x] & 0x0F
    }
    pub fn priority(&self, x: usize, y: usize) -> u8 {
        self.buf[y * WIDTH + x] >> 4
    }
}

/// What the painter draws into and reads the picture from: the engine's memory
/// (engine/picture.rs), or a pair of pages of its own ([`draw_room`]).
pub trait Mem {
    /// A byte at `seg:off`.
    fn read(&self, seg: u32, off: u16) -> u8;
    fn write(&mut self, seg: u32, off: u16, v: u8);
    /// Byte `i` of the picture being drawn, or `None` past its end.
    fn data(&self, i: usize) -> Option<u8>;
    /// The drawing's work so far, before each plot of a line and after each run of a
    /// fill: for a front end that shows the picture as it is drawn.
    fn progress(&mut self, _work: &Work, _plots: u32) {}
}

/// The interpreter's state the engine keeps in its data segment (`$198C`-`$1993`).
#[derive(Clone, Copy, Default, Debug)]
pub struct State {
    pub mode: u8,
    pub toggle: u8,
    pub vis: u8,
    pub pri: u8,
    pub y: u8,
    pub x: u8,
    pub ty: u8,
    pub tx: u8,
}

/// Linear addresses of the two pages, as segment * 16 (the engine's segments 1000 and
/// 1800).
const PRI_SEG: u32 = 0x1000;
const VIS_SEG: u32 = 0x1800;

/// The address of row y in a page (the table at `DS:$1828`).
fn row(y: u8) -> u16 {
    let y = y as u16;
    (y & 3) * 0x2000 + (y >> 2) * 160
}

/// Pages of its own: memory from linear `0x10000` to `0x1FFFF`, the priority page
/// then the visual page. Outside it reads give `FF` and writes are lost.
struct Pages<'a> {
    mem: Vec<u8>,
    data: &'a [u8],
}

impl Mem for Pages<'_> {
    fn read(&self, seg: u32, off: u16) -> u8 {
        let a = seg * 16 + off as u32;
        match a {
            0x10000..=0x1FFFF => self.mem[(a - 0x10000) as usize],
            _ => 0xFF,
        }
    }
    fn write(&mut self, seg: u32, off: u16, v: u8) {
        let a = seg * 16 + off as u32;
        if (0x10000..=0x1FFFF).contains(&a) {
            self.mem[(a - 0x10000) as usize] = v;
        }
    }
    fn data(&self, i: usize) -> Option<u8> {
        self.data.get(i).copied()
    }
}

struct Painter<'a> {
    m: &'a mut dyn Mem,
    pos: usize,
    /// `[$198C]`: 0 for F4-F7 (both pages); `FF` for F8-FB and FC; 1 for FE; `80` for FD.
    mode: u8,
    /// `[$198D]`: toggled by F0.
    toggle: u8,
    /// `[$198E]`, `[$198F]`: the visual and priority colours, in both halves of a byte.
    vis: u8,
    pri: u8,
    /// `[$1990]` = y, `[$1991]` = x: the current point.
    y: u8,
    x: u8,
    /// `[$1992]` = y, `[$1993]` = x: the target point of a line.
    ty: u8,
    tx: u8,
    plots: u32,
    warnings: Vec<String>,
    work: Work,
}

impl Painter<'_> {
    fn read(&self, seg: u32, off: u16) -> u8 {
        self.m.read(seg, off)
    }
    fn write(&mut self, seg: u32, off: u16, v: u8) {
        self.m.write(seg, off, v)
    }
    fn next(&mut self) -> u8 {
        let b = self.m.data(self.pos).unwrap_or_else(|| {
            self.warnings.push("picture runs past its end".into());
            0xFF
        });
        self.pos += 1;
        b
    }

    /// `$640`: a colour in both halves of the byte.
    fn both(c: u8) -> u8 {
        (c << 4) | (c & 0x0F)
    }

    /// `$3684`: read a point (x clamped to 159, y to 167); None at a command byte, which
    /// is returned for the dispatcher.
    fn point(&mut self) -> Result<(), u8> {
        let x = self.next();
        if x > 0xEF {
            return Err(x);
        }
        self.x = x.min(0x9F);
        self.y = self.next().min(0xA7);
        Ok(())
    }

    /// `$3792` + `$6BA`: plot at (x, y) by the mode and toggle.
    fn plot_at(&mut self, x: u8, y: u8) {
        let di = row(y).wrapping_add(x as u16);
        self.m.progress(&self.work, self.plots);
        self.plots += 1;
        if self.mode & 1 != 0 {
            if self.toggle & 1 != 0 {
                self.write(VIS_SEG, di, self.vis);
                return;
            }
        } else {
            self.write(VIS_SEG, di, self.vis);
        }
        self.write(PRI_SEG, di, self.pri);
    }

    fn plot(&mut self) {
        self.plot_at(self.x, self.y);
    }

    /// `$36C4`: the absolute line (F6), stepped by error terms in 8-bit registers.
    fn line_abs(&mut self) {
        self.tx = self.tx.min(0x9F);
        self.ty = self.ty.min(0xA7);
        let (mut by, mut bx) = (self.y, self.x);
        let (dy, ystep) = if self.ty >= by { (self.ty - by, 1u8) } else { (by - self.ty, 0xFF) };
        let (dx, xstep) = if self.tx >= bx { (self.tx - bx, 1u8) } else { (bx - self.tx, 0xFF) };
        let (n, mut al, mut ah) = if dx >= dy { (dx, 0u8, dx >> 1) } else { (dy, dy >> 1, 0u8) };
        let mut cx = n as u16;
        self.work.lines += 1;
        loop {
            self.work.abs_steps += 1;
            ah = ah.wrapping_add(dy);
            if ah >= n {
                ah = ah.wrapping_sub(n);
                by = by.wrapping_add(ystep);
            }
            al = al.wrapping_add(dx);
            if al >= n {
                al = al.wrapping_sub(n);
                bx = bx.wrapping_add(xstep);
            }
            self.y = by;
            self.x = bx;
            self.plot();
            if cx == 0 {
                break;
            }
            cx -= 1;
            if cx == 0 {
                break;
            }
        }
    }

    /// `$37AC`: the line to the target used by the corner and relative lines (F4, F5,
    /// F7), stepped by multiplying and dividing; the current point becomes the target.
    fn line_to(&mut self) {
        self.tx = self.tx.min(0x9F);
        self.ty = self.ty.min(0xA7);
        let (xplus, dx) = if self.tx > self.x { (true, self.tx - self.x) } else { (false, self.x - self.tx) };
        let (yplus, dy) = if self.ty > self.y { (true, self.ty - self.y) } else { (false, self.y - self.ty) };
        let at = |s: &Self, ox: u8, oy: u8| -> (u8, u8) {
            let x = if xplus { s.x.wrapping_add(ox) } else { s.x.wrapping_sub(ox) };
            let y = if yplus { s.y.wrapping_add(oy) } else { s.y.wrapping_sub(oy) };
            (x, y)
        };
        // offset * other / major, rounded up when twice the remainder (in 8 bits)
        // passes the divisor.
        let step = |i: u8, other: u8, major: u8| -> u8 {
            let p = i as u16 * other as u16;
            let (mut q, r) = ((p / major as u16) as u8, (p % major as u16) as u8);
            if r.wrapping_shl(1) > major {
                q = q.wrapping_add(1);
            }
            q
        };
        self.work.lines += 1;
        if dx > dy {
            // $381F
            let mut i = 0u8;
            while i != dx {
                i = i.wrapping_add(1);
                let oy = step(i, dy, dx);
                self.work.to_steps += 1;
                let (x, y) = at(self, i, oy);
                self.plot_at(x, y);
            }
        } else if dy != 0 {
            // $3858
            let mut i = 0u8;
            while i != dy {
                i = i.wrapping_add(1);
                let ox = step(i, dx, dy);
                self.work.to_steps += 1;
                let (x, y) = at(self, ox, i);
                self.plot_at(x, y);
            }
        }
        self.x = self.tx;
        self.y = self.ty;
    }

    /// `$794`: the run of `dh` bytes through `di`: (its left end, its length).
    fn span(&mut self, seg: u32, di: u16, dh: u8) -> (u16, u16) {
        // Backward: REPE SCASB with DF set, at most 255 bytes.
        let (mut d, mut cx) = (di, 0xFFu16);
        while cx != 0 {
            let v = self.read(seg, d);
            self.work.fill_scans += 1;
            d = d.wrapping_sub(1);
            cx -= 1;
            if v != dh {
                break;
            }
        }
        let left = d.wrapping_add(2);
        // Forward from there, at most 255 bytes.
        let (mut d, mut cx) = (left, 0xFFu16);
        while cx != 0 {
            let v = self.read(seg, d);
            self.work.fill_scans += 1;
            d = d.wrapping_add(1);
            cx -= 1;
            if v != dh {
                break;
            }
        }
        (left, d.wrapping_sub(1).wrapping_sub(left))
    }

    /// `$7AF`: from a seed, the run to fill: the seed's own run if it is still fillable,
    /// else the run at the next fillable byte within `count` bytes forward (the width of
    /// the run it was queued from, `$7B8`).
    fn seed_run(&mut self, seg: u32, di: u16, count: u16, dh: u8) -> Option<(u16, u16)> {
        self.work.fill_seeds += 1;
        if self.read(seg, di) == dh {
            return Some(self.span(seg, di, dh));
        }
        let (mut d, mut cx) = (di, count);
        while cx != 0 {
            let v = self.read(seg, d);
            self.work.fill_scans += 1;
            d = d.wrapping_add(1);
            cx -= 1;
            if v == dh {
                break;
            }
        }
        let d = d.wrapping_sub(1);
        (self.read(seg, d) == dh).then(|| self.span(seg, d, dh))
    }

    /// `$6DC` + `$6F7`: fill from (x, y).
    fn fill(&mut self) {
        let di = row(self.y).wrapping_add(self.x as u16);
        let (seg, dh, colour) = if self.mode & 1 != 0 { (VIS_SEG, 0xFF, self.vis) } else { (PRI_SEG, 0x11, self.pri) };
        if colour == dh || self.read(seg, di) != dh {
            return;
        }
        // The stack of seeds: (the width of the run it was queued from, address); a (0, 0)
        // entry at the bottom.
        let mut stack: Vec<(u16, u16)> = vec![(0, 0)];
        let (mut left, mut len) = self.span(seg, di, dh);
        let mut guard = 0u32;
        'run: loop {
            guard += 1;
            if guard > 200_000 {
                self.warnings.push("fill did not finish".into());
                return;
            }
            // $731: queue the rows below and above the run's left end, unless that is the
            // entry on top of the stack.
            let top = *stack.last().unwrap();
            let mut below = left.wrapping_add(0x2000);
            if below >= 0x8000 {
                below = below.wrapping_sub(0x7F60);
            }
            if top != (len, below) {
                stack.push((len, below));
            }
            let above = match left.checked_sub(0x2000) {
                Some(a) => a,
                None => left.wrapping_sub(0x2000).wrapping_add(0x7F60),
            };
            if top != (len, above) {
                stack.push((len, above));
            }
            // Fill the run, and the priority page too for FC.
            self.work.fill_runs += 1;
            self.work.fill_bytes += len as u32;
            for k in 0..len {
                self.write(seg, left.wrapping_add(k), colour);
                self.plots += 1;
            }
            if self.mode == 0xFF {
                for k in 0..len {
                    self.write(PRI_SEG, left.wrapping_add(k), self.pri);
                }
            }
            self.m.progress(&self.work, self.plots);
            // $77C: try the seeds on the stack, newest first.
            loop {
                let (width, a) = *stack.last().unwrap();
                if width == 0 {
                    return;
                }
                if let Some((l, n)) = self.seed_run(seg, a, width, dh) {
                    left = l;
                    len = n;
                    continue 'run;
                }
                stack.pop();
            }
        }
    }
}

/// Draw `data` as the engine draws a room picture: both pages cleared (`$615`: visual
/// `FF`, priority `11`), the frame from the engine's data segment, then the picture.
/// `frame` is the frame sequence (`vocab::DATA_SEGMENT + FRAME` in the code segment).
pub fn draw_room(frame: &[u8], data: &[u8]) -> Picture {
    let mut mem = vec![0x11u8; 0x10000];
    mem[0x8000..].fill(0xFF);
    let mut pages = Pages { mem, data: frame };
    let mut state = State::default();
    draw(&mut pages, &mut state);
    pages.data = data;
    let mut p = draw(&mut pages, &mut state);
    p.buf = buffer(&pages.mem);
    p
}

/// The two pages as a 160 × 168 buffer, priority << 4 | colour.
fn buffer(mem: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT as u8 {
        let r = row(y) as usize;
        for x in 0..WIDTH {
            buf.push((mem[r + x] & 0x0F) << 4 | (mem[0x8000 + r + x] & 0x0F));
        }
    }
    buf
}

/// Run the interpreter (`$353F`) on `m`, from and back to the engine's `state`. The F0
/// toggle starts each picture cleared.
pub fn draw(m: &mut dyn Mem, state: &mut State) -> Picture {
    let mut p = Painter {
        m,
        pos: 0,
        mode: state.mode,
        toggle: 0,
        vis: state.vis,
        pri: state.pri,
        y: state.y,
        x: state.x,
        ty: state.ty,
        tx: state.tx,
        plots: 0,
        warnings: Vec::new(),
        work: Work::default(),
    };
    let mut op = p.next();
    loop {
        // $354E: FF ends; a byte below F0 stops the picture.
        if op == 0xFF {
            break;
        }
        if op < 0xF0 {
            p.warnings.push(format!("stopped on non-command byte {op:02X} at {}", p.pos - 1));
            break;
        }
        op = match op {
            0xF0 => {
                p.toggle ^= 0xFF;
                p.next()
            }
            0xF1 => {
                p.vis = Painter::both(p.next());
                p.next()
            }
            0xF2 => {
                p.pri = Painter::both(p.next());
                p.next()
            }
            0xF3 => {
                p.vis = Painter::both(p.next());
                p.pri = Painter::both(p.next());
                p.next()
            }
            // Corners: F4/F8 start with a y move, F5/F9 with an x move.
            0xF4 | 0xF5 | 0xF8 | 0xF9 => {
                p.mode = if op >= 0xF8 { 0xFF } else { 0 };
                match p.point() {
                    Err(b) => b,
                    Ok(()) => {
                        p.plot();
                        let mut y_first = op == 0xF4 || op == 0xF8;
                        loop {
                            let v = p.next();
                            if v > 0xEF {
                                break v;
                            }
                            if y_first {
                                p.ty = v;
                                p.tx = p.x;
                            } else {
                                p.tx = v;
                                p.ty = p.y;
                            }
                            p.line_to();
                            y_first = !y_first;
                        }
                    }
                }
            }
            0xF6 | 0xFA => {
                p.mode = if op == 0xFA { 0xFF } else { 0 };
                match p.point() {
                    Err(b) => b,
                    Ok(()) => {
                        p.plot();
                        loop {
                            let v = p.next();
                            if v > 0xEF {
                                break v;
                            }
                            p.tx = v;
                            p.ty = p.next();
                            p.line_abs();
                        }
                    }
                }
            }
            0xF7 | 0xFB => {
                p.mode = if op == 0xFB { 0xFF } else { 0 };
                match p.point() {
                    Err(b) => b,
                    Ok(()) => {
                        p.plot();
                        loop {
                            let v = p.next();
                            if v > 0xEF {
                                break v;
                            }
                            // $3621: x moves by bits 4-6 (sign bit 7), y by bits 0-2 (sign 3).
                            let (mx, my) = ((v & 0x70) >> 4, v & 7);
                            p.tx = if v & 0x80 != 0 { p.x.wrapping_sub(mx) } else { p.x.wrapping_add(mx) };
                            p.ty = if v & 0x08 != 0 { p.y.wrapping_sub(my) } else { p.y.wrapping_add(my) };
                            p.line_to();
                        }
                    }
                }
            }
            0xFC..=0xFE => {
                match op {
                    0xFE => {
                        p.mode = 1;
                        p.vis = Painter::both(p.next());
                    }
                    0xFD => {
                        p.mode = 0x80;
                        p.pri = Painter::both(p.next());
                    }
                    _ => {
                        p.mode = 0xFF;
                        p.vis = Painter::both(p.next());
                        p.pri = Painter::both(p.next());
                    }
                }
                loop {
                    match p.point() {
                        Err(b) => break b,
                        Ok(()) => p.fill(),
                    }
                }
            }
            _ => unreachable!(),
        };
    }
    *state = State { mode: p.mode, toggle: p.toggle, vis: p.vis, pri: p.pri, y: p.y, x: p.x, ty: p.ty, tx: p.tx };
    Picture { buf: Vec::new(), plots: p.plots, end: p.pos, warnings: p.warnings, work: p.work }
}
