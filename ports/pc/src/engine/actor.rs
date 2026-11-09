//! The game cycle (`$293A`), each actor's update (`$2AAD`), and the actions that load
//! things and add, place and clear actors (`$0B37`-`$112D`, `PCjr $4AFC`-`$2063`).

use super::res::{Script, View};
use super::save::SAVE_OFFSETS;
use super::{Actor, CelRef, World, ACTORS};
use std::rc::Rc;

/// The loop for each direction, for views with 2 loops (DS:1D55) and 4 (DS:1D5E).
const LOOPS_2: usize = 0x1D55;
const LOOPS_4: usize = 0x1D5E;
/// Where the ego comes in on each edge (DS:114B): x for edges 1 and 2, y for 3 and 4.
const EDGE_ENTRY: usize = 0x114B;
/// The prompt (DS:1136).
pub const PROMPT: usize = 0x1136;

impl World {
    // ---- Loading.

    /// `$1FB6`: a disk read of `n` sectors; the timer stops reading keys meanwhile.
    pub(super) fn read_sectors(&mut self, n: u16) {
        self.g.keys_on = false;
        self.checkpoint(0x1FB6);
        self.site(super::clock::Site::Disk);
        self.cost.sectors += n as u32;
        self.g.keys_on = true;
    }

    /// `$1D34`: room-table entry `n`'s script (v1 = n; v3, v6, v21, v22 = 0). It goes
    /// at the top of the resource memory (`$1F29`), which then moves past its sections.
    pub fn load_room_script(&mut self, n: u8) -> Rc<Script> {
        self.vars[3] = 0;
        self.vars[6] = 0;
        self.vars[21] = 0;
        self.vars[22] = 0;
        self.vars[1] = n;
        for r in self.res.script_reads(n) {
            self.read_sectors(r);
        }
        let s = self.res.script(n);
        let base = self.g.heap;
        let at = |o: usize| base.wrapping_add(o as u16);
        self.g.room_ptrs = [base, at(s.cycle), at(s.parser), at(s.messages)];
        self.g.heap = at(s.data.len());
        s
    }

    /// `$1D55`: room `n`'s picture, for op 06 or 25 to draw.
    pub fn load_picture(&mut self, n: u8) {
        for r in self.res.picture_reads(n) {
            self.read_sectors(r);
        }
        self.g.picture = Some(self.res.picture(n));
    }

    /// `$1DDE`: view `v`, read from the disk (every time it is asked for), at the top
    /// of the resource memory, which moves past it; and where it went. An actor whose
    /// record (from a restored save) points there has it from now on.
    pub fn load_view(&mut self, v: u8) -> (Option<Rc<View>>, u16) {
        for r in self.res.view_reads(v) {
            self.read_sectors(r);
        }
        let x = self.res.view(v);
        let addr = self.g.heap;
        match &x {
            Some(view) => {
                self.g.heap = addr.wrapping_add(view.len);
                for a in &mut self.actors {
                    if a.pending_view == Some(addr) {
                        a.pending_view = None;
                        a.view = Some(view.clone());
                        for c in [&mut a.cel_drawn, &mut a.cel_new] {
                            if let Some((lp, cel)) = view.cel_at(c.ptr.wrapping_sub(addr) as usize) {
                                *c = CelRef { view: Some(view.clone()), lp, cel, ptr: c.ptr };
                            }
                        }
                    }
                }
            }
            None => self.log.push(format!("view {v} isn't on the disk")),
        }
        (x, addr)
    }

    /// `$0B3A`: op 2C: the group of four sounds holding sound `n`: room-table entry
    /// 5Ah + (n − 1) / 4, read as a script, its sections the four streams. v1 and v2 are
    /// kept; v3, v6, v21 and v22 are cleared by the loader.
    pub fn load_sound_group(&mut self, n: u8) {
        let k = n.wrapping_sub(1);
        self.wait_print_done();
        let keep = (self.vars[1], self.vars[2]);
        let s = self.load_room_script((k >> 2).wrapping_add(0x5A));
        (self.vars[1], self.vars[2]) = keep;
        let base = (k & 0xFC) as usize;
        for (i, at) in [s.actions, s.cycle, s.parser, s.messages].into_iter().enumerate() {
            if let Some(slot) = self.g.sounds.get_mut(base + i) {
                *slot = Some(Self::stream(&s, at));
            }
        }
    }

