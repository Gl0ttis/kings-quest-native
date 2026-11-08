//! Text output and input (port plan M4). Graphics-mode text is drawn by aux $14A4 into
//! hi-res rows 20-23 under the picture, using the game's font (fixed resource aux $1D00):
//! 96 glyphs of 8 bytes, bit 0 leftmost, ink = 0, each bit shown as two dots. The text
//! screen (opcodes $3C-$3F, $31) uses the Apple text page; it is drawn here with the same
//! font, uppercase inverted as the engine's output filter ($5782) requests.

use super::World;
use crate::pic::{dhgr_rgb, HEIGHT, WIDTH};

pub const COLS: usize = 40;
pub const ROWS: usize = 24;
/// Frame size: 560 double hi-res dots by 192 lines.
pub const FRAME_W: usize = 560;
pub const FRAME_H: usize = 192;

#[derive(Clone)]
pub struct Text {
    /// Hi-res text rows (only rows 20-23 are below the picture).
    pub hires: [[u8; COLS]; ROWS],
    /// Cells above row 20 that text has been drawn into (over the picture).
    pub written: [[bool; COLS]; ROWS],
    /// Apple text page, shown while `text_mode` is set: the screen codes the original
    /// stores (see `screen_code`).
    pub page: [[u8; COLS]; ROWS],
    pub text_mode: bool,
    pub ch: u8,             // $24
    pub cv: u8,             // $25
    pub saved: (u8, u8),    // $FC/$FD
    pub lines: u8,          // $1D: lines printed since the last input
    pub idle: u8,           // $1C: cycles until `lines` resets
    pub input: Vec<u8>,     // $7042 / $6F2F
    pub last_line: Vec<u8>, // $6F08
    pub need_prompt: bool,  // $702C
    pub need_reprint: bool, // $6EF0
    pub line_ready: bool,   // $6EFC
}

impl Default for Text {
    fn default() -> Self {
        Text {
            hires: [[b' '; COLS]; ROWS],
            written: [[false; COLS]; ROWS],
            page: [[b' '; COLS]; ROWS],
            text_mode: false,
            ch: 0,
            cv: 20,
            saved: (20, 0),
            lines: 0,
            idle: 0,
            input: Vec::new(),
            last_line: Vec::new(),
            need_prompt: true,
            need_reprint: false,
            line_ready: false,
        }
    }
}

impl World {
    /// $5796: output one character through the current vector (hi-res text or text page).
    pub fn put_char(&mut self, c: u8) {
        let c = c & 0x7F;
        let t = &mut self.text;
        if c == 0x0D {
            if !t.text_mode {
                // $15AB clear to end of line (by printing spaces), then $153F new line /
                // scroll rows 20-23.
                for x in t.ch as usize..COLS {
                    t.hires[t.cv as usize][x] = b' ';
                }
                // The CR itself goes through the glyph routine too (refemu, REFEMU_CHARS).
                self.cost.chars += 1 + (COLS as u64).saturating_sub(self.text.ch as u64);
                if self.text.cv >= 23 {
                    // $154C: after the scroll, row 23 is cleared by drawing 40 spaces.
                    self.cost.chars += COLS as u64;
                }
            }
            let t = &mut self.text;
            if t.cv < 23 {
                t.cv += 1;
            } else if t.text_mode {
                // The ROM's scroll clears the new bottom line with normal spaces.
                t.page.rotate_left(1);
                t.page[23] = [0xA0; COLS];
            } else {
                for r in 20..23 {
                    t.hires[r] = t.hires[r + 1];
                }
                t.hires[23] = [b' '; COLS];
            }
            t.ch = 0;
            return;
        }
        let (r, x) = (t.cv as usize % ROWS, t.ch as usize);
        if !t.text_mode {
            self.cost.chars += 1;
        }
        if x < COLS {
            if t.text_mode {
                t.page[r][x] = screen_code(c);
            } else {
                t.hires[r][x] = c;
                if r < 20 {
                    t.written[r][x] = true;
                }
            }
        }
        t.ch = t.ch.wrapping_add(1);
    }

    /// $579B: print a string as is.
    pub fn put_str(&mut self, s: &[u8]) {
        for &c in s {
            if c == 0 {
                break;
            }
            self.put_char(c);
        }
    }

