//! The actors on screen (`$3FFC`-`$48E2`, `$1875`): the trial step and collisions,
//! drawing and erasing, the background saved under each actor, and the changed
//! rectangles copied to the displayed page.
//!
//! Actors are drawn into the work page against its priorities; `$1875` copies a
//! rectangle of the work page to the displayed page. An actor is on screen while its
//! room is v1, off at FF; room 0 ends the table.

use super::screen::{HEIGHT, WIDTH};
use super::{World, ACTORS};

/// Directions to (x sign, y sign): 0 none, 1 plus, 80h minus (CS:430A, just before the
/// step routine; the PCjr's at CS:2F1B).
const DIRECTIONS: usize = 0x430A;

impl World {
    /// `$42E2`: actor `i` on screen? None at the end of the table (room 0).
    fn on_screen(&self, i: usize) -> Option<bool> {
        match self.actors.get(i).map_or(0, |a| a.room) {
            0 => None,
            r => Some(r == self.vars[1]),
        }
    }

    #[inline]
    fn work(&self, x: i32, y: i32) -> u8 {
        buffer_at(x, y).map_or(0, |i| self.screen.work[i])
    }
    #[inline]
    fn set_work(&mut self, x: i32, y: i32, v: u8) {
        if let Some(i) = buffer_at(x, y) {
            self.screen.work[i] = v;
        }
    }

    /// `$400D`: put the current actor on screen, or leave it off (room FF) if it can't
    /// stand where it is. The ego is first kept inside the right edge, with v3 and v6
    /// cleared.
    pub fn try_draw(&mut self) {
        let a = self.g.current;
        self.actors[a].cel_drawn = self.actors[a].cel_new.clone();
        if self.actors[a].slot == 0 {
            self.vars[3] = 0;
            self.vars[6] = 0;
            let max = 0x9Du8.wrapping_sub(self.actors[a].cel_new.size().1);
            if max <= self.actors[a].x {
                self.actors[a].x = max;
            }
        }
        if self.move_actor(a) || self.actor_contact(a) {
            self.actors[self.g.current].room = 0xFF;
            return;
        }
        // $41A2: drawn where it stands, not where the trial step took it.
        let x = &mut self.actors[a];
        x.room = 0xFF;
        x.new_x = x.x;
        x.new_y = x.y;
        self.restore_all();
        let r = self.vars[1];
        self.actors[self.g.current].room = r;
        self.save_all();
        self.draw_all();
    }

    /// `$3FB7`: count down every actor's delay (to the end of the table); if any
    /// reached 0, the actors are redrawn: backgrounds back, steps, backgrounds saved,
    /// drawn.
    pub fn countdowns(&mut self) {
        self.checkpoint(0x3FB7);
        self.g.current = 0;
        let mut any = false;
        for x in self.actors.iter_mut() {
            if x.room == 0 {
                break;
            }
            if x.countdown == 0 {
                x.countdown = x.delay;
            }
            x.countdown = x.countdown.wrapping_sub(1);
            if x.countdown == 0 {
                any = true;
            }
        }
        if any {
            self.restore_all();
            self.move_all();
            self.save_all();
            self.draw_all();
        }
    }

    /// `$4063`: take the current actor off the screen. The others take their pending
    /// step meanwhile.
    pub fn erase_actor(&mut self) {
        let a = self.g.current;
        let r = self.vars[1];
        self.actors[a].room = r;
        self.restore_all();
        self.move_all();
        self.save_all();
        self.actors[a].room = 0xFF;
        self.draw_all();
        self.actors[a].room = r;
        self.blit_and_commit(a);
        self.actors[a].room = 0xFF;
    }

    /// `$409E`: draw the current actor into the picture for good, and take it off as an
    /// actor.
    pub fn add_to_picture(&mut self) {
        self.restore_all();
        self.move_all();
        let a = self.g.current;
        self.save_bg(a);
        self.draw_cel_permanent(a);
        self.actors[a].room = 0xFF;
        self.save_all();
        self.draw_all();
        self.actors[a].room = self.vars[1];
        self.blit_and_commit(a);
        self.actors[a].room = 0xFF;
    }

    /// `$40D4`: does actor `a`'s bottom row touch the ego's box? 0 if it does; if not,
    /// the value the test's registers held (not 0, but for a coincidence).
    pub fn touches_ego(&self, a: usize) -> u16 {
        let x = &self.actors[a];
        let w = x.cel_new.size().1;
        let e = &self.actors[0];
        let (eh, ew) = e.cel_new.size();
        let top = e.y.wrapping_sub(eh).wrapping_sub(1);
        if x.y < top {
            return (top as u16) << 8 | e.x as u16;
        }
        let eright = e.x.wrapping_add(ew).wrapping_sub(1);
        let right = x.x.wrapping_add(w).wrapping_sub(1);
        if e.x > right || x.x > eright {
            return (e.x as u16) << 8 | eright as u16;
        }
        0
    }

