//! The text screen (`$08E6`-`$0B2A`): the inventory (op 31), the text screen of ops
//! 3C-3F, and the Play diskette's prompts. Text is printed word-wrapped at 40 columns,
//! continuation lines starting at column 1 (`$0A7D`), at the text cursor (DS:1133).

use super::res::Script;
use super::World;

/// The inventory's strings: pointers to "You Are Now Carrying:", "nothing", "Your
/// Score Is: ", "with a ", " time penalty" (DS:10D8, 1EB4, 1EBE, 1ED0, 1EDA).
const CARRYING: usize = 0x10D8;
const NOTHING: usize = 0x10F0;
const SCORE: usize = 0x10FA;
const WITH_A: usize = 0x110C;
const PENALTY: usize = 0x1116;

impl World {
    /// `PCjr $4043`: to the text screen: the graphics cursor kept, the screen cleared in the
    /// text attribute. Coming this way, the text cursor isn't reset (the original
    /// writes it with DS pointing at video memory).
    pub fn enter_text_mode(&mut self) {
        self.g.gfx_cursor = self.text.cursor;
        self.g.hpos_gfx = self.g.bios_hpos;
        self.clear_text_screen(false);
    }

    /// `$0A3E`: the text screen cleared in the text attribute, the cursor home; the
    /// text cursor too, if `reset` (when called directly).
    pub fn clear_text_screen(&mut self, reset: bool) {
        self.text.text_mode = true;
        self.text.text = super::text::Grid::default();
        for r in 0..25 {
            self.text.text.attrs[r] = [self.g.text_attr; 40];
        }
        self.g.bios_hpos = self.g.hpos_text;
        self.text.cursor = (0, 0);
        if reset {
            self.g.text_cursor = (0, 0);
        }
    }

    /// `PCjr $40A6`: back to graphics, the graphics cursor back.
    pub fn leave_text_mode(&mut self) {
        self.g.hpos_text = self.g.bios_hpos;
        self.text.text_mode = false;
        self.g.bios_hpos = self.g.hpos_gfx;
        self.text.cursor = self.g.gfx_cursor;
    }

    /// `$18F0`: op 3C: the text screen, in foreground `fg` on background `bg`.
    pub fn text_screen(&mut self, fg: u8, bg: u8) {
        self.g.text_attr = bg << 4 | (fg & 0x0F);
        self.enter_text_mode();
    }

    /// `$09A9`: op 3D: message `msg` at (`row`, `col`), then a new line.
    pub fn print_at(&mut self, s: &Script, row: u8, col: u8, msg: u16) {
        let text = s.data.clone();
        self.print_wrapped(&text, s.messages + msg as usize, row, col);
        self.text_screen_newline(0);
    }

    /// `$09C6`: op 3E: message `msg` at the text cursor, then a new line.
    pub fn print_text(&mut self, s: &Script, msg: u16) {
        let text = s.data.clone();
        let (row, col) = self.g.text_cursor;
        self.print_wrapped(&text, s.messages + msg as usize, row, col);
        self.text_screen_newline(0);
    }

    /// `PCjr $3ECB`: an engine string at the text cursor, then a new line.
    pub fn print_ds(&mut self, at: usize) {
        let ds = self.res.ds.clone();
        let (row, col) = self.g.text_cursor;
        self.print_wrapped(&ds, at, row, col);
        self.text_screen_newline(0);
    }

    /// `$0A7D`: the string at `at` from (`row`, `col`), up to its 0. Characters outside
    /// 20h-7Fh are left out; a space that would leave the next word past column 39
    /// starts a new line at column 1 instead.
    fn print_wrapped(&mut self, s: &[u8], at: usize, row: u8, col: u8) {
        let byte = |i: usize| s.get(i).copied().unwrap_or(0);
        let mut si = at;
        let c = byte(si);
        si += 1;
        if c == 0 {
            return;
        }
        self.text.cursor = (row, col);
        self.text_screen_char(c);
        loop {
            let c = byte(si);
            si += 1;
            match c {
                0 => return,
                0x80.. | 0..=0x1F => {}
                b' ' => {
                    let len = (si..).take_while(|&i| byte(i) > 0x20).count() as u8;
                    if self.g.text_cursor.1.wrapping_add(len) < 40 {
                        self.text_screen_char(c);
                    } else {
                        self.text_screen_newline(1);
                    }
                }
                c => self.text_screen_char(c),
            }
        }
    }

