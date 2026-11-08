//! Picture interpreter for King's Quest (Apple II). Behaviour follows the aux-memory
//! routines loaded from fixed resource `aux_09C0_code` (addresses below are aux addresses).
//!
//! The engine draws into a 140x160 byte buffer at aux $4000 (row stride 140, $0F01):
//! low nibble = visual colour (an Apple double hi-res colour value), high nibble =
//! priority. The buffer starts as $1F everywhere ($0EE1). The engine writes the visual
//! nibble to the double hi-res screen at the same time; a port only needs the buffer.

pub const WIDTH: usize = 140;
pub const HEIGHT: usize = 160;

/// Picture colour index -> Apple DHGR colour value (aux $16E0, first 16 entries).
pub const COLOR_TABLE: [u8; 16] =
    [0x00, 0x02, 0x04, 0x0E, 0x01, 0x03, 0x08, 0x0A, 0x05, 0x06, 0x0C, 0x07, 0x0B, 0x03, 0x0D, 0x0F];

pub struct Picture {
    /// (priority << 4) | visual, row-major, WIDTH * HEIGHT.
    pub buf: Vec<u8>,
    /// Non-fatal oddities seen while drawing (e.g. colour index outside the table).
    pub warnings: Vec<String>,
    /// Offset just past the terminating $FF. Stored resources often carry a few
    /// unused bytes after it (all 83 pictures end cleanly on $FF).
    pub end: usize,
    /// Pixels plotted, by lines and fills (calls of aux $102F): the drawing work.
    pub plots: u32,
    /// Of those, the ones plotted by fills (which cost the original more each).
    pub fill_plots: u32,
    /// The drawing's runs in order: (plots before the run, is it a fill), a new run at
    /// each line segment and each fill (seed point) that plots anything. With per-kind costs it gives the
    /// time at which any plot is reached.
    pub runs: Vec<(u32, bool)>,
}

impl Picture {
    pub fn visual(&self, x: usize, y: usize) -> u8 {
        self.buf[y * WIDTH + x] & 0x0F
    }
    pub fn priority(&self, x: usize, y: usize) -> u8 {
        self.buf[y * WIDTH + x] >> 4
    }
}

struct Painter<'a> {
    data: &'a [u8],
    pos: usize,
    buf: Vec<u8>,
    c0: u8, // alternate write mask, toggled by F0 (initial $F0, $0BAA)
    c3: u8, // current colour byte: (priority << 4) | visual
    c4: u8, // current write mask
    f0: u8, // current x
    f1: u8, // current y
    c5: u8, // target x
    c6: u8, // target y
    warnings: Vec<String>,
    overrun: bool,
    plots: u32,
    fill_plots: u32,
    runs: Vec<(u32, bool)>,
    new_command: bool,
    /// For draw_partial: the buffer as it stood after `limit` plots.
    shown: Option<Vec<u8>>,
    limit: u32,
}