    /// $6DE9: new line, pausing for RETURN (with a beep) every fourth line.
    fn message_newline(&mut self) {
        if self.text.lines >= 3 {
            self.io_bell();
            loop {
                let k = self.io_wait_key();
                if k == 0x0D || k == 0x1B {
                    break;
                }
            }
            self.text.lines = 0;
        }
        self.text.lines += 1;
        self.put_char(0x0D);
    }

    /// $56CA: print a message (lines separated by CR), then arrange for the prompt to be
    /// redrawn.
    pub fn print_message(&mut self, msg: &[u8]) {
        if !self.text.input.is_empty() {
            self.message_newline();
        }
        self.text.ch = 0;
        for line in msg.split(|&c| c == 0x0D) {
            self.put_str(line);
            self.message_newline();
        }
        self.text.idle = 10;
        self.text.need_reprint = true;
    }

    /// $57B1: start a fresh prompt line ("> " and the block cursor).
    fn prompt(&mut self) {
        self.text.ch = 0;
        self.text.need_prompt = false;
        let cv = self.text.cv as usize;
        // aux $1C (via $4A20) clears the line by drawing 40 spaces.
        self.text.hires[cv] = [b' '; COLS];
        if !self.text.text_mode {
            self.cost.chars += COLS as u64;
        }
        self.put_str(b"> ");
        self.cursor();
    }

    /// $57E7: draw the cursor glyph ($7F) without advancing.
    fn cursor(&mut self) {
        self.put_char(0x7F);
        self.text.ch = self.text.ch.wrapping_sub(1);
    }

    /// $57DF: erase the cursor.
    fn uncursor(&mut self) {
        self.put_char(b' ');
        self.text.ch = self.text.ch.wrapping_sub(1);
    }

    /// Start of each cycle ($4029-$4036): redraw the prompt if needed.
    pub fn refresh_prompt(&mut self) {
        if self.text.need_prompt {
            self.prompt();
        }
        if self.text.need_reprint {
            self.prompt();
            self.text.need_reprint = false;
            let line = self.text.input.clone();
            self.put_str(&line);
            self.cursor();
        }
    }

    /// $5312/$53C3: a typed key that is not a direction. Returns true if consumed.
    pub fn type_key(&mut self, k: u8) {
        match k {
            0x1B => {
                // ESC: pause until a key.
                self.io_wait_key();
                return;
            }
            b'5' => return self.canned(b"restart game"),
            b'3' => return self.canned(b"restore game"),
            0x09 => {
                self.inventory_screen();
                return;
            }
            _ => {}
        }
        if self.g.flag_6f05 != 0 {
            return; // input disabled (opcode $2B)
        }
        match k {
            b'1' => self.canned(b"save game"),
            b'=' => self.vars[31] ^= 1,
            0x05 => {
                let l = self.text.last_line.clone();
                self.set_input(&l);
            }
            0x03 => self.set_input(&[]),
            0x0E => {
                if let Some(&c) = self.text.last_line.get(self.text.input.len()) {
                    self.edit_key(c);
                }
            }
            _ => self.edit_key(k),
        }
    }

    fn canned(&mut self, s: &[u8]) {
        self.set_input(s);
        self.edit_key(0x0D);
    }

    /// $53A7
    fn set_input(&mut self, s: &[u8]) {
        self.text.input = s.to_vec();
        self.text.need_reprint = false;
        self.prompt();
        let line = self.text.input.clone();
        self.put_str(&line);
        self.cursor();
    }

    /// $53C3: line editing.
    fn edit_key(&mut self, k: u8) {
        match k {
            0x0D => {
                self.uncursor();
                self.put_char(0x0D);
                self.text.last_line = std::mem::take(&mut self.text.input);
                self.text.lines = 0;
                self.text.need_prompt = true;
                self.text.line_ready = true;
            }
            0x7F => {
                if !self.text.input.is_empty() {
                    self.uncursor();
                    self.text.input.pop();
                    self.text.ch = self.text.ch.wrapping_sub(1);
                    self.cursor();
                }
            }
            b' ' | b'A'..=0x7E if self.text.input.len() < 0x25 => {
                self.text.input.push(k);
                self.put_char(k);
                self.cursor();
            }
            _ => {}
        }
    }

