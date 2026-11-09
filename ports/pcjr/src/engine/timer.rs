//! The game's timer handler (INT 1Ch, `$0808`), 18.2 times a second:
//! 1. the music: the next record when its time is up;
//! 2. the messages: the next word of the first queued one (every v24 + 1 ticks);
//! 3. the joystick, every other tick when no key is waiting (in refemu-pcjr none is
//!    connected, and the read times out);
//! 4. the keyboard: control keys act at once; the first character typed goes into the
//!    key ring for the input line.

use super::{Msg, Stream, World};
use std::rc::Rc;

/// The key ring's size.
const RING: usize = 80;
/// Key codes (DS:0D2A), for characters 0-7Ah; and for keypad scan codes 47h-51h, the
/// directions (DS:0D04).
const KEY_CODES: usize = 0x0D2A;
const KEYPAD: usize = 0x0D04;
/// Alt-letter scan codes 10h-32h (DS:0DA5): all ignored.
const ALT_CODES: usize = 0x0DA5;
/// The joystick's directions (DS:0D11), by 4 left / 8 right + 1 up / 2 down.
const JOY_DIRS: usize = 0x0D11;

impl World {
    /// `$0808`.
    pub fn int1c(&mut self) {
        if !self.g.music_on && self.g.queue.is_empty() && self.g.joy_skip && self.keys.is_empty() {
            self.g.joy_skip = false;
            return;
        }
        if self.g.music_on {
            self.music_tick();
        }
        if !self.g.queue.is_empty() {
            self.g.print_countdown = self.g.print_countdown.wrapping_sub(1);
            if self.g.print_countdown == 0 {
                self.g.print_countdown = self.vars[24].wrapping_add(1);
                self.print_chunk();
            }
        }
        if self.g.joy_skip {
            self.g.joy_skip = false;
        } else if self.keys.is_empty() {
            self.joystick();
        }
        self.key_tick();
    }

    /// `$0563`: read the joystick. Its axes' one-shots are fired and the port polled
    /// until both have ended; one that runs 41 turns is a time-out (no joystick).
    /// Button 1 is the `-` key held down: v16 while it is down (the key toggles v16,
    /// and v31 marks that toggle, which a button that is up leaves alone); button 2 is
    /// the `0` key, the jump (v17). The stick's direction goes in DS:1278, for the next
    /// pass to take when the keypad gives none.
    fn joystick(&mut self) {
        self.g.joy_skip = true;
        self.g.joy_timeout = 0;
        let stick = self.clock.joystick().filter(|j| j.x < 41 && j.y < 41);
        let Some(j) = stick else {
            self.cost.joystick += 1;
            self.g.joy_timeout = 1;
            self.g.joy_dir = 0;
            return;
        };
        self.cost.joystick_turns += j.x.max(j.y) as u32 + 1;
        if !(self.vars[31] == 1 && !j.buttons[0]) {
            self.vars[16] = j.buttons[0] as u8;
            self.vars[31] = 0;
        }
        if j.buttons[1] {
            self.vars[31] = 0;
            self.vars[17] = 1;
        }
        // (A key that came during the read would end it here, the direction kept; the
        // read only starts with none waiting, and the port's keys come at checkpoints.)
        let col = match j.x {
            0..=6 => 4,
            7..=18 => 0,
            _ => 8,
        };
        let row = match j.y {
            0..=6 => 1,
            7..=18 => 0,
            _ => 2,
        };
        self.g.joy_dir = self.res.ds[JOY_DIRS + col + row];
        if self.g.joy_dir != 0 {
            self.vars[31] = 0;
        }
    }

    /// `$0844`: send the next record to the sound chip and take its duration. 0 ends
    /// the tune (v15 = 1; on to the queued tune if there is another), FF repeats it.
    fn music_tick(&mut self) {
        self.g.music_countdown = self.g.music_countdown.wrapping_sub(1);
        if self.g.music_countdown != 0 {
            return;
        }
        let Some(mut s) = self.g.music.clone() else { return };
        let n = s.data.get(s.at).copied().unwrap_or(0) as usize;
        s.at += 1;
        let n = if n == 0 { 0x10000 } else { n };
        for _ in 0..n {
            let b = s.data.get(s.at).copied().unwrap_or(0);
            s.at += 1;
            if self.g.sound_on {
                self.sound_out.push(b);
            }
            self.cost.sound += 1;
        }
        let d = s.data.get(s.at).copied().unwrap_or(0);
        s.at += 1;
        match d {
            0 => {
                self.vars[15] = 1;
                let next = self.g.music_next.clone();
                match next {
                    Some(n) if self.g.music_start.as_ref() != Some(&n) => {
                        self.g.music_start = Some(n.clone());
                        self.g.music_countdown = 1;
                        self.g.music = Some(n);
                    }
                    _ => {
                        self.g.music_on = false;
                        self.g.music_countdown = 1;
                    }
                }
            }
            0xFF => {
                self.g.music_countdown = 1;
                self.g.music = self.g.music_start.clone();
            }
            d => {
                self.g.music_countdown = d;
                self.g.music = Some(s);
            }
        }
    }