    // ---- Views, loops and cels.

    /// `$1E2B`: actor `a` to `cel` of loop `lp`; the loop's cel count.
    pub fn select_cel(&mut self, a: usize, lp: u8, cel: u8) -> u8 {
        let x = &mut self.actors[a];
        let ptr = x.view.as_ref().map_or(0, |v| x.view_addr.wrapping_add(v.cel_offset(lp, cel) as u16));
        x.cel_new = CelRef { view: x.view.clone(), lp, cel, ptr };
        x.header(lp)[3]
    }

    /// `$1E7D`: as `$1E2B`, and the loop's delay and steps.
    pub fn select_loop_cel(&mut self, a: usize, lp: u8, cel: u8) -> u8 {
        self.select_loop(a, lp);
        self.select_cel(a, lp, cel)
    }

    /// `$1ED2`: the loop's delay and steps only.
    pub fn select_loop(&mut self, a: usize, lp: u8) -> u8 {
        let x = &mut self.actors[a];
        let h = x.header(lp);
        x.step_x = h[0];
        x.step_y = h[1];
        x.delay = h[2];
        h[3]
    }

    /// `$1009`: cel 0 of the loop facing the viewer: loop 0 for a view with one loop, 1
    /// with two, 2 with more.
    fn set_default_loop(&mut self, a: usize) {
        let lp = match self.actors[a].loops() {
            1 => 0,
            2 => 1,
            _ => 2,
        };
        self.set_loop_cel(a, lp, 0);
    }

    /// `$1019`: loop `lp`, cel `cel`; the cel number itself isn't stored.
    fn set_loop_cel(&mut self, a: usize, lp: u8, cel: u8) {
        let n = self.select_loop_cel(a, lp, cel);
        self.actors[a].cel_count = n;
        self.actors[a].lp = lp;
    }

    // ---- The game cycle.

    /// `$293A`: one pass. The ego takes the direction of the keypad (or joystick); the
    /// actors update (the others in slot order, then the ego); they are drawn; then the
    /// cycle rules run, the global script's (again while they ask) and the room's.
    pub fn game_cycle(&mut self) {
        self.checkpoint(0x293A);
        self.passes += 1;
        let row = self.pass_row();
        self.trace.push(row);
        if self.keep_vars {
            self.vars_trace.push(self.vars);
        }
        // The pass that ended: its work, with the characters the BIOS drew in it.
        self.charge();
        self.charged = 0.0;
        if let Some(n) = self.fixed_pass_cycles {
            self.cycles = self.cycles.max(self.pass_start + n);
            self.pass_start = self.cycles;
        }
        self.cost.chars = std::mem::take(&mut self.text.drawn);
        self.cost.scrolled = std::mem::take(&mut self.text.scrolled);
        let done = std::mem::take(&mut self.cost);
        self.loading_before += done.loading_cycles();
        if let Some(t) = self.cost_trace.as_mut() {
            t.push(done);
        }
        if self.frame_hashes.is_some() || self.frame_at == Some(self.passes) {
            let f = self.frame();
            if let Some(h) = self.frame_hashes.as_mut() {
                h.push(super::frame_hash(&f));
            }
            if self.frame_at == Some(self.passes) {
                self.frame = Some(f);
            }
        }
        if self.probe.is_some_and(|(a, b)| (a..=b).contains(&self.passes)) {
            let e = &self.actors[0];
            eprintln!(
                "pass {} ego ({}, {}) motion {:02X} dir {} no_move {} key_dir {} bios {:04X?} ring {}",
                self.passes,
                e.x,
                e.y,
                e.motion,
                e.dir,
                e.no_move,
                self.g.key_dir,
                self.keys,
                self.g.ring.len()
            );
        }
        if self.passes > self.max_passes {
            self.stop(format!("{} passes", self.max_passes));
        }
        self.site(super::clock::Site::CycleStart);
        self.g.saved_dir = self.actors[0].dir;
        let mut d = if self.g.key_dir != 0 { self.g.key_dir } else { self.g.joy_dir };
        if self.vars[30] != 0 {
            d = 0;
        }
        let e = &mut self.actors[0];
        if !self.g.program && (e.dir == 0 || e.motion == e.dir) {
            e.motion = d;
        }
        if e.cycle < 2 {
            e.cycle = if self.g.program2 || d != 0 { 0 } else { 1 };
        }
        let mut slot = 1u8;
        while slot <= self.g.actor_count && (slot as usize) < ACTORS {
            let x = &self.actors[slot as usize];
            if x.room == self.vars[1] && x.countdown == 0 {
                self.update_actor(slot as usize);
            }
            slot += 1;
        }
        let e = &self.actors[0];
        if e.room == self.vars[1] && e.countdown == 0 {
            self.update_actor(0);
        }
        let e = &mut self.actors[0];
        if !self.g.program && e.motion <= 8 && e.motion != e.dir && e.cycle == 0 && e.view_num == 0 {
            e.dir = 0;
        }
        if !self.g.restoring {
            self.countdowns();
        }
        if self.actors[0].cycle == 0 {
            self.actors[0].dir = self.g.saved_dir;
        }
        if let Some(g) = self.global.clone() {
            while self.match_cycle_rules(&g) != 0 {}
        }
        if let Some(r) = self.room.clone() {
            self.match_cycle_rules(&r);
        }
        self.vars[17] = 0;
    }