    /// $4044-$4076: handle an entered line: parse, run the parser rules, set v28/v13.
    pub fn handle_line(&mut self) {
        if !self.text.line_ready {
            return;
        }
        self.text.line_ready = false;
        self.text.need_prompt = true;
        let line = self.text.last_line.clone();
        let p = super::parser::parse(&self.res.vocab, &line);
        if let Some(t) = p.cmp_table {
            self.g.word_cmp = t;
        }
        if let Some(w) = &p.unknown {
            if self.vars[5] == 0 {
                self.put_str(b"I don't understand \"");
                self.put_str(w.as_bytes());
                self.put_str(b"\".\r");
            }
        }
        self.g.said = [p.verb, p.noun];
        if p.nothing() {
            if self.vars[5] != 0 {
                self.vars[28] = 1;
            }
            return;
        }
        // $408E
        let matched = self.run_parser_rules(p.verb, p.noun);
        if self.g.abort_pass {
            return; // restarted: $4018 reset the stack, so $408E never returns here
        }
        self.vars[28] = if matched || p.verb == 0xFF {
            3
        } else if p.verb != 0 {
            2
        } else {
            1
        };
        self.vars[13] = (self.vars[13] + 1) % 3;
    }

    /// $56E6: switch to the text screen and clear it.
    pub fn enter_text_mode(&mut self) {
        self.note("text screen");
        self.text.saved = (self.text.cv, self.text.ch);
        self.text.text_mode = true;
        // $571A: 24 x 40 spaces through the output filter: inverse spaces, white. The
        // last one wraps past the bottom line, so the ROM scrolls once and clears the
        // bottom line with normal (black) spaces.
        self.text.page = [[0x20; COLS]; ROWS];
        self.text.page[ROWS - 1] = [0xA0; COLS];
        self.text.cv = 0;
        self.text.ch = 0;
    }

    /// $5FCF: wait for a key, then return to the graphics screen.
    pub fn leave_text_mode(&mut self) {
        self.io_wait_key();
        self.text.text_mode = false;
        (self.text.cv, self.text.ch) = self.text.saved;
    }

    /// $45CF: the inventory screen (Tab, or opcode $31).
    pub fn inventory_screen(&mut self) {
        self.enter_text_mode();
        self.text.cv = 4;
        self.text.ch = 0;
        self.put_str(b"You are now carrying:\r\r");
        let mut count = 0;
        for o in 0..0x6F {
            if self.objects[o][2] & 0x80 == 0 {
                continue;
            }
            // $4623: two columns, at 3 and 21.
            let col = if self.text.ch >= 0x16 {
                self.put_char(0x0D);
                3
            } else if self.text.ch >= 4 {
                0x15
            } else {
                3
            };
            self.text.ch = col;
            let name = self.object_name(o);
            self.put_str(&name);
            count += 1;
        }
        if count == 0 {
            self.text.ch = 3;
            self.put_str(b"nothing");
        }
        // $4655: score three lines further down.
        self.text.cv = (self.text.cv + 3).min(23);
        self.text.ch = 0;
        self.put_str(b"Your score is: ");
        let s = self.vars[19].to_string();
        self.put_str(s.as_bytes());
        self.leave_text_mode();
    }

    /// Object name: the string at (object bytes 0-1) - 2, inside the object table that
    /// starts at $1A80 (111 records followed by the names).
    pub fn object_name(&self, o: usize) -> Vec<u8> {
        let table: Vec<u8> = self.objects.iter().flatten().copied().collect();
        let r = self.objects[o];
        let p = (r[0] as usize | (r[1] as usize) << 8).wrapping_sub(2).wrapping_sub(0x1A80);
        let s = table.get(p..).unwrap_or(&[]);
        s.iter().take_while(|&&c| c != 0).copied().collect()
    }

    /// Render the whole display: the picture (or the text page) and text rows 20-23,
    /// as 0RGB pixels, FRAME_W x FRAME_H.
    pub fn render_frame(&self) -> Vec<u32> {
        self.render_frame_with(&self.screen, true)
    }

    /// render_frame with `pic` (a picture buffer, e.g. a picture part drawn) in place
    /// of the composed screen, and without the text rows if `text` is false (while a
    /// room's picture is drawn, the original's text rows are blank: the prompt comes
    /// back at the start of the next pass).
    pub fn render_frame_with(&self, pic: &[u8], text: bool) -> Vec<u32> {
        let mut fb = self.frame_pixels(pic, text);
        // Text mode has no colour burst, so no fringes there.
        if self.display.ntsc && !self.text.text_mode {
            super::display::ntsc(&mut fb);
        }
        fb
    }