impl Painter<'_> {
    /// $0CF9. Reading past the resource returns $FF (end) and records the overrun.
    fn next(&mut self) -> u8 {
        match self.data.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                b
            }
            None => {
                self.overrun = true;
                0xFF
            }
        }
    }

    /// $1079: logical coordinates are offset by (10, 3); returns the buffer index.
    fn index(&self, x: u8, y: u8) -> Option<usize> {
        let x = x.checked_sub(10)? as usize;
        let y = y.checked_sub(3)? as usize;
        (x < WIDTH && y < HEIGHT).then(|| y * WIDTH + x)
    }

    /// $102A
    fn plot(&mut self) {
        if let Some(i) = self.index(self.f0, self.f1) {
            self.watch(i, "plot");
            self.buf[i] = (self.buf[i] & !self.c4) | (self.c4 & self.c3);
            self.run(false);
            self.plots += 1;
            self.show(i);
        }
    }

    /// Note the start of a run of line or fill plots.
    fn run(&mut self, fill: bool) {
        if self.new_command || self.runs.last().is_none_or(|r| r.1 != fill) {
            self.runs.push((self.plots, fill));
            self.new_command = false;
        }
        if fill {
            self.fill_plots += 1;
        }
    }

    /// Copy a write to the partial view while it is within the plot limit.
    fn show(&mut self, i: usize) {
        if self.plots <= self.limit {
            if let Some(s) = self.shown.as_mut() {
                s[i] = self.buf[i];
            }
        }
    }

    /// Debugging aid: KQ_PIC_WATCH="x,y" reports writes to that picture pixel.
    fn watch(&self, i: usize, what: &str) {
        if let Ok(v) = std::env::var("KQ_PIC_WATCH") {
            if let Some((x, y)) = v.split_once(',') {
                if let (Ok(x), Ok(y)) = (x.parse::<usize>(), y.parse::<usize>()) {
                    if i == (y - 3) * WIDTH + (x - 10) {
                        eprintln!(
                            "pic watch ({x},{y}): {what} at data offset {} colour byte {:02X} mask {:02X} from ({},{})",
                            self.pos, self.c3, self.c4, self.f0, self.f1
                        );
                    }
                }
            }
        }
    }

    /// $0D26 (the table value is ORed in whole, as the engine does)
    fn set_visual(&mut self, c: u8) {
        let v = match COLOR_TABLE.get(c as usize) {
            Some(&v) => v,
            None => {
                self.warnings.push(format!("colour index {c} outside 16-entry table at offset {}", self.pos - 1));
                0
            }
        };
        self.c3 = (self.c3 & 0xF0) | v;
    }

    /// $0D34
    fn set_priority(&mut self, p: u8) {
        self.c3 = (self.c3 & 0x0F) | (p << 4);
    }

    /// $0D0E: read an (x, y) pair into both current and target. Err(op) on an opcode byte.
    fn read_point(&mut self) -> Result<(), u8> {
        let x = self.next();
        if x >= 0xF0 {
            return Err(x);
        }
        self.f0 = x;
        self.c5 = x;
        let y = self.next();
        if y >= 0xF0 {
            return Err(y);
        }
        self.f1 = y;
        self.c6 = y;
        Ok(())
    }

    /// $0F63: draw from (f0, f1) to (c5, c6), not re-plotting the start in the general case.
    fn line(&mut self) {
        self.new_command = true;
        if self.f0 == self.c5 {
            // $1001: vertical
            loop {
                self.plot();
                if self.f1 == self.c6 {
                    return;
                }
                self.f1 = if self.f1 < self.c6 { self.f1 + 1 } else { self.f1 - 1 };
            }
        }
        if self.f1 == self.c6 {
            // $1016: horizontal
            loop {
                self.plot();
                if self.f0 == self.c5 {
                    return;
                }
                self.f0 = if self.f0 < self.c5 { self.f0 + 1 } else { self.f0 - 1 };
            }
        }
        let (dy, sy) = if self.c6 >= self.f1 { (self.c6 - self.f1, 1u8) } else { (self.f1 - self.c6, 0xFF) };
        let (dx, sx) = if self.c5 >= self.f0 { (self.c5 - self.f0, 1u8) } else { (self.f0 - self.c5, 0xFF) };
        let (mut n, ce, mut ca, mut c7) = if dx >= dy { (dx, dx, dx >> 1, 0u8) } else { (dy, dy, 0u8, dy >> 1) };
        loop {
            // 8-bit accumulators: the ADC carry is discarded before the CMP, as in the engine.
            ca = ca.wrapping_add(dy);
            if ca >= ce {
                ca = ca.wrapping_sub(ce);
                self.f1 = self.f1.wrapping_add(sy);
            }
            c7 = c7.wrapping_add(dx);
            if c7 >= ce {
                c7 = c7.wrapping_sub(ce);
                self.f0 = self.f0.wrapping_add(sx);
            }
            self.plot();
            n = n.wrapping_sub(1);
            if n == 0 {
                return;
            }
        }
    }

    /// $0D47: flood fill from (f0, f1), transcribed from the engine's stack-based scanline
    /// fill ($0D7E-$0E8E). Each stack entry is [right end, left, cursor, y, how it was
    /// pushed]. A span is filled rightward from its cursor, then leftward from cursor - 1;
    /// then the row above is scanned from the cursor, and the row below from the left end,
    /// pushing a child entry at each fillable pixel found.
    ///
    /// The leftward scan ends with `DEY; BPL`, so it continues only while the x index
    /// stays below 128: from x index 129 or more it fills a single pixel and stops. That
    /// quirk leaves occasional unfilled pixels near the right edge, as in the original.
    fn fill(&mut self) {
        self.new_command = true;
        let (target, tmask) = if self.c4 == 0xF0 {
            if self.c3 & 0xF0 == 0x10 {
                return;
            }
            (0x10u8, 0xF0u8)
        } else {
            if self.c3 & 0x0F == 0x0F {
                return;
            }
            (0x0Fu8, 0x0Fu8)
        };
        // $0D6B: the seed must be on screen and fillable.
        match self.index(self.f0, self.f1) {
            Some(i) if self.buf[i] & tmask == target => {}
            _ => return,
        }
        let fillable =
            |p: &Painter, x: u8, y: u8| -> Option<bool> { p.index(x, y).map(|i| p.buf[i] & tmask == target) };
        // [0] right end, [1] left, [2] cursor, [3] y, [4] 1 = from above, 2 = from below, $FF = seed
        let mut stack: Vec<[u8; 5]> = vec![[0, 0, self.f0, self.f1, 0xFF]];
        enum S {
            Process,
            Above,
            Below,
            Pop,
        }
        let mut st = S::Process;
        let mut guard = 0u32;
        loop {
            guard += 1;
            if guard > 2_000_000 {
                self.warnings.push("fill did not finish".into());
                return;
            }
            let top = stack.len() - 1;
            st = match st {
                S::Process => {
                    // $0D7E-$0D96
                    let y = stack[top][3];
                    // $0E47: rightward from the cursor.
                    let mut x = stack[top][2];
                    if self.index(x, y).is_some() {
                        while fillable(self, x, y) == Some(true) {
                            self.f0 = x;
                            self.f1 = y;
                            self.fill_plot(x, y);
                            x = x.wrapping_add(1);
                            if x.wrapping_sub(10) >= WIDTH as u8 {
                                break;
                            }
                        }
                    }
                    stack[top][0] = x;
                    // $0E6B: leftward from cursor - 1; continues only while the index < 128.
                    let mut x = stack[top][2].wrapping_sub(1);
                    if self.index(x, y).is_some() {
                        loop {
                            if fillable(self, x, y) != Some(true) {
                                break;
                            }
                            self.fill_plot(x, y);
                            let idx = x.wrapping_sub(10);
                            x = x.wrapping_sub(1);
                            if idx.wrapping_sub(1) & 0x80 != 0 {
                                break;
                            }
                        }
                    }
                    let left = x.wrapping_add(1);
                    stack[top][2] = left;
                    stack[top][1] = left;
                    S::Above
                }
                S::Above => {
                    // $0D98
                    let e = stack[top];
                    if e[2] >= e[0] {
                        S::Below
                    } else {
                        match fillable(self, e[2], e[3].wrapping_sub(1)) {
                            None => S::Below,
                            Some(true) => {
                                stack.push([0, 0, e[2], e[3].wrapping_sub(1), 1]);
                                S::Process
                            }
                            Some(false) => {
                                stack[top][2] = e[2].wrapping_add(1);
                                S::Above
                            }
                        }
                    }
                }
                S::Below => {
                    // $0E0F
                    let e = stack[top];
                    if e[1] >= e[0] {
                        S::Pop
                    } else {
                        match fillable(self, e[1], e[3].wrapping_add(1)) {
                            None => S::Pop,
                            Some(true) => {
                                stack.push([0, 0, e[1], e[3].wrapping_add(1), 2]);
                                S::Process
                            }
                            Some(false) => {
                                stack[top][1] = e[1].wrapping_add(1);
                                S::Below
                            }
                        }
                    }
                }
                S::Pop => {
                    // $0DEF: the parent resumes after the child's right end.
                    let e = stack.pop().unwrap();
                    match (e[4], stack.last_mut()) {
                        (1, Some(parent)) => {
                            parent[2] = e[0];
                            S::Above
                        }
                        (2, Some(parent)) => {
                            parent[1] = e[0];
                            S::Below
                        }
                        _ => return,
                    }
                }
            };
        }
    }

    /// $102F within a fill.
    fn fill_plot(&mut self, x: u8, y: u8) {
        if let Some(i) = self.index(x, y) {
            self.watch(i, "fill");
            self.buf[i] = (self.buf[i] & !self.c4) | (self.c4 & self.c3);
            self.run(true);
            self.plots += 1;
            self.show(i);
        }
    }

    /// $0C54/$0C5D: alternating corner lines. `y_first` = F4/F8.
    fn corner(&mut self, mask: u8, y_first: bool) -> u8 {
        self.c4 = mask;
        if let Err(op) = self.read_point() {
            return op;
        }
        self.plot();
        let mut y_turn = y_first;
        loop {
            let b = self.next();
            if b >= 0xF0 {
                return b;
            }
            if y_turn {
                self.c6 = b
            } else {
                self.c5 = b
            }
            self.line();
            y_turn = !y_turn;
        }
    }

    /// $0C88: absolute polyline.
    fn absolute(&mut self, mask: u8) -> u8 {
        self.c4 = mask;
        if let Err(op) = self.read_point() {
            return op;
        }
        self.plot();
        loop {
            let x = self.next();
            if x >= 0xF0 {
                return x;
            }
            self.c5 = x;
            let y = self.next();
            if y >= 0xF0 {
                return y;
            }
            self.c6 = y;
            self.line();
        }
    }

    /// $0CAD/$0CC4: relative polyline. bit7 = -x, bits 6-4 = |dx|, bit3 = -y, bits 2-0 = |dy|.
    fn relative(&mut self, mask: u8) -> u8 {
        self.c4 = mask;
        if let Err(op) = self.read_point() {
            return op;
        }
        self.plot();
        loop {
            let b = self.next();
            if b >= 0xF0 {
                return b;
            }
            let dx = (b & 0x70) >> 4;
            self.c5 = if b & 0x80 != 0 { self.c5.wrapping_sub(dx) } else { self.c5.wrapping_add(dx) };
            let dy = b & 0x07;
            self.c6 = if b & 0x08 != 0 { self.c6.wrapping_sub(dy) } else { self.c6.wrapping_add(dy) };
            self.line();
        }
    }

    /// $0C36: fill at each following point.
    fn fills(&mut self) -> u8 {
        loop {
            if let Err(op) = self.read_point() {
                return op;
            }
            self.fill();
        }
    }
}