    /// `$2AAD`: actor `a`'s update: its motion and the loop for its direction, the
    /// barriers, then its animation.
    fn update_actor(&mut self, a: usize) {
        self.checkpoint(0x2AAD);
        if self.actors[a].motion == 0 {
            self.actors[a].dir = 0;
        } else {
            let m = self.actors[a].motion;
            if m > 8 {
                self.motion(a);
            } else {
                self.actors[a].dir = m;
            }
            self.choose_loop(a);
        }
        self.barriers(a);
        self.animate(a);
    }

    /// `$2AF3`: the loop facing the actor's direction (views with 2 or 4 loops); a
    /// pending set_loop (op 38) for its view; then the cel (kept, or the loop's last).
    fn choose_loop(&mut self, a: usize) {
        let x = &self.actors[a];
        let dir = x.dir as usize;
        let mut new = match x.loops() {
            4 => self.res.ds[LOOPS_4 + dir],
            2 => self.res.ds[LOOPS_2 + dir],
            1 => return,
            n => {
                // The original takes an uninitialised local here; no view of the game
                // has such a count.
                self.log.push(format!("view {} has {n} loops: loop left as it was", x.view_num));
                x.lp
            }
        };
        if self.actors[a].view_num == self.g.pending_loop {
            new = self.actors[a].lp;
            self.actors[a].lp = new.wrapping_add(1);
            self.g.pending_loop = 0xFF;
        }
        if new == self.actors[a].lp {
            return;
        }
        let cel = self.actors[a].cel;
        let n = self.select_cel(a, new, cel);
        self.actors[a].cel_count = n;
        if n <= cel {
            let c = n.wrapping_sub(1);
            self.actors[a].cel = c;
            self.select_cel(a, new, c);
        }
        self.actors[a].lp = new;
    }

    /// `$2BAB`: the barriers (op 20 for x, op 21 for y): an actor in a band on the side
    /// of a barrier, heading across it, is stopped (no-move 2). The bands are worked out
    /// in 16 bits (no wraparound).
    fn barriers(&mut self, a: usize) {
        let g = &self.g;
        let x = &mut self.actors[a];
        if g.barrier_x == 0 && g.barrier_y == 0 {
            if x.no_move == 2 {
                x.no_move = 0;
            }
            return;
        }
        if x.no_move == 1 {
            return;
        }
        let d = x.dir as i32;
        if g.barrier_x != 0 {
            let b = g.barrier_x as i32;
            let px = x.x as i32;
            x.no_move = if (b - g.band_x.0 as i32..=b).contains(&px) && d > 1 && d < 5
                || (b..=b + g.band_x.1 as i32).contains(&px) && d > 5 && d <= 8
            {
                2
            } else {
                0
            };
        }
        if g.barrier_y != 0 {
            let b = g.barrier_y as i32;
            let py = x.y as i32;
            x.no_move = if (b - g.band_y.0 as i32..=b).contains(&py) && d > 3 && d < 7
                || (b..=b + g.band_y.1 as i32).contains(&py) && (d == 8 || d < 3)
            {
                2
            } else {
                0
            };
        }
    }