    /// `$0AC9`: a character at the BIOS cursor in the text attribute; the cursor moves
    /// on (no right margin: only a wrap past 255 starts a new line).
    fn text_screen_char(&mut self, c: u8) {
        self.text.put_attr(c, self.g.text_attr);
        let (r, col) = self.text.cursor;
        let col = col.wrapping_add(1);
        if col == 0 {
            self.g.text_cursor = (r, 1);
            self.text_screen_newline(1);
            return;
        }
        self.g.text_cursor = (r, col);
        self.text.cursor = (r, col);
    }

    /// `$0AE5`: the next row (the screen scrolling past row 24), at column `col`.
    fn text_screen_newline(&mut self, col: u8) {
        let mut r = self.text.cursor.0.wrapping_add(1);
        if r > 24 {
            r = 24;
            self.text.scroll_fill(1, 0, 24, self.g.text_attr);
        }
        self.g.text_cursor = (r, col);
        self.text.cursor = (r, col);
    }

    /// `$09E1` on an engine string named by a pointer in the engine's data.
    fn print_ds_at(&mut self, ptr: usize, row: u8, col: u8) {
        let at = self.res.ds_word(ptr) as usize;
        let ds = self.res.ds.clone();
        self.print_wrapped(&ds, at, row, col);
    }

    /// `$08E6`: op 31: the inventory: the objects carried (bit 7 of their room), two to a
    /// row, or "nothing"; the score; and with v20, the time penalty. Then a key (`c`
    /// and two hex digits changes the colours, which the redraw sets back).
    pub fn inventory_screen(&mut self) {
        loop {
            self.g.text_attr = 0x1F;
            self.enter_text_mode();
            self.print_ds_at(CARRYING, 5, 8);
            let (mut row, mut col, mut other) = (7u8, 3u8, 0x15u8);
            for o in 0..110 {
                let ob = self.objects[o];
                if ob.room < 0x80 {
                    continue;
                }
                let ds = self.res.ds.clone();
                self.print_wrapped(&ds, ob.name as usize, row, col);
                std::mem::swap(&mut col, &mut other);
                if col <= 0x10 {
                    row += 1;
                }
            }
            if row == 7 && col == 3 {
                self.print_ds_at(NOTHING, 7, 3);
                col = 0x10;
            }
            if col >= 0x10 {
                row += 1;
            }
            row += 2;
            self.print_ds_at(SCORE, row, 9);
            let n = Self::number(self.vars[19]);
            let r = self.g.text_cursor.0;
            self.print_wrapped(&n, 0, r, 0x18);
            if self.vars[20] != 0 {
                let r = self.g.text_cursor.0 + 2;
                self.print_ds_at(WITH_A, r, 3);
                let n = Self::number(self.vars[20]);
                let r = self.g.text_cursor.0;
                self.print_wrapped(&n, 0, r, 10);
                let (r, c) = self.g.text_cursor;
                self.print_ds_at(PENALTY, r, c.wrapping_add(1));
            }
            // PCjr $4191: a key; 'c' and two hex digits set the colours, and the screen is
            // drawn again (in its own colours).
            self.g.keys_on = false;
            self.site_keys();
            self.wait_until("a key", |w| !w.keys.is_empty());
            if self.keys.front().map(|&k| k as u8) == Some(b'c') {
                self.keys.pop_front();
                let hi = Self::hex(self.blocking_key() as u8);
                let lo = Self::hex(self.blocking_key() as u8);
                self.g.text_attr = hi << 4 | lo;
                self.leave_text_mode();
                continue;
            }
            self.g.keys_on = true;
            return;
        }
    }

    /// `$09F2`: a number in decimal, without leading zeros, ended by 0.
    fn number(n: u8) -> Vec<u8> {
        let mut s = n.to_string().into_bytes();
        s.push(0);
        s
    }

    /// `$0B2A`: a hex digit's value (`a`-`f` after `9`).
    fn hex(c: u8) -> u8 {
        let v = c.wrapping_sub(b'0');
        if v > 9 {
            v.wrapping_sub(0x27)
        } else {
            v
        }
    }
}