    fn frame_pixels(&self, pic: &[u8], text: bool) -> Vec<u32> {
        let mut fb = vec![0u32; FRAME_W * FRAME_H];
        let rgb = |c: [u8; 3]| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        if self.text.text_mode {
            for r in 0..ROWS {
                self.draw_text_row(&mut fb, r, &self.text.page[r], true);
            }
            return fb;
        }
        let specks =
            self.display.fill_specks && std::ptr::eq(pic, self.screen.as_slice()) && self.base.len() == pic.len();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mut v = pic[y * WIDTH + x] & 0x0F;
                if specks {
                    if let Some(c) = super::display::speck(&self.base, pic, x, y) {
                        v = c;
                    }
                }
                let px = rgb(dhgr_rgb(v));
                fb[y * FRAME_W + x * 4..y * FRAME_W + x * 4 + 4].fill(px);
            }
        }
        if !text {
            return fb;
        }
        for r in 20..ROWS {
            self.draw_text_row(&mut fb, r, &self.text.hires[r], false);
        }
        // Text drawn over the picture (the title screens).
        for r in 0..20 {
            if self.text.written[r].iter().any(|&w| w) {
                let mut fb_row = vec![0u32; FRAME_W * 8];
                self.draw_text_row(&mut fb_row, 0, &self.text.hires[r], false);
                for line in 0..8 {
                    for col in 0..COLS {
                        if self.text.written[r][col] {
                            let src = line * FRAME_W + col * 14;
                            let d = (r * 8 + line) * FRAME_W + col * 14;
                            fb[d..d + 14].copy_from_slice(&fb_row[src..src + 14]);
                        }
                    }
                }
            }
        }
        fb
    }

    fn draw_text_row(&self, fb: &mut [u32], r: usize, row: &[u8; COLS], text_page: bool) {
        if text_page {
            for (col, &code) in row.iter().enumerate() {
                let (c, inverse) = displayed(code);
                for line in 0..8 {
                    // 14 pixels: 7 dots, each two wide; `on` = drawn in the text colour.
                    let dots: [bool; 14] = match self.display.charrom.as_ref() {
                        Some(rom) => {
                            let b = rom.row(c, false, line);
                            std::array::from_fn(|d| b >> (d / 2) & 1 == 1)
                        }
                        None => {
                            // The game's font is drawn black on white (as in its own text
                            // rows), so a lit dot here is background.
                            let b = self
                                .res
                                .font
                                .get((c.max(0x20) - 0x20) as usize % 96 * 8 + line)
                                .copied()
                                .unwrap_or(0xFF);
                            [0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6, 7].map(|i| b >> i & 1 == 0)
                        }
                    };
                    for (d, &on) in dots.iter().enumerate() {
                        // Normal: white text on black. Inverse: black text on white.
                        let white = on != inverse;
                        fb[(r * 8 + line) * FRAME_W + col * 14 + d] = if white { 0xFFFFFF } else { 0 };
                    }
                }
            }
            return;
        }
        for (col, &c) in row.iter().enumerate() {
            let invert = false;
            let g = (c.max(0x20) - 0x20) as usize % 96;
            for line in 0..8 {
                let b = self.res.font.get(g * 8 + line).copied().unwrap_or(0xFF);
                // aux $14ED: bits 0-3 doubled then bits 4-7 doubled, 7 dots per byte.
                let dots = [0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6, 7].map(|i| b >> i & 1 == 1);
                for (d, &on) in dots.iter().enumerate() {
                    let white = on != invert;
                    fb[(r * 8 + line) * FRAME_W + col * 14 + d] = if white { 0xFFFFFF } else { 0 };
                }
            }
        }
    }
}

/// $5782, the text-screen output filter, then the ROM's COUT: the screen code stored for
/// character `c`. The game turns on the IIe's alternate character set ($C00F), where
/// $40-$5F would be MouseText, so upper case ($41-$60) is masked to the inverse codes
/// $01-$20. Everything else is stored as it comes, high bit clear: inverse too.
pub fn screen_code(c: u8) -> u8 {
    let c = c & 0x7F;
    if (0x41..0x61).contains(&c) {
        c & 0x1F
    } else {
        c
    }
}

