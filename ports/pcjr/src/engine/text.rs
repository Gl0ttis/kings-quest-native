//! Text: the characters on the graphics screen (the input line and the messages, rows
//! 21-24), with the BIOS cursor the game moves.
//!
//! The original draws characters through the BIOS, in the ROM's font, which the port
//! doesn't have: it keeps the characters, and the front end draws them.

/// The 40 × 25 character cells of the screen, and the cursor.
#[derive(Clone)]
pub struct Text {
    pub cells: [[u8; 40]; 25],
    pub cursor: (u8, u8),
}

impl Default for Text {
    fn default() -> Self {
        Text { cells: [[b' '; 40]; 25], cursor: (0, 0) }
    }
}

impl Text {
    /// INT 10h AH=9: a character at the cursor, which doesn't move.
    pub fn put(&mut self, c: u8) {
        let (r, col) = self.cursor;
        if r < 25 && col < 40 {
            self.cells[r as usize][col as usize] = c;
        }
    }

    /// INT 10h AH=6: scroll rows `top..=bottom` up `lines` (0: clear them).
    pub fn scroll(&mut self, lines: u8, top: u8, bottom: u8) {
        let bottom = bottom.min(24);
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let lines = if lines == 0 || lines > height { height } else { lines };
        for r in top..=bottom {
            let src = r + lines;
            self.cells[r as usize] = if src <= bottom { self.cells[src as usize] } else { [b' '; 40] };
        }
    }

    /// INT 10h AH=0Eh: a character, the cursor moving on (and the screen scrolling).
    pub fn teletype(&mut self, c: u8) {
        let (mut r, mut col) = self.cursor;
        match c {
            0x0D => col = 0,
            0x0A => r += 1,
            0x08 => col = col.saturating_sub(1),
            0x07 => {}
            _ => {
                self.put(c);
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

    /// Row `r` as text.
    pub fn row(&self, r: usize) -> String {
        self.cells[r].iter().map(|&c| c as char).collect()
    }
}