    /// `$2CEA`: the animation, by cycle mode (jump table CS:19FC).
    fn animate(&mut self, a: usize) {
        let x = &self.actors[a];
        let (lp, cel, count) = (x.lp, x.cel, x.cel_count);
        match x.cycle {
            // Cycle through the loop.
            0 => {
                let mut c = cel.wrapping_add(1);
                if c >= count {
                    c = 0;
                }
                self.actors[a].cel = c;
                self.select_cel(a, lp, c);
            }
            // Back to cel 0, then still (v8 = 1).
            2 => {
                if cel != 0 {
                    self.actors[a].cel = cel - 1;
                    self.select_cel(a, lp, cel - 1);
                } else {
                    self.actors[a].cycle = 1;
                    self.vars[8] = 1;
                }
            }
            // On to the last cel, then still (v8 = 1; the ego back to the player).
            3 => {
                if cel as i32 == count as i32 - 1 {
                    self.actors[a].cycle = 1;
                    self.vars[8] = 1;
                    if a == 0 {
                        self.g.program2 = false;
                        self.g.program = false;
                    }
                } else {
                    self.actors[a].cel = cel.wrapping_add(1);
                    self.select_cel(a, lp, cel.wrapping_add(1));
                }
            }
            4 => self.actors[a].cycle = 3,
            // Op 2D's special view: on to its last cel, then back to view 0, walking
            // right (3) by the kept step.
            5 => {
                if cel as i32 == count as i32 - 1 {
                    self.set_ego_view(0, false);
                    self.vars[8] = 1;
                    let x = &mut self.actors[a];
                    x.step_x = self.g.special_step;
                    x.dir = 3;
                    x.cycle = 7;
                } else {
                    self.actors[a].cel = cel.wrapping_add(1);
                    self.select_loop_cel(a, lp, cel.wrapping_add(1));
                    self.actors[a].dir = 0;
                }
            }
            6 => {
                let x = &mut self.actors[a];
                x.step_x = self.g.special_step;
                x.dir = 7;
                x.cycle = 5;
            }
            7 => {
                if a == 0 {
                    self.g.program2 = false;
                    self.g.program = false;
                }
                let x = &mut self.actors[a];
                x.cycle = 1;
                x.dir = 0;
                x.flags = 0;
                self.select_loop(a, 0);
            }
            _ => {}
        }
    }

    // ---- Actions on actors.

    /// `$10A4`: op 01: the ego, with view 0 read again, at object 0's position, in
    /// room 1 (v1 = 1), loop 0 (with cel 6's pointer, cel number 0).
    pub fn reset_ego(&mut self) {
        self.wait_print_done();
        let (v, addr) = self.load_view(0);
        self.view_table[0] = v.clone();
        self.view_table_addr[0] = addr;
        let (ox, oy) = (self.objects[0].x, self.objects[0].y);
        let save_offset = self.res.ds_word(SAVE_OFFSETS);
        let e = &mut self.actors[0];
        e.view = v;
        e.view_addr = addr;
        e.view_num = 0;
        e.slot = 0;
        e.save_offset = save_offset;
        e.cel = 0;
        e.lp = 0;
        e.countdown = 1;
        e.x = ox;
        e.y = oy;
        e.cel_count = 8;
        self.select_loop_cel(0, 0, 6);
        self.vars[1] = 1;
        self.nudge_actor(0);
        self.g.current = 0;
        if !self.g.restoring {
            self.erase_actor();
        }
    }