/// What a screen code shows with the alternate character set: (ASCII character,
/// inverse). $00-$1F inverse @A-Z[\]^_, $20-$3F inverse symbols and digits, $40-$5F
/// MouseText (never stored; shown as inverse capitals), $60-$7F inverse lower case,
/// $80-$FF normal.
pub fn displayed(code: u8) -> (u8, bool) {
    match code {
        0x00..=0x1F => (code | 0x40, true),
        0x20..=0x3F | 0x60..=0x7F => (code, true),
        0x40..=0x5F => (code, true),
        _ => (code & 0x7F, false),
    }
}

/// The joystick and the two buttons, as the engine reads them.
#[derive(Default, Clone, Copy, Debug)]
pub struct Joystick {
    /// Paddle counts 0-255 ($C064/$C065: about 128 at rest), or None with no joystick.
    pub paddles: Option<(u8, u8)>,
    /// Buttons 0 and 1 ($C061/$C062): on a IIe, also the Open-Apple and Solid-Apple keys.
    pub buttons: [bool; 2],
}

/// Front ends implement this to supply keys while the engine waits, and to make sounds.
pub trait IoImpl {
    /// A picture drawn outside a pass (the title screens), for front ends that show the
    /// drawing in real time: `draw.span` gives its timing from now, `frame(n)` the
    /// display after `n` plots. The default shows nothing.
    fn show_drawing(&mut self, _draw: &super::cost::PicDraw, _frame: &dyn Fn(u32) -> Vec<u32>) {}
    /// Show `frame` for `secs` seconds of the original's time (start-up), without taking
    /// keys: a key pressed meanwhile stays for the next wait, as the Apple's keyboard
    /// latch keeps it. `loading`: the time is disk loading (skipped with fast loads).
    /// The default returns at once.
    fn pause(&mut self, _frame: &[u32], _secs: f64, _loading: bool) {}
    /// The joystick now. The default is none.
    fn joystick(&mut self) -> Joystick {
        Joystick::default()
    }
    /// Block until a key is pressed; `frame` is the current display to show meanwhile.
    fn wait_key(&mut self, frame: &[u32]) -> u8;
    /// Wait for a key for at most `secs` seconds (the title screens). Front ends without
    /// real time return None at once.
    fn wait_key_timeout(&mut self, _frame: &[u32], _secs: f64) -> Option<u8> {
        None
    }
    /// The Apple ROM bell ($6618 -> $FBDD). See crate::sound.
    fn bell(&mut self);
    /// Two beeps with a short pause ($6625), for save and restore disk errors.
    fn double_bell(&mut self) {
        self.bell();
        self.bell();
    }
    /// Store a saved game in slot 1-26 (letters A-Z).
    fn save(&mut self, slot: u8, data: &[u8]) -> Result<(), String>;
    /// Fetch a saved game, if that slot holds one.
    fn load(&mut self, slot: u8) -> Option<Vec<u8>>;
}

impl World {
    pub(crate) fn io_wait_key(&mut self) -> u8 {
        let frame = self.render_frame();
        match self.io.as_mut() {
            Some(io) => io.wait_key(&frame),
            None => 0x0D,
        }
    }

    pub(crate) fn io_bell(&mut self) {
        match self.io.as_mut() {
            Some(io) => io.bell(),
            None => self.note("bell"),
        }
    }

    pub(crate) fn io_double_bell(&mut self) {
        match self.io.as_mut() {
            Some(io) => io.double_bell(),
            None => self.note("double bell"),
        }
    }
}

/// Keys queued for headless runs (RETURN when the queue is empty), and saves kept in
/// memory.
#[derive(Default)]
pub struct Scripted {
    pub keys: std::collections::VecDeque<u8>,
    pub saves: std::collections::HashMap<u8, Vec<u8>>,
    /// Waits answered with RETURN because the queue was empty.
    pub empty_waits: u32,
}

impl IoImpl for Scripted {
    fn wait_key(&mut self, _frame: &[u32]) -> u8 {
        self.keys.pop_front().unwrap_or_else(|| {
            // Some prompts (Y/N) accept only particular keys and would wait forever.
            self.empty_waits += 1;
            assert!(self.empty_waits < 10_000, "headless run is waiting for a key that was not supplied");
            0x0D
        })
    }
    fn bell(&mut self) {}
    fn save(&mut self, slot: u8, data: &[u8]) -> Result<(), String> {
        self.saves.insert(slot, data.to_vec());
        Ok(())
    }
    fn load(&mut self, slot: u8) -> Option<Vec<u8>> {
        self.saves.get(&slot).cloned()
    }
}

pub type Io = Box<dyn IoImpl>;