    /// `$41BD`: the background under each actor on screen put back, in slot order.
    fn restore_all(&mut self) {
        for i in 0..4 {
            match self.on_screen(i) {
                None => return,
                Some(true) => self.restore_bg(i),
                Some(false) => {}
            }
        }
    }

    /// `$41D3`: each actor on screen takes its step, in draw order (v3, v21 and v22
    /// cleared first); one that touches another stays where it is.
    fn move_all(&mut self) {
        self.vars[3] = 0;
        self.vars[21] = 0;
        self.vars[22] = 0;
        for slot in self.draw_order() {
            match self.on_screen(slot) {
                None => return,
                Some(true) => {
                    self.move_actor(slot);
                    if self.actor_contact(slot) {
                        let x = &mut self.actors[slot];
                        x.new_x = x.x;
                        x.new_y = x.y;
                    }
                }
                Some(false) => {}
            }
        }
    }

    /// `$4203`: the background under each actor on screen saved, in slot order.
    fn save_all(&mut self) {
        for i in 0..4 {
            match self.on_screen(i) {
                None => return,
                Some(true) => self.save_bg(i),
                Some(false) => {}
            }
        }
    }

    /// `$4219`: the actors on screen drawn in draw order, then each one's rectangle
    /// copied to the displayed page, in slot order.
    fn draw_all(&mut self) {
        for slot in self.draw_order() {
            match self.on_screen(slot) {
                None => break,
                Some(true) => self.draw_cel(slot),
                Some(false) => {}
            }
        }
        for i in 0..4 {
            match self.on_screen(i) {
                None => return,
                Some(true) => self.blit_and_commit(i),
                Some(false) => {}
            }
        }
    }

    /// `$4248`: the draw order of the four records. Each gets a key: its y, or with a
    /// fixed priority the y of that priority's band (its top if the actor is above it,
    /// its bottom if below); past the end of the table, FF. The original sorts these
    /// with three more (key, slot) pairs in its buffer that it never refills: (0, 0) at
    /// first, so the very first sort puts slot 0 four times in front; (FF, 1-3) ever
    /// after, which sort last. The sort is stable.
    fn draw_order(&mut self) -> [usize; 4] {
        let mut pairs = [(0u8, 0u8); 7];
        let mut ended = false;
        for (i, p) in pairs.iter_mut().take(4).enumerate() {
            let x = &self.actors[i];
            if ended || x.room == 0 {
                ended = true;
                *p = (0xFF, i as u8);
                continue;
            }
            let mut key = x.y;
            if x.flags & 1 != 0 {
                let band = x.y.wrapping_sub(0x28) >> 3;
                if band != x.priority {
                    key = (x.priority << 3).wrapping_add(0x28);
                    if band > x.priority {
                        key = key.wrapping_add(7);
                    }
                }
            }
            *p = (key, i as u8);
        }
        pairs[4..].copy_from_slice(&self.g.order_spare);
        pairs.sort_by_key(|p| p.0);
        self.g.order_spare.copy_from_slice(&pairs[4..]);
        [0, 1, 2, 3].map(|i| pairs[i].1 as usize)
    }