    /// `$0F99`: op 02: an actor for object `n` (view `n`, read from the disk), at the
    /// object's position, off screen until drawn.
    pub fn add_actor(&mut self, n: u8) {
        if self.g.actor_count > 3 {
            return;
        }
        self.g.actor_count += 1;
        let slot = self.g.actor_count as usize;
        self.actors[slot] = Actor::default();
        self.wait_print_done();
        let (v, addr) = self.load_view(n);
        let o = &mut self.objects[n as usize];
        o.slot = slot as u8;
        let (ox, oy) = (o.x, o.y);
        let save_offset = self.res.ds_word(SAVE_OFFSETS + 2 * slot);
        let x = &mut self.actors[slot];
        x.view = v;
        x.view_addr = addr;
        x.slot = slot as u8;
        x.save_offset = save_offset;
        x.room = 0xFF;
        x.view_num = n;
        x.x = ox;
        x.y = oy;
        self.set_default_loop(slot);
        let x = &mut self.actors[slot];
        if x.delay == 0 {
            x.delay = 1;
        }
        x.countdown = x.delay;
    }

    /// `$102E`: op 03: the actors (not the ego) go: each in this room leaves its
    /// position, loop and cel in its object. The objects lose their actors.
    pub fn clear_actors(&mut self) {
        for slot in 1..ACTORS {
            let room = self.actors[slot].room;
            if room == 0 {
                break;
            }
            self.actors[slot].room = 0;
            if room != self.vars[1] {
                continue;
            }
            let x = &self.actors[slot];
            let o = &mut self.objects[x.view_num as usize];
            o.room = room | (o.room & 0x80);
            o.loop_cel = x.cel | x.lp << 4;
            o.x = x.x;
            o.y = x.y;
        }
        for o in &mut self.objects[1..=110] {
            o.slot = 0;
        }
        self.g.actor_count = 0;
        self.actors[1].room = 0;
        // $108F: the resource memory back to the room's script.
        self.g.heap = self.g.room_marks[0];
    }

    /// `$0EDD`: ops 04 and 37: room `r`.
    pub fn new_room(&mut self, r: u8) {
        self.g.room_changed = 1;
        // The ego comes in at the edge it left by.
        let e = self.vars[3].wrapping_sub(1);
        if e <= 3 {
            self.vars[3] = 0;
            let at = self.res.ds[EDGE_ENTRY + e as usize];
            if e >> 1 == 0 {
                self.actors[0].x = at;
            } else {
                self.actors[0].y = at;
            }
        }
        let old = self.vars[1];
        self.vars[18] = old;
        // v14: a random bit, set when a byte of the engine's code has even parity.
        let b = self.res.code_byte(self.g.random);
        self.g.random = self.g.random.wrapping_add(1);
        if self.g.random >= 0x4000 {
            self.g.random = 0x0B00;
        }
        self.vars[14] = b.count_ones().is_multiple_of(2) as u8;
        // Actors 1-3 in the old room leave their position in an object: the one
        // numbered by object 0's x (the ego's stored x), not their own (a bug).
        for slot in 1..=3 {
            if self.actors[slot].room == old {
                let i = self.objects[0].x as usize;
                let (x, y) = (self.actors[slot].x, self.actors[slot].y);
                match self.objects.get_mut(i) {
                    Some(o) => {
                        o.x = x;
                        o.y = y;
                    }
                    None => self.log.push(format!("new room: object {i} doesn't exist")),
                }
            }
        }
        self.stop_music();
        self.wait_print_done();
        self.room = Some(self.load_room_script(r));
        self.extra = None;
        self.g.barrier_x = 0;
        self.g.barrier_y = 0;
        let e = &mut self.actors[0];
        e.motion = 0;
        e.dir = 0;
        e.flags = 0;
        self.clear_input_rows();
        self.queue_engine_string(PROMPT);
        self.wait_print_done();
        self.g.text_col = 2;
        self.wait_print_done();
        self.text.cursor = (0x18, 2);
        // $0F83: the room script's sections kept, for clearing the actors.
        self.g.room_marks = self.g.room_ptrs;
    }