/// Draw a picture resource onto a fresh buffer ($0EBA clears to $1F, then $0B83).
pub fn draw(data: &[u8]) -> Picture {
    draw_onto(vec![0x1F; WIDTH * HEIGHT], data)
}

/// Draw a picture resource over an existing buffer, as the engine does when it draws
/// a second picture without clearing first (script opcode `redraw_picture`).
pub fn draw_onto(buf: Vec<u8>, data: &[u8]) -> Picture {
    paint(buf, data, None)
}

/// The buffer after the first `plots` plots of drawing `data` over `buf`: what the
/// screen shows part way through, since the engine plots to the screen as it goes.
pub fn draw_partial(buf: Vec<u8>, data: &[u8], plots: u32) -> Vec<u8> {
    paint(buf, data, Some(plots)).buf
}

fn paint(buf: Vec<u8>, data: &[u8], limit: Option<u32>) -> Picture {
    assert_eq!(buf.len(), WIDTH * HEIGHT);
    let shown = limit.map(|_| buf.clone());
    let mut p = Painter {
        data,
        pos: 0,
        buf,
        c0: 0xF0,
        c3: 0x10, // $0BA0-$0BA8: visual 0, priority 1
        c4: 0,
        f0: 0,
        f1: 0,
        c5: 0,
        c6: 0,
        warnings: Vec::new(),
        overrun: false,
        plots: 0,
        fill_plots: 0,
        runs: Vec::new(),
        new_command: false,
        shown,
        limit: limit.unwrap_or(u32::MAX),
    };
    let mut op = p.next();
    // $0BB1: $FF or any byte below $F0 in opcode position ends the picture.
    // (A partial draw stops after the command that reaches the limit.)
    while (0xF0..0xFF).contains(&op) && p.plots < p.limit {
        p.new_command = true;
        op = match op {
            0xF0 => {
                p.c0 ^= 0xFF;
                p.next()
            }
            0xF1 => {
                let c = p.next();
                p.set_visual(c);
                p.next()
            }
            0xF2 => {
                let v = p.next();
                p.set_priority(v);
                p.next()
            }
            0xF3 => {
                let c = p.next();
                p.set_visual(c);
                let v = p.next();
                p.set_priority(v);
                p.next()
            }
            0xF4 => p.corner(0xFF, true),
            0xF5 => p.corner(0xFF, false),
            0xF6 => p.absolute(0xFF),
            0xF7 => p.relative(0xFF),
            0xF8 => p.corner(p.c0, true),
            0xF9 => p.corner(p.c0, false),
            0xFA => p.absolute(p.c0),
            0xFB => p.relative(p.c0),
            0xFC => {
                p.c4 = 0xFF;
                let c = p.next();
                p.set_visual(c);
                let v = p.next();
                p.set_priority(v);
                p.fills()
            }
            0xFD => {
                p.c4 = 0xF0;
                let v = p.next();
                p.set_priority(v);
                p.fills()
            }
            0xFE => {
                p.c4 = 0x0F;
                let c = p.next();
                p.set_visual(c);
                p.fills()
            }
            _ => unreachable!(),
        };
    }
    if p.overrun {
        p.warnings.push("ran past end of resource without $FF".into());
    } else if op != 0xFF {
        p.warnings.push(format!("stopped on non-opcode byte ${op:02X} at offset {}", p.pos - 1));
    }
    if let Some(s) = p.shown {
        return Picture {
            buf: s,
            warnings: p.warnings,
            end: p.pos,
            plots: p.plots.min(p.limit),
            fill_plots: p.fill_plots,
            runs: p.runs,
        };
    }
    Picture { buf: p.buf, warnings: p.warnings, end: p.pos, plots: p.plots, fill_plots: p.fill_plots, runs: p.runs }
}