    /// `$431C`: actor `a`'s step (if its countdown is 0 and it may move) to its new
    /// position, and whether it is blocked there (then the new position is put back).
    ///
    /// An edge undoes the step on that axis and is reported (v3 for the ego; v21 = the
    /// view and v22 = the edge for the first other actor); the limits are 28h ≤ y <
    /// A6h, 1 < x, and the right end below 9Eh. Then the work page's priorities under
    /// the new bottom row: 15 blocks (unless flag bit 1), 0 blocks (unless bit 2), and
    /// so does the actor's own priority (unless that is 1). For the ego, v4 = 1 when
    /// no priority there is above 0 (then v5 = the colour's dot pattern under its right
    /// end, the PC's picture buffer holding patterns); while
    /// v4 is set, and for view 6Bh, 0 doesn't block and 1 is looked for instead.
    pub fn move_actor(&mut self, a: usize) -> bool {
        let x = &self.actors[a];
        let (h, w) = x.cel_new.size();
        let _ = h;
        let (mut nx, mut ny) = (x.x, x.y);
        let (mut dx, mut dy) = (0u8, 0u8);
        if x.countdown == 0 && x.no_move == 0 {
            let at = DIRECTIONS + 2 * (x.dir as usize & 0x7F);
            let (xs, ys) = (self.res.code[at], self.res.code[at + 1]);
            dx = match xs {
                0 => 0,
                s if s & 0x80 != 0 => 0u8.wrapping_sub(x.step_x),
                _ => x.step_x,
            };
            dy = match ys {
                0 => 0,
                s if s & 0x80 != 0 => 0u8.wrapping_sub(x.step_y),
                _ => x.step_y,
            };
        }
        ny = ny.wrapping_add(dy);
        if !(0x28..0xA6).contains(&ny) {
            let e = if ny < 0x28 { 3 } else { 4 };
            ny = ny.wrapping_sub(dy);
            self.report_edge(a, e);
        }
        let right = nx.wrapping_add(w.wrapping_sub(1)).wrapping_add(dx);
        nx = nx.wrapping_add(dx);
        if right >= 0x9E || nx == 1 || nx >= 0x9E {
            let e = if right >= 0x9E { 2 } else { 1 };
            nx = nx.wrapping_sub(dx);
            self.report_edge(a, e);
        }
        let x = &mut self.actors[a];
        x.new_x = nx;
        x.new_y = ny;
        let flags = x.flags;
        let mut p = if ny < 0x30 { 1 } else { (ny - 0x28) >> 3 } & 0x0F;
        if flags & 1 != 0 {
            p = x.priority;
        }
        let ego = x.slot == 0;
        let watery = if ego { self.vars[4] != 0 } else { x.view_num == 0x6B };
        let view6b = x.view_num == 0x6B;
        let own = |px: u8| px == p && (px != 0x0F || flags & 2 == 0);
        let b15 = |px: u8| flags & 2 == 0 && px == 0x0F;
        let mut count = 0u8;
        let mut last = 0u8;
        let row = ny as i32;
        for i in 0..w.max(1) as i32 {
            let v = self.work(nx as i32 + i, row);
            last = v;
            let px = v & 0x0F;
            let blocked = if !ego && !watery {
                b15(px) || flags & 4 == 0 && px == 0 || p != 1 && own(px)
            } else if watery {
                if px == 1 {
                    count = 1;
                    b15(px)
                } else {
                    b15(px) || own(px)
                }
            } else {
                if px != 0 {
                    count = count.wrapping_add(1);
                }
                b15(px) || px > 1 && own(px)
            };
            if blocked {
                let x = &mut self.actors[a];
                x.new_x = x.x;
                x.new_y = x.y;
                return true;
            }
        }
        if ego || watery {
            if view6b {
                if count != 0 {
                    let x = &mut self.actors[a];
                    x.new_x = x.x;
                    x.new_y = x.y;
                    return true;
                }
            } else {
                self.vars[4] = 0;
                if count == 0 {
                    self.vars[4] = 1;
                    // The PC's picture buffer holds each colour's dot pattern (its table
                    // at DS:12BA), not the colour: v5 is the pattern (light green, 10,
                    // is 11).
                    self.vars[5] = crate::composite::PATTERN[(last >> 4) as usize & 15];
                }
            }
        }
        self.actors[a].priority = p;
        false
    }

    /// An edge reached: v3 for the ego; for another actor v21 = its view and v22 = the
    /// edge, unless one is already reported.
    fn report_edge(&mut self, a: usize, e: u8) {
        if self.actors[a].slot == 0 {
            self.vars[3] = e;
        } else if self.vars[21] == 0 {
            self.vars[21] = self.actors[a].view_num;
            self.vars[22] = e;
        }
    }

    /// `$44F2`: does actor `a`'s new bottom row touch another actor on screen on the
    /// same new row? Two actors other than the ego block each other; between the ego
    /// and another, the other is stopped, and v6 = its view unless it is following
    /// (motion 11h).
    pub fn actor_contact(&mut self, a: usize) -> bool {
        let x = &self.actors[a];
        let right = x.cel_new.size().1.wrapping_add(x.new_x);
        let left = x.new_x.wrapping_sub(1);
        let (ny, own) = (x.new_y, x.slot as usize);
        for i in 0..4 {
            if i == own {
                continue;
            }
            let o = &self.actors[i];
            if o.room == 0 {
                return false;
            }
            if o.room != self.vars[1] {
                continue;
            }
            let oright = o.cel_new.size().1.wrapping_add(o.new_x);
            if ny != o.new_y || right < o.new_x || oright < left {
                continue;
            }
            let follower = if own == 0 {
                i
            } else if i != 0 {
                return true;
            } else {
                own
            };
            let f = &self.actors[follower];
            let v = f.view_num;
            if f.motion != 0x11 {
                self.vars[6] = v;
            }
            return v != 0xFF;
        }
        false
    }