    /// `$110D`: the text rows 21-24 cleared; the cursor to the start of row 24.
    pub fn clear_input_rows(&mut self) {
        self.wait_print_done();
        self.text.scroll(0, 0x15, 0x18);
        self.g.text_col = 0;
        self.wait_print_done();
        self.text.cursor = (0x18, 0);
    }

    /// `$0D6A`: put actor `slot` on screen where it stands, or else at the nearest spot
    /// where it fits, going round in a square spiral (up, right, down, left), within
    /// 2 ≤ x ≤ 157 − width and 2 ≤ y < A8h. Nothing is drawn while restoring.
    pub fn nudge_actor(&mut self, slot: u8) {
        if slot > self.g.actor_count {
            return;
        }
        let a = slot as usize;
        self.actors[a].dir = 0;
        let max_x = 0x9Du8.wrapping_sub(self.actors[a].cel_new.size().1);
        let mut spiral = Spiral::default();
        loop {
            if self.g.restoring {
                return;
            }
            self.g.current = a;
            self.try_draw();
            if self.actors[a].room != 0xFF {
                return;
            }
            loop {
                spiral.step(&mut self.actors[a]);
                let (x, y) = (self.actors[a].x, self.actors[a].y);
                if x > 1 && y > 1 && y < 0xA8 && x <= max_x {
                    break;
                }
            }
        }
    }

    /// `$0E14`: op 26: object `o` drawn into the picture at its actor's position, with
    /// loop `l` and cel `c`.
    pub fn place_object(&mut self, o: u8, l: u8, c: u8) {
        let slot = self.objects[o as usize].slot;
        let a = slot as usize;
        let ob = &mut self.objects[o as usize];
        let carried = ob.room >= 0x80;
        ob.room = self.vars[1].wrapping_add(if carried { 0x80 } else { 0 });
        ob.x = self.actors[a].x;
        ob.y = self.actors[a].y;
        self.actors[a].lp = l;
        self.actors[a].cel = c;
        let n = self.select_cel(a, l, c);
        self.actors[a].cel_count = n;
        self.nudge_actor(slot);
        self.g.current = a;
        self.add_to_picture();
    }

    /// `$0E60`: op 25: room `n`'s picture over the current one; the actors taken off
    /// and put back.
    pub fn overlay_picture(&mut self, n: u8) {
        self.wait_print_done();
        self.load_picture(n);
        self.draw_loaded_picture();
        let r = self.vars[1];
        for a in 0..ACTORS {
            match self.actors[a].room {
                0 => break,
                x if x == r => {
                    self.g.current = a;
                    self.erase_actor();
                    self.actors[a].room = r;
                }
                _ => {}
            }
        }
        self.merge_colours();
        for a in 0..ACTORS {
            match self.actors[a].room {
                0 => break,
                x if x == r => {
                    self.g.current = a;
                    self.try_draw();
                }
                _ => {}
            }
        }
    }

    /// `$0B9A`: op 2D (view v, step n): the ego in a special view (v26 = 1), moved left
    /// by n, animating through it under the program's control (cycle mode 5).
    pub fn ego_special_view(&mut self, v: u8, n: u8) {
        self.vars[8] = 0;
        self.vars[26] = 1;
        self.g.program = true;
        self.actors[0].cycle = 5;
        self.actors[0].flags = 7;
        self.g.kept_step = self.actors[0].step_x;
        self.g.current = 0;
        self.erase_actor();
        self.g.special_step = n;
        let e = &mut self.actors[0];
        e.x = e.x.wrapping_sub(n);
        e.view_num = v;
        e.view = self.view_table[v as usize].clone();
        e.view_addr = self.view_table_addr[v as usize];
        e.pending_view = None;
        e.cel = 0;
        self.g.kept_loop_cel = (e.lp, 0);
        self.set_default_loop(0);
        self.try_draw();
    }