/// Apple DHGR colour value (as stored in the buffer) -> RGB.
///
/// The plot routine rotates the value right by one bit before building the screen
/// pattern ($0F3D). Reading the rotated nibble LSB-first as a display bit pattern gives
/// the colour names below (Apple IIe Technical Reference order). With that reading,
/// COLOR_TABLE sends picture indices 0-15 to the PC 16-colour order (black, blue, green,
/// cyan, red, magenta, brown, light grey, dark grey, light blue, light green, light cyan,
/// pink, magenta, yellow, white). RGB values are the Apple IIgs equivalents.
pub fn dhgr_rgb(v: u8) -> [u8; 3] {
    const BY_PATTERN: [u16; 16] = [
        0x000, // 0000 black
        0xD03, // 0001 magenta (deep red)
        0x850, // 0010 brown
        0xF60, // 0011 orange
        0x072, // 0100 dark green
        0x555, // 0101 grey 1
        0x1D0, // 0110 green
        0xFF0, // 0111 yellow
        0x009, // 1000 dark blue
        0xD2D, // 1001 purple
        0xAAA, // 1010 grey 2
        0xF98, // 1011 pink
        0x22F, // 1100 medium blue
        0x6AF, // 1101 light blue
        0x4F9, // 1110 aqua
        0xFFF, // 1111 white
    ];
    let v = v & 0x0F;
    let rot = (v >> 1) | ((v & 1) << 3);
    // Pattern string is bit0 bit1 bit2 bit3 of `rot`, read left to right.
    let idx = ((rot & 1) << 3) | ((rot & 2) << 1) | ((rot & 4) >> 1) | ((rot & 8) >> 3);
    let c = BY_PATTERN[idx as usize];
    [((c >> 8) & 0xF) as u8 * 17, ((c >> 4) & 0xF) as u8 * 17, (c & 0xF) as u8 * 17]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_table_matches_pc_order() {
        let names: Vec<[u8; 3]> = COLOR_TABLE.iter().map(|&v| dhgr_rgb(v)).collect();
        assert_eq!(names[0], [0, 0, 0]);
        assert_eq!(names[1], [0x00, 0x00, 0x99]); // blue
        assert_eq!(names[2], [0x00, 0x77, 0x22]); // green
        assert_eq!(names[4], [0xDD, 0x00, 0x33]); // red
        assert_eq!(names[14], [0xFF, 0xFF, 0x00]); // yellow
        assert_eq!(names[15], [0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn relative_line_and_fill() {
        // Box outline with relative lines, then fill inside with colour 4 (red).
        // F7 x=20 y=10, +5 right, +5 down, -5 left, -5 up; FE 4 at (22, 12); FF.
        let pic = draw(&[0xF3, 0, 2, 0xF7, 20, 10, 0x50, 0x05, 0xD0, 0x0D, 0xFE, 4, 22, 12, 0xFF]);
        assert!(pic.warnings.is_empty(), "{:?}", pic.warnings);
        assert_eq!(pic.buf[(10 - 3) * WIDTH + (20 - 10)], 0x20); // black, priority 2
        assert_eq!(pic.visual(22 - 10, 12 - 3), COLOR_TABLE[4]);
        assert_eq!(pic.visual(26 - 10, 12 - 3), 0x0F); // outside the box untouched
    }

    #[test]
    fn partial_draw_follows_the_plots() {
        let data = [0xF3, 0, 2, 0xF7, 20, 10, 0x50, 0x05, 0xD0, 0x0D, 0xFE, 4, 22, 12, 0xFF];
        let full = draw(&data);
        let blank = vec![0x1F; WIDTH * HEIGHT];
        assert_eq!(draw_partial(blank.clone(), &data, 0), blank);
        assert_eq!(draw_partial(blank.clone(), &data, full.plots), full.buf);
        // Part way: some of the outline, no fill yet; more plots never undo pixels.
        let changed = |n| draw_partial(blank.clone(), &data, n).iter().zip(&blank).filter(|(a, b)| a != b).count();
        assert!(changed(10) > 0 && changed(10) <= 10);
        assert!((1..full.plots).all(|n| changed(n) <= changed(n + 1)));
        assert_ne!(draw_partial(blank.clone(), &data, 10), full.buf);
    }
}
