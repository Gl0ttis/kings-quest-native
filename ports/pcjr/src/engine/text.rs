//! Text on the screen. The original draws characters through the BIOS, in the ROM's
//! font, which the port doesn't have: it keeps the characters and their attributes,
//! and the front end draws them.
//!
//! Two screens: the graphics screen's character cells (the input line and the messages,
//! rows 21-24, and the title's credits), and the text screen (mode 1, page 7) of the
//! inventory, the Play diskette and ops 3C-3F. The BIOS cursor is one, as the game uses
//! it (it keeps the graphics cursor itself while the text screen is up, at DS:1EF4).

/// A 40 × 25 screen of characters and attributes.
#[derive(Clone)]
pub struct Grid {
    pub cells: [[u8; 40]; 25],
    pub attrs: [[u8; 40]; 25],
}

impl Default for Grid {
    fn default() -> Self {
        Grid { cells: [[b' '; 40]; 25], attrs: [[7; 40]; 25] }
    }
}

#[derive(Clone, Default)]
pub struct Text {
    pub graphics: Grid,
    pub text: Grid,
    /// The text screen is up.
    pub text_mode: bool,
    pub cursor: (u8, u8),
}

impl Text {
    /// The screen shown.
    pub fn grid(&self) -> &Grid {
        if self.text_mode {
            &self.text
        } else {
            &self.graphics
        }
    }
    fn grid_mut(&mut self) -> &mut Grid {
        if self.text_mode {
            &mut self.text
        } else {
            &mut self.graphics
        }
    }

    /// INT 10h AH=9: a character at the cursor, which doesn't move.
    pub fn put_attr(&mut self, c: u8, attr: u8) {
        let (r, col) = self.cursor;
        if r < 25 && col < 40 {
            let g = self.grid_mut();
            g.cells[r as usize][col as usize] = c;
            g.attrs[r as usize][col as usize] = attr;
        }
    }
    pub fn put(&mut self, c: u8) {
        self.put_attr(c, 7);
    }

    /// INT 10h AH=6: scroll rows `top..=bottom` up `lines` (0: clear them), the new rows
    /// in attribute `fill`.
    pub fn scroll(&mut self, lines: u8, top: u8, bottom: u8) {
        self.scroll_fill(lines, top, bottom, 0);
    }
    pub fn scroll_fill(&mut self, lines: u8, top: u8, bottom: u8, fill: u8) {
        let bottom = bottom.min(24);
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let lines = if lines == 0 || lines > height { height } else { lines };
        let g = self.grid_mut();
        for r in top..=bottom {
            let src = r + lines;
            if src <= bottom {
                g.cells[r as usize] = g.cells[src as usize];
                g.attrs[r as usize] = g.attrs[src as usize];
            } else {
                g.cells[r as usize] = [b' '; 40];
                g.attrs[r as usize] = [fill; 40];
            }
        }
    }

    /// INT 10h AH=0Eh: a character, the cursor moving on (and the screen scrolling).
    pub fn teletype(&mut self, c: u8, attr: u8) {
        let (mut r, mut col) = self.cursor;
        match c {
            0x0D => col = 0,
            0x0A => r += 1,
            0x08 => col = col.saturating_sub(1),
            0x07 => {}
            _ => {
                self.put_attr(c, attr);
                col += 1;
                if col >= 40 {
                    col = 0;
                    r += 1;
                }
            }
        }
        if r >= 25 {
            self.scroll(1, 0, 24);
            r = 24;
        }
        self.cursor = (r, col);
    }

    /// Row `r` of the screen shown, as text.
    pub fn row(&self, r: usize) -> String {
        self.grid().cells[r].iter().map(|&c| c as char).collect()
    }
}