    /// `$0BF8` (op 1C) and `$0C00` (op 41, `fit`): the ego takes view v, if it is
    /// loaded (v26 = v). Leaving view 0 its loop and cel are kept, and they come back
    /// with view 0; other views start on the loop facing the viewer. Back to view 0
    /// from view 46h on land (v3 = 0), it is put on dry land. Op 41 then puts it in
    /// water for view 5Dh (except in room 31h), or back inside the right edge.
    pub fn set_ego_view(&mut self, v: u8, fit: bool) {
        self.g.fit_check = fit;
        self.vars[26] = v;
        let Some(view) = self.view_table[v as usize].clone() else { return };
        let e = &mut self.actors[0];
        e.view = Some(view);
        e.view_addr = self.view_table_addr[v as usize];
        e.pending_view = None;
        e.cel = 0;
        let old = e.view_num;
        e.view_num = v;
        if old == 0 {
            self.g.kept_loop_cel = (e.lp, e.cel);
            self.set_default_loop(0);
        } else if v == 0 {
            let (l, c) = self.g.kept_loop_cel;
            self.set_loop_cel(0, l, c);
        } else {
            self.set_default_loop(0);
        }
        if old == 0x46 && self.vars[3] == 0 && v == 0 {
            self.place_ego_by_water(v);
            return;
        }
        if !self.g.fit_check {
            return;
        }
        if self.vars[1] != 0x31 && v == 0x5D {
            self.place_ego_by_water(v);
            return;
        }
        let w = self.actors[0].cel_new.size().1;
        if w.wrapping_add(self.actors[0].x) > 0x9D {
            self.g.current = 0;
            self.erase_actor();
            self.actors[0].x = 0x9D - w;
            self.g.current = 0;
            self.try_draw();
        }
    }

    /// `$0CB0`: the ego put where it is in water (view 5Dh) or on land (the others),
    /// trying where it stands, then (from x 14) 15 to the left, then spiralling out.
    fn place_ego_by_water(&mut self, v: u8) {
        self.g.water_target = (v == 0x5D) as u8;
        self.g.current = 0;
        self.erase_actor();
        self.actors[0].dir = 0;
        let max_x = 0x9Du8.wrapping_sub(self.actors[0].cel_new.size().1);
        if self.try_ego_spot() {
            return;
        }
        if self.actors[0].x >= 0x0E {
            self.actors[0].x -= 0x0F;
            if self.try_ego_spot() {
                return;
            }
            self.actors[0].x = self.actors[0].x.wrapping_add(0x0F);
        }
        let mut spiral = Spiral::default();
        loop {
            if self.try_ego_spot() {
                return;
            }
            loop {
                spiral.step(&mut self.actors[0]);
                let (x, y) = (self.actors[0].x, self.actors[0].y);
                if x > 1 && x <= max_x && y > 1 && y <= 0xA6 {
                    break;
                }
            }
        }
    }

    /// `$0D43`: does the ego fit here, in water or not as wanted? If so, it is drawn.
    fn try_ego_spot(&mut self) -> bool {
        let want = self.g.water_target;
        self.vars[4] = want ^ 1;
        if self.move_actor(0) || self.vars[4] != want {
            return false;
        }
        self.try_draw();
        true
    }
}

/// The square spiral of `$0D95`/`$0CF1`: runs of 1, 1, 2, 2, 3, 3... up, right, down,
/// left.
struct Spiral {
    left: u8,
    len: u8,
    side: u8,
}

impl Default for Spiral {
    fn default() -> Self {
        Spiral { left: 1, len: 1, side: 4 }
    }
}

impl Spiral {
    fn step(&mut self, a: &mut Actor) {
        match self.side {
            4 => a.y = a.y.wrapping_sub(1),
            3 => a.x = a.x.wrapping_add(1),
            2 => a.y = a.y.wrapping_add(1),
            1 => a.x = a.x.wrapping_sub(1),
            _ => {}
        }
        if matches!(self.side, 3 | 1) && self.left == 1 {
            self.len = self.len.wrapping_add(1);
        }
        self.left = self.left.wrapping_sub(1);
        if self.left == 0 {
            self.left = self.len;
            self.side -= 1;
            if self.side == 0 {
                self.side = 4;
            }
        }
    }
}