    /// `$4599`: the saved background put back at the actor's current position, the
    /// size of its last drawn cel.
    fn restore_bg(&mut self, a: usize) {
        let x = &self.actors[a];
        let (h, w) = x.cel_drawn.size();
        let top = x.y as i32 - h as i32 + 1;
        let left = x.x as i32;
        let saved = std::mem::take(&mut self.actors[a].saved);
        self.cost.background += (rows(h) * w as i32) as u32;
        let mut k = 0;
        for r in 0..rows(h) {
            for c in 0..w as i32 {
                let v = saved.get(k).copied().unwrap_or(0);
                k += 1;
                self.set_work(left + c, top + r, v);
            }
        }
        self.actors[a].saved = saved;
    }

    /// `$45DC`: the background under the new position saved, the size of the new
    /// cel.
    fn save_bg(&mut self, a: usize) {
        let x = &self.actors[a];
        let (h, w) = x.cel_new.size();
        let top = x.new_y as i32 - h as i32 + 1;
        let left = x.new_x as i32;
        let n = rows(h) as usize * w as usize;
        self.cost.background += n as u32;
        let mut saved = std::mem::take(&mut self.actors[a].saved);
        if saved.len() < n {
            saved.resize(n, 0);
        }
        let mut k = 0;
        for r in 0..rows(h) {
            for c in 0..w as i32 {
                saved[k] = self.work(left + c, top + r);
                k += 1;
            }
        }
        self.actors[a].saved = saved;
    }

    /// The first priority below (x, y), down the column, that isn't 15 (`$46AE`); the
    /// permanent draw drifts a column left on every row after the first (`$47D0`).
    fn below(&mut self, x: i32, y: i32, drift: bool) -> u8 {
        let (mut x, mut y) = (x, y);
        let mut first = true;
        loop {
            y += 1;
            if drift && !first {
                x -= 1;
            }
            first = false;
            self.cost.below += 1;
            if y >= HEIGHT as i32 {
                return 0;
            }
            let q = self.work(x, y) & 0x0F;
            if q != 0x0F {
                return q;
            }
        }
    }

    /// `$461E`: actor `a` drawn at its new position into the work page, behind higher
    /// priorities: a pixel over priority 15 shows if the first priority below it isn't
    /// higher. Its own priority isn't written. Hidden (+1C, and v7 for the ego) when
    /// nothing showed.
    fn draw_cel(&mut self, a: usize) {
        let x = &self.actors[a];
        let cref = x.cel_new.clone();
        let Some(cel) = cref.get() else { return };
        let top = x.new_y as i32 - cel.height as i32 + 1;
        let left = x.new_x as i32;
        let own = x.priority;
        let ego = x.slot == 0;
        let mut shown = 0;
        for r in 0..cel.height as i32 {
            for c in 0..cel.width as i32 {
                let Some(colour) = cel.pixels[(r * cel.width as i32 + c) as usize] else { continue };
                let (px, py) = (left + c, top + r);
                let p = self.work(px, py) & 0x0F;
                let keep = if p <= own {
                    Some(p)
                } else if p == 0x0F {
                    (self.below(px, py, false) <= own).then_some(0x0F)
                } else {
                    None
                };
                if let Some(p) = keep {
                    self.set_work(px, py, colour << 4 | p);
                    shown += 1;
                }
            }
        }
        self.cost.cel_pixels += cel.height as u32 * cel.width as u32;
        self.cost.cel_nibbles += cel.nibbles;
        self.cost.cel_written += shown;
        self.set_hidden(a, ego, shown == 0);
    }

    fn set_hidden(&mut self, a: usize, ego: bool, hidden: bool) {
        self.actors[a].hidden = hidden as u8;
        if ego {
            self.vars[7] = hidden as u8;
        }
    }