    /// `$0A9D`: print the first queued message from where it was left: up to and
    /// including a space (all of it while `print_all`). 00 ends it with a new line;
    /// 01-0C with the cursor back to the start of row 24; FF just ends it; 0D is a new
    /// line inside it. When the queue is empty, v23 = 0 and the prompt is due again.
    pub fn print_chunk(&mut self) {
        loop {
            let Some(m) = self.g.queue.front_mut() else { return };
            let c = m.text.get(m.at).copied().unwrap_or(0xFF);
            m.at += 1;
            let ended = match c {
                0xFF => true,
                0 => {
                    self.text_newline();
                    true
                }
                1..=0x0C => {
                    self.g.text_col = 0;
                    self.text.cursor = (0x18, 0);
                    true
                }
                0x0D => {
                    self.text_newline();
                    false
                }
                c => {
                    self.text.put(c);
                    self.g.text_col = self.g.text_col.wrapping_add(1);
                    self.text.cursor = (self.g.text_row, self.g.text_col);
                    if c != b' ' {
                        continue;
                    }
                    false
                }
            };
            if ended {
                self.g.queue.pop_front();
                if self.g.queue.is_empty() {
                    self.g.prompt_due = true;
                    self.vars[23] = 0;
                    return;
                }
            }
            if !self.g.print_all {
                return;
            }
        }
    }

    /// `$0B76`: the text window (rows 21-24) up a line; the cursor to the start of row
    /// 24.
    pub fn text_newline(&mut self) {
        self.text.scroll(1, 0x15, 0x18);
        self.g.text_col = 0;
        self.text.cursor = (0x18, 0);
    }

    /// `$0A7D`: queue a message for the timer to print (dropped if 8 are queued);
    /// v23 = 1.
    pub fn queue_message(&mut self, text: Rc<Vec<u8>>, at: usize) {
        if self.g.queue.len() < 8 {
            self.g.queue.push_back(Msg { text, at });
        }
        self.vars[23] = 1;
    }

    /// `$462F`: wait until the queued messages are printed.
    pub fn wait_print_done(&mut self) {
        self.wait_until("the messages to print", |w| w.vars[23] == 0);
    }

    /// `$08CE`: read keys while the game lets the timer (0E27 and v25).
    fn key_tick(&mut self) {
        loop {
            if !self.g.keys_on || self.vars[25] == 0 {
                return;
            }
            let Some(k) = self.keys.pop_front() else { return };
            let (al, ah) = (k as u8, (k >> 8) as u8);
            if self.g.paused {
                // $0909: a key ends the pause.
                self.g.paused = false;
                continue;
            }
            // `cmp al,7Ah; jg`: signed, so 80h-FFh go on to the table, past its end.
            if (al as i8) > 0x7A {
                continue;
            }
            if self.g.input_off && !matches!(al, b'7' | b'5' | 9) {
                continue;
            }
            if al == 0 {
                // $09BF: the keypad walks; pressing the same direction again stops.
                if (0x47..=0x51).contains(&ah) {
                    let mut d = self.res.ds[KEYPAD + (ah - 0x47) as usize];
                    if self.g.key_dir == d {
                        d = 0;
                    }
                    self.g.key_dir = d;
                    if d != 0 {
                        self.vars[31] = 0;
                    }
                    return;
                }
                if (0x10..=0x32).contains(&ah) && self.res.ds[ALT_CODES + (ah - 0x10) as usize] != 0xFF {
                    return;
                }
                continue;
            }
            if al >= 0x80 {
                // The original looks past its table, into its key ring pointers, the
                // sound table and its flags; the port reads the table's initial bytes.
                self.log.push(format!("key {k:04X}: character {al:02X} is past the key table"));
            }
            match self.res.ds[KEY_CODES + al as usize] {
                0xFF => {}
                c @ 0..=0x7F => {
                    if self.g.ring.len() < RING {
                        self.g.ring.push_back(c);
                    }
                    return;
                }
                c @ 0x80..=0x85 => self.g.key_flags[(c & 0x7F) as usize] = true,
                0x8F => self.g.paused = true,
                0x8E => {
                    let d = self.vars[31] ^ 1;
                    self.vars[31] = d;
                    self.vars[16] = d;
                    if d == 1 {
                        self.g.key_dir = 0;
                    }
                }
                0x8D => {
                    self.vars[31] = 0;
                    self.vars[17] = 1;
                }
                0x8C => self.g.sound_on = !self.g.sound_on,
                0x8B => self.g.debug = !self.g.debug,
                0x8A => self.vars[27] = 1,
                _ => {}
            }
        }
    }

    /// `$41FC`: op 0F: play sound `n` (1-40), or the current tune again for 0.
    pub fn play_sound(&mut self, n: u8) {
        let s = if n == 0 {
            self.g.music_next.clone()
        } else {
            match self.g.sounds.get(n as usize - 1).cloned().flatten() {
                Some(s) => Some(s),
                None => return,
            }
        };
        self.vars[15] = 0;
        self.g.music_on = true;
        self.g.music = s.clone();
        self.g.music_start = s.clone();
        self.g.music_countdown = 1;
        if self.g.music_next.is_none() {
            self.g.music_next = s;
        }
    }

    /// `$0B1C`: stop the music and silence the chip.
    pub fn stop_music(&mut self) {
        self.g.music_next = None;
        self.g.music_on = false;
        self.sound_out.extend_from_slice(&[0x04, 0x9F, 0xBF, 0xDF, 0xFF]);
    }

    /// A stream in a loaded sound group.
    pub fn stream(group: &super::res::Script, section: usize) -> Stream {
        Stream { data: group.data.clone(), at: section }
    }
}