    /// `$46CA`: actor `a` drawn into the picture for good, its own priority with its
    /// colours (15 over a priority-15 pixel when the first priority below, drifting
    /// left, isn't higher). Then priority 15 along its bottom row and, unless its y is a
    /// multiple of 8, a box up to its band's top: that row and both sides.
    fn draw_cel_permanent(&mut self, a: usize) {
        let x = &self.actors[a];
        let cref = x.cel_new.clone();
        let Some(cel) = cref.get() else { return };
        let (nx, ny) = (x.new_x, x.new_y);
        let top = ny as i32 - cel.height as i32 + 1;
        let left = nx as i32;
        let own = x.priority;
        let ego = x.slot == 0;
        let mut shown = 0;
        for r in 0..cel.height as i32 {
            for c in 0..cel.width as i32 {
                let Some(colour) = cel.pixels[(r * cel.width as i32 + c) as usize] else { continue };
                let (px, py) = (left + c, top + r);
                let p = self.work(px, py) & 0x0F;
                let v = if p == 0x0F {
                    (self.below(px, py, true) <= own).then_some(colour << 4 | 0x0F)
                } else {
                    (p <= own).then_some(colour << 4 | own)
                };
                if let Some(v) = v {
                    self.set_work(px, py, v);
                    shown += 1;
                }
            }
        }
        self.set_hidden(a, ego, shown == 0);
        let w = cel.width as i32;
        self.mark15(nx as i32, ny as i32, w, 1);
        let band = ny & 0xF8;
        if band == ny {
            return;
        }
        self.mark15(nx as i32, band as i32, w, 1);
        let from = band + 1;
        if from == ny {
            return;
        }
        let n = (ny - from) as i32;
        self.mark15(nx as i32, from as i32, 1, n);
        self.mark15(nx as i32 + w - 1, from as i32, 1, n);
    }

    /// `$476E`/`$478C`: priority 15 over a `w` × `h` block.
    fn mark15(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for r in 0..h {
            for c in 0..w {
                let v = self.work(x + c, y + r) | 0x0F;
                self.set_work(x + c, y + r, v);
            }
        }
    }

    /// `$4865`: the rectangle the actor was in and now is in copied to the displayed
    /// page (`$1875`: each pixel's colour), then its new position and cel committed.
    /// The rectangle takes the larger cel and adds the step on an axis it moved along
    /// (not the distance), its width rounded up to even.
    fn blit_and_commit(&mut self, a: usize) {
        let x = &self.actors[a];
        let (oh, ow) = x.cel_drawn.size();
        let (nh, nw) = x.cel_new.size();
        let top = (x.y.wrapping_sub(oh).wrapping_add(1)).min(x.new_y.wrapping_sub(nh).wrapping_add(1));
        let mut left = x.x.min(x.new_x);
        let mut rows_n = oh.max(nh);
        let mut cols = ow.max(nw);
        if x.y != x.new_y {
            rows_n = rows_n.wrapping_add(x.step_y);
        }
        if x.x != x.new_x {
            cols = cols.wrapping_add(x.step_x);
        }
        if cols & 1 != 0 {
            cols = cols.wrapping_add(1);
        }
        // The PC's copy to the screen (`$1875`) takes two pixels to a CGA byte, from an
        // even one: an odd left edge goes one back, the width 2 wider.
        if left & 1 != 0 {
            left -= 1;
            cols = cols.wrapping_add(2);
        }
        self.cost.blit += (rows(rows_n) * cols as i32) as u32;
        for r in 0..rows(rows_n) {
            for c in 0..cols as i32 {
                let (px, py) = (left as i32 + c, top as i32 + r);
                // Unclipped: past the right edge it reads on into the buffer (row y + 4,
                // its layout's next 160 bytes) and writes on into the screen (line y + 2,
                // the CGA's next 80 bytes).
                let (src, dst) = if px < WIDTH as i32 { (py, py) } else { (py + 4, py + 2) };
                let qx = px % WIDTH as i32;
                if let (Some(s), true) = (buffer_at(qx, src), (0..HEIGHT as i32).contains(&dst)) {
                    let v = self.screen.work[s] & 0xF0;
                    self.screen.shown[dst as usize * WIDTH + qx as usize] = v | v >> 4;
                }
            }
        }
        let x = &mut self.actors[a];
        x.x = x.new_x;
        x.y = x.new_y;
        x.cel_drawn = x.cel_new.clone();
        debug_assert!(a < ACTORS);
    }
}

/// Pixel (x, y) of the picture buffer, as its layout addresses it: rows of 160 in four
/// banks, so x 160-319 is the start of row y + 4 (where a cel drawn past the right edge
/// lands); off the picture's 168 rows, nothing that is shown.
fn buffer_at(x: i32, y: i32) -> Option<usize> {
    let (x, y) = if (WIDTH as i32..2 * WIDTH as i32).contains(&x) { (x - WIDTH as i32, y + 4) } else { (x, y) };
    ((0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y)).then(|| y as usize * WIDTH + x as usize)
}

/// A count in a byte decremented to 0: 0 means 256.
fn rows(n: u8) -> i32 {
    if n == 0 {
        256
    } else {
        n as i32
    }
}
