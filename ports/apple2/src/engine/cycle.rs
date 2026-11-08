//! The game cycle ($4026): keyboard direction, actor update ($4BF0), delay counters and
//! the draw pass ($5D05), timers ($40D0) and cycle rules ($40BA). See docs/motion.md.

use super::{f, World, MAX_ACTORS};

/// Direction to loop, for views with two loops ($6D69) and with three or more ($6D72).
/// 4 = keep the current loop.
const LOOPS_2: [u8; 9] = [4, 4, 1, 1, 1, 4, 0, 0, 0];
const LOOPS_4: [u8; 9] = [4, 3, 1, 1, 1, 2, 0, 0, 0];
/// $6EF1: direction from (x sign, y sign), each 0 = target smaller, 1 = within slack, 2 = larger.
const DIR_GRID: [u8; 9] = [8, 7, 6, 1, 0, 5, 2, 3, 4];

impl World {
    /// $52C0: a direction key. Pressing the current direction again stops the ego.
    pub fn press_direction(&mut self, d: u8) {
        self.g.key_dir = if d == self.g.key_dir { 0 } else { d };
        self.vars[31] = 0;
    }

    /// One pass of the main loop ($4026). Keys arrive through `key` between cycles, which
    /// corresponds to the keyboard step ($52A8); the prompt redraw that starts the
    /// engine's next pass ($4029-$4036) is done at the end of this one.
    pub fn cycle(&mut self) {
        self.pic_draws.clear();
        self.poll_joystick(); // $4039 -> $52A8 -> $4F0E -> $4846
        self.handle_line(); // $4044-$4076
        if std::mem::take(&mut self.g.abort_pass) {
            self.refresh_prompt();
            return;
        }
        self.update_actors(); // $4BF0
        self.g.cycle = self.g.cycle.wrapping_add(1); // INC $4D
        self.timers(); // $40D0
        if self.g.restore_request != 0 {
            self.restore_game(); // $4080 -> $4F56
        }
        self.run_cycle_rules(); // $40BA
        self.g.abort_pass = false;
        let chars = self.cost.chars;
        self.refresh_prompt(); // $4029-$4036 of the next pass
        self.cost.next_chars += self.cost.chars - chars;
        self.cost.chars = chars;
    }

    /// $4817 (power-on): the joystick's resting position sets the thresholds, half of it
    /// either side. A paddle count of 0 (nothing connected: the timer never runs) leaves
    /// the joystick off.
    pub(crate) fn calibrate_joystick(&mut self) {
        let j = self.io.as_mut().map(|io| io.joystick()).unwrap_or_default();
        let (x, y) = j.paddles.unwrap_or((0, 0));
        if x == 0 {
            self.g.joy_x = (self.g.joy_x.0, 0);
            return;
        }
        self.g.joy_x = (x - x / 2, x.wrapping_add(x / 2));
        self.g.joy_y = (y - y / 2, y.wrapping_add(y / 2));
    }

    /// $4846, once a pass before the typed line is handled: button 0 (Open-Apple) sets
    /// v16 and button 1 (Solid-Apple) v17, each restarting the "more" line count ($1D);
    /// then the stick gives a direction ($67F4), used while no key has set one.
    fn poll_joystick(&mut self) {
        let j = self.io.as_mut().map(|io| io.joystick()).unwrap_or_default();
        if j.buttons[0] {
            self.vars[16] = 1;
            self.text.lines = 0;
        }
        if j.buttons[1] {
            self.vars[17] = 1;
            self.text.lines = 0;
        }
        self.g.joy_dir = 0;
        if self.g.joy_x.1 == 0 {
            return;
        }
        let (x, y) = j.paddles.unwrap_or((0, 0));
        let zone = |v: u8, (lo, hi): (u8, u8)| {
            if v < lo {
                0
            } else if v >= hi {
                2
            } else {
                1
            }
        };
        // $67F4, by 3 * column + row: up-left, left, down-left; up, none, down; up-right,
        // right, down-right.
        const DIRS: [u8; 9] = [8, 7, 6, 1, 0, 5, 2, 3, 4];
        self.g.joy_dir = DIRS[3 * zone(x, self.g.joy_x) + zone(y, self.g.joy_y)];
        if self.g.joy_dir != 0 {
            self.vars[31] = 0;
        }
    }

    /// $40D0
    fn timers(&mut self) {
        for v in [2usize, 9, 10, 11, 12] {
            if self.vars[v] != 0 {
                self.vars[v] -= 1;
            }
        }
        // $40E5: after 10 idle cycles the "more" line count starts again.
        if self.text.idle != 0 {
            self.text.idle -= 1;
            if self.text.idle == 0 {
                self.text.lines = 0;
            }
        }
    }

    /// $52A8: a key press. Direction keys steer the ego; anything else goes to the
    /// input line and the function keys ($5312).
    pub fn key(&mut self, k: u8) {
        let dir = match k {
            0x15 => 3,
            0x08 => 7,
            0x0B => 1,
            0x0A => 5,
            b';' => 8,
            b'\'' => 2,
            b'.' => 6,
            b'/' => 4,
            _ => 0,
        };
        if dir != 0 {
            self.press_direction(dir);
        } else {
            self.type_key(k);
        }
    }

    pub(crate) fn n_actors(&self) -> usize {
        (self.g.actor_count as usize).min(MAX_ACTORS)
    }

    /// $4BF0
    fn update_actors(&mut self) {
        if self.g.actor_count == 0 {
            return;
        }
        let mut dir = if self.g.key_dir != 0 { self.g.key_dir } else { self.g.joy_dir };
        if self.vars[30] != 0 {
            dir = 0;
        }
        let e = &mut self.actors[0];
        if self.g.ego_auto == 0 && (e[f::DIR] == 0 || e[f::DIR] == e[f::MOTION]) {
            e[f::MOTION] = dir;
        }
        if e[f::CYCLE] < 2 {
            e[f::CYCLE] = if self.g.ego_auto2 != 0 || dir != 0 { 0 } else { 1 };
        }
        for s in (0..self.n_actors()).rev() {
            let a = &self.actors[s];
            if a[f::ROOM] == self.vars[1] && a[f::COUNTDOWN] == 0 {
                self.update_actor(s);
            }
        }
        // $4C5E: a stopped ego under player control faces no direction.
        let e = &mut self.actors[0];
        if e[f::VIEW] == 0 && self.g.ego_auto == 0 && e[f::MOTION] < 9 && e[f::MOTION] != e[f::DIR] && e[f::CYCLE] == 0
        {
            e[f::DIR] = 0;
        }
        self.countdowns(); // $5D05
    }

    /// $60DD
    fn update_actor(&mut self, s: usize) {
        let m = self.actors[s][f::MOTION];
        if m == 0 {
            self.actors[s][f::DIR] = 0;
        } else {
            if m >= 9 {
                self.motion(s, m);
            } else {
                self.actors[s][f::DIR] = m;
            }
            self.choose_loop(s);
        }
        self.barriers(s);
        self.animate(s);
    }

    /// $6239: pick the loop that matches the direction, keeping the cel where possible.
    fn choose_loop(&mut self, s: usize) {
        let a = self.actors[s];
        if a[f::LOOP_COUNT] == 1 {
            return;
        }
        let d = (a[f::DIR] as usize).min(8);
        let mut l = if a[f::LOOP_COUNT] == 2 { LOOPS_2[d] } else { LOOPS_4[d] };
        if l == 4 {
            l = a[f::LOOP];
        }
        if l == a[f::LOOP] {
            return;
        }
        self.actors[s][f::LOOP] = l;
        let n = self.select_loop_cel(s, false);
        self.actors[s][f::CEL_COUNT] = n;
        if n <= a[f::CEL] {
            self.actors[s][f::CEL] = n.wrapping_sub(1);
            self.select_loop_cel(s, false);
        }
    }

    /// $57F8: invisible barrier lines set by set_x_limits / set_y_limits.
    fn barriers(&mut self, s: usize) {
        let g = self.g.clone();
        let a = &mut self.actors[s];
        if g.barrier_x == 0 && g.barrier_y == 0 {
            if a[f::NO_MOVE] == 2 {
                a[f::NO_MOVE] = 0;
            }
            return;
        }
        if a[f::NO_MOVE] == 1 {
            return;
        }
        let d = a[f::DIR];
        if g.barrier_x != 0 {
            let (b, x) = (g.barrier_x, a[f::X]);
            let r = if b >= x && b.wrapping_sub(g.barrier[0]) <= x {
                if (2..5).contains(&d) {
                    2
                } else {
                    0
                }
            } else if b <= x && b.wrapping_add(g.barrier[1]) >= x {
                if (6..9).contains(&d) {
                    2
                } else {
                    0
                }
            } else {
                0
            };
            a[f::NO_MOVE] = r;
        }
        if g.barrier_y != 0 {
            let (b, y) = (g.barrier_y, a[f::Y]);
            let r = if b >= y && b.wrapping_sub(g.barrier[2]) <= y {
                if (4..7).contains(&d) {
                    2
                } else {
                    0
                }
            } else if b <= y && b.wrapping_add(g.barrier[3]) >= y {
                if d == 8 || d < 3 {
                    2
                } else {
                    0
                }
            } else {
                0
            };
            a[f::NO_MOVE] = r;
        }
    }

    /// Cycle modes, table $6117.
    fn animate(&mut self, s: usize) {
        let a = self.actors[s];
        let (cel, count) = (a[f::CEL], a[f::CEL_COUNT]);
        match a[f::CYCLE] {
            0 => {
                let c = cel.wrapping_add(1);
                self.actors[s][f::CEL] = if c >= count { 0 } else { c };
                self.select_loop_cel(s, false);
            }
            2 => {
                if cel == 0 {
                    self.vars[8] = 1;
                    self.actors[s][f::CYCLE] = 1;
                } else {
                    self.actors[s][f::CEL] = cel - 1;
                    self.select_loop_cel(s, false);
                }
            }
            3 => {
                if cel == count.wrapping_sub(1) {
                    self.vars[8] = 1;
                    self.actors[s][f::CYCLE] = 1;
                    if a[f::SLOT] == 0 {
                        self.g.ego_auto = 0;
                        self.g.ego_auto2 = 0;
                    }
                } else {
                    self.actors[s][f::CEL] = cel.wrapping_add(1);
                    self.select_loop_cel(s, false);
                }
            }
            4 => self.actors[s][f::CYCLE] = 3,
            5 => {
                if cel < count.wrapping_sub(1) {
                    self.actors[s][f::CEL] = cel + 1;
                    self.select_loop_cel(s, true);
                    self.actors[s][f::DIR] = 0;
                } else {
                    self.set_ego_view(0);
                    self.actors[s][f::STEP_X] = self.g.special;
                    self.vars[8] = 1;
                    self.actors[s][f::DIR] = 3;
                    self.actors[s][f::CYCLE] = 7;
                }
            }
            6 => {
                self.actors[s][f::STEP_X] = self.g.special;
                self.actors[s][f::DIR] = 7;
                self.actors[s][f::CYCLE] = 5;
            }
            7 => {
                if a[f::SLOT] == 0 {
                    self.g.ego_auto = 0;
                    self.g.ego_auto2 = 0;
                }
                let x = &mut self.actors[s];
                x[f::CYCLE] = 1;
                x[f::DIR] = 0;
                x[f::FLAGS] = 0;
                self.select_loop0_header(s);
            }
            _ => {}
        }
    }

    /// aux $0A1C with loop 0, cel 0 ($4A00): copy loop 0's header (steps, delay).
    fn select_loop0_header(&mut self, s: usize) {
        let (l, c) = (self.actors[s][f::LOOP], self.actors[s][f::CEL]);
        self.actors[s][f::LOOP] = 0;
        self.actors[s][f::CEL] = 0;
        self.select_loop_cel(s, true);
        self.actors[s][f::LOOP] = l;
        self.actors[s][f::CEL] = c;
    }

    /// $5531: motion modes $0C-$11 (table $6AE4).
    fn motion(&mut self, s: usize, m: u8) {
        self.g.slack = [0, 0];
        let a = self.actors[s];
        self.g.disp = [a[f::X], a[f::Y], a[f::STEP_X], a[f::STEP_Y]];
        self.g.disp_slot = s as u8;
        // $554F-$555D: the handler address is stored into the JSR at $5560.
        self.g.motion_jsr = self.engine_word(0x6AE4 + ((m.wrapping_sub(0x0C) << 1) as u16));
        match m {
            0x0C => self.chase(s),
            0x0D => {
                // $55D6: move down to target y
                let a = self.actors[s];
                if a[f::Y] >= self.g.target_y || (a[f::Y] == self.g.last_pos[1] && a[f::DIR] != 0) {
                    self.arrive(s);
                } else {
                    self.actors[s][f::STEP_Y] = 4;
                    self.actors[s][f::DIR] = 5;
                }
            }
            0x0E => self.move_to_target(s),
            0x0F => {
                self.g.slack[0] = 5;
                self.chase(s);
            }
            0x10 => self.wander(s),
            0x11 => self.follow(s),
            _ => {}
        }
        self.g.last_pos = [self.actors[s][f::X], self.actors[s][f::Y]];
        self.g.disp_slot = 0xFF;
    }

    /// The actor's x, y, step x and step y as the motion code sees them ($57/$58/$55/$56
    /// for the actor being dispatched).
    fn mpos(&self, s: usize) -> [u8; 4] {
        if self.g.disp_slot as usize == s {
            self.g.disp
        } else {
            let a = &self.actors[s];
            [a[f::X], a[f::Y], a[f::STEP_X], a[f::STEP_Y]]
        }
    }

    /// $566F: direction from the actor to (tx, ty) with the given slack.
    fn direction_to(&self, s: usize, tx: u8, ty: u8, slack: [u8; 2]) -> u8 {
        let p = self.mpos(s);
        let axis = |d: u8, sl: u8| -> usize {
            let neg = d & 0x80 != 0;
            let mag = if neg { 0u8.wrapping_sub(d) } else { d };
            if mag <= sl {
                1
            } else if neg {
                2
            } else {
                0
            }
        };
        let sx = axis(p[0].wrapping_sub(tx), slack[0]);
        let sy = axis(p[1].wrapping_sub(ty), slack[1]);
        DIR_GRID[sx * 3 + sy]
    }

    /// $5571
    fn move_to_target(&mut self, s: usize) {
        let p = self.mpos(s);
        let (tx, ty) = (self.g.target_x, self.g.target_y);
        if p[0] == tx && p[1] == ty {
            self.arrive(s);
            return;
        }
        let d = self.direction_to(s, tx, ty, [p[2], p[3]]);
        self.actors[s][f::DIR] = d;
        if d == 0 {
            self.arrive(s);
        }
    }

    /// $43C6: head for the ego (or the move_to target), with detours when stuck.
    fn chase(&mut self, s: usize) {
        let ego = (self.actors[0][f::X], self.actors[0][f::Y]);
        let a = self.actors[s];
        if self.vars[6] != 0 && self.vars[6] == a[f::VIEW] {
            if a[f::MOTION] != 0x11 {
                self.arrive(s);
                return;
            }
            self.vars[6] = 0;
        }
        if a[f::NO_MOVE] == 2 && self.barrier_detour(s, ego) {
            return;
        }
        let p = self.mpos(s);
        if p[0] == self.g.last_pos[0] && p[1] == self.g.last_pos[1] {
            if self.g.wander_timer == 0 {
                // $4413: first entry follows two equal compares (carry set). A retry after
                // a zero duration carries bit 1 of the timer (LSR LSR); after one of 11 or
                // more, the carry from CMP #$0B (set).
                let mut carry = true;
                loop {
                    self.pick_wander_direction(s, carry);
                    let old = self.g.wander_timer;
                    let t = old >> 2;
                    if t == 0 {
                        carry = old & 2 != 0;
                        continue;
                    }
                    if t >= 0x0B {
                        carry = true;
                        continue;
                    }
                    self.g.wander_timer = t;
                    return;
                }
            }
            self.g.wander_timer = 0;
        } else if self.g.wander_timer != 0 {
            self.g.wander_timer -= 1;
            return;
        }
        // $4486
        let (tx, ty) = if a[f::MOTION] == 0x0E { (self.g.target_x, self.g.target_y) } else { ego };
        let slack = [self.g.slack[0].max(p[2]), self.g.slack[1].max(p[3])];
        let d = self.direction_to(s, tx, ty, slack);
        self.actors[s][f::DIR] = d;
        if d == 0 {
            self.arrive(s);
        } else if a[f::MOTION] == 0x11 {
            self.follow_strike(s, ego);
        }
    }

    /// $4432: blocked by a barrier: head along it toward the ego. Returns true if handled.
    fn barrier_detour(&mut self, s: usize, ego: (u8, u8)) -> bool {
        self.actors[s][f::NO_MOVE] = 0;
        let a = self.actors[s];
        let toward_y = |w: &mut World| -> bool {
            if ego.1 == a[f::Y] {
                return false;
            }
            w.actors[s][f::DIR] = if ego.1 < a[f::Y] { 1 } else { 5 };
            true
        };
        let toward_x = |w: &mut World| -> bool {
            if ego.0 == a[f::X] {
                return false;
            }
            w.actors[s][f::DIR] = if ego.0 < a[f::X] { 7 } else { 3 };
            true
        };
        if self.g.barrier_x != 0 {
            if !toward_y(self) {
                toward_x(self);
            }
            true
        } else if self.g.barrier_y != 0 {
            if !toward_x(self) {
                toward_y(self);
            }
            true
        } else {
            false
        }
    }

    /// $5F2B
    fn wander(&mut self, s: usize) {
        // The motion dispatch ($5531: SBC, ASL) leaves the carry clear.
        if self.g.wander_timer == 0 {
            self.pick_wander_direction(s, false);
            return;
        }
        self.g.wander_timer -= 1;
        if self.actors[s][f::DIR] == 0 {
            self.pick_wander_direction(s, false);
            return;
        }
        let (blocked, carry) = self.wander_blocked(s, false);
        if blocked {
            self.pick_wander_direction(s, carry);
        }
    }

    /// $5F4A: pick a random direction that is not blocked, and a duration.
    fn pick_wander_direction(&mut self, s: usize, mut carry: bool) {
        for _ in 0..1000 {
            let (r, c) = self.random_c(carry);
            self.actors[s][f::DIR] = (r & 7) + 1;
            let (blocked, c2) = self.wander_blocked(s, c);
            if !blocked {
                let t = (r & 0xF0) >> 2;
                self.g.wander_timer = if t == 0 { 4 } else { t };
                return;
            }
            carry = c2;
        }
    }

    /// $5F6E: trial move in the current direction. Returns (blocked, carry on exit).
    fn wander_blocked(&mut self, s: usize, carry_in: bool) -> (bool, bool) {
        if self.actors[s][f::DIR] == 0 {
            return (true, carry_in);
        }
        self.vars[21] = 0;
        if self.move_actor(s) {
            return (true, false); // returned from aux through XFER, which clears carry
        }
        if self.vars[21] != 0 && self.vars[21] == self.actors[s][f::VIEW] {
            // CMP equal leaves carry set on both outcomes.
            self.g.edge_toggle ^= 1;
            return (self.g.edge_toggle == 0, true);
        }
        (false, false)
    }

    /// $55AC: follow_ego. Patrols along y = 40 and strikes when the ego is below.
    fn follow(&mut self, s: usize) {
        if self.g.follow_state != 0 {
            self.g.slack[1] = self.g.follow_dist;
            self.chase(s);
            return;
        }
        let p = self.mpos(s);
        if p[0] == self.g.last_pos[0] && p[1] == self.g.last_pos[1] {
            self.vars[21] = 0;
            self.g.follow_state = 1;
            self.follow(s);
            return;
        }
        self.move_to_target(s);
    }

    /// $5D8E: in follow mode, check for contact with the ego, or dive toward it.
    fn follow_strike(&mut self, s: usize, ego: (u8, u8)) {
        let a = self.actors[s];
        let reach = a[f::Y].wrapping_add(a[f::STEP_Y]).wrapping_add(self.g.follow_dist);
        if reach >= ego.1 && self.vars[21] != 0 {
            let t = self.vars[21];
            self.vars[21] = 0;
            if t == a[f::VIEW] {
                self.arrive(s);
                return;
            }
        }
        // $5DB4/$5E10: does the actor's bottom row touch the ego's box?
        self.select_cel_dims(s);
        let (left, right, bottom) = (a[f::X], a[f::X].wrapping_add(self.g.cel_w).wrapping_sub(1), a[f::Y]);
        self.select_cel_dims(0);
        let e = self.actors[0];
        let touching = e[f::Y].wrapping_sub(self.g.cel_h) <= bottom
            && e[f::X].wrapping_add(self.g.cel_w) > left
            && right >= e[f::X];
        if touching {
            self.vars[6] = a[f::VIEW];
            self.arrive(s);
            return;
        }
        let reach = a[f::Y].wrapping_sub(a[f::STEP_Y]).wrapping_add(self.g.follow_dist);
        if reach >= ego.1 {
            self.actors[s][f::DIR] = if a[f::X] >= ego.0 { 8 } else { 2 };
        }
    }

    /// $55F8: the actor reached its goal.
    fn arrive(&mut self, s: usize) {
        self.select_loop0_header(s);
        if self.g.disp_slot as usize == s {
            // $55F8-$55FC: $55/$57 become the loop and cel (0) to select.
            self.g.disp[0] = 0;
            self.g.disp[2] = 0;
        }
        if self.actors[s][f::SLOT] == 0 {
            self.g.ego_auto = 0;
            self.g.ego_auto2 = 0;
        }
        let m = self.actors[s][f::MOTION];
        if m == 0x11 {
            // $5647: alternate between striking and patrolling the top of the screen.
            self.g.follow_state ^= 1;
            if self.g.follow_state != 0 {
                self.follow(s);
                return;
            }
            self.g.target_y = 0x28;
            if self.g.patrol_side != 0 {
                self.g.target_x = 0x96u8.wrapping_sub(self.g.cel_w);
                self.g.patrol_side = 0;
            } else {
                self.g.target_x = 0x0A;
                self.g.patrol_side = 1;
            }
            self.move_to_target(s);
            return;
        }
        if m == 0x0C || m == 0x0F {
            self.vars[6] = self.actors[s][f::VIEW];
        } else {
            self.vars[8] = 1;
        }
        if m == 0x0F {
            self.actors[s][f::FLAGS] = 0;
        }
        self.actors[s][f::DIR] = 0;
        self.actors[s][f::MOTION] = 0;
    }

    /// $5D05: count down each actor's delay; if any reaches zero, run the draw pass.
    fn countdowns(&mut self) {
        let mut due = false;
        for s in 0..self.n_actors() {
            let a = &mut self.actors[s];
            let v = if a[f::COUNTDOWN] == 0 { a[f::DELAY] } else { a[f::COUNTDOWN] };
            let v = v.wrapping_sub(1);
            if v == 0 {
                due = true;
            }
            a[f::COUNTDOWN] = v;
        }
        if due {
            self.draw_pass();
        }
    }

    /// $4DED (move every on-screen actor in draw order, cancelling moves that touch
    /// the ego), then the draw and commit passes ($4E42, $4E69).
    pub fn draw_pass(&mut self) {
        self.restore_all(); // $4DD7
        self.move_all(); // $4DED
        self.save_all(); // $4E2C
        for s in 0..self.n_actors() {
            if self.actors[s][f::ROOM] == self.vars[1] && self.actors[s][f::DRAWN] != 0 {
                self.cost_cel(s);
                self.cost_screen(s);
            }
        }
        self.screen = self.compose_at_new();
        self.commit_all(); // $4E69
    }

    /// $4EA1 + $4EE3: selection sort on the draw keys. Among equal keys the later slot is
    /// taken first; a chosen key of 0 stops the scan early, as in the engine.
    pub fn draw_order(&self) -> Vec<usize> {
        let n = self.n_actors();
        let mut keys: Vec<u8> = (0..n).map(|s| self.sort_key(s)).collect();
        let mut order = Vec::with_capacity(n);
        for _ in 0..n {
            let mut best = 0;
            let mut a = keys[0];
            for (x, &k) in keys.iter().enumerate().skip(1) {
                if a < k {
                    continue;
                }
                best = x;
                a = k;
                if a == 0 {
                    break;
                }
            }
            order.push(best);
            keys[best] = 0xFF;
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::super::{f, Resources, World};

    fn world() -> World {
        World::new(Resources { objects: vec![[0; 8]; 150], engine: vec![0; 0x3200], ..Default::default() })
    }

    #[test]
    fn direction_grid_matches_compass() {
        let mut w = world();
        w.actors[1][f::X] = 50;
        w.actors[1][f::Y] = 100;
        let d = |w: &World, tx, ty| w.direction_to(1, tx, ty, [0, 0]);
        assert_eq!(d(&w, 50, 100), 0);
        assert_eq!(d(&w, 50, 60), 1); // up
        assert_eq!(d(&w, 90, 60), 2); // up-right
        assert_eq!(d(&w, 90, 100), 3); // right
        assert_eq!(d(&w, 90, 140), 4);
        assert_eq!(d(&w, 50, 140), 5); // down
        assert_eq!(d(&w, 10, 140), 6);
        assert_eq!(d(&w, 10, 100), 7); // left
        assert_eq!(d(&w, 10, 60), 8);
        // Within the slack counts as arrived on that axis.
        assert_eq!(w.direction_to(1, 53, 100, [3, 0]), 0);
    }

    #[test]
    fn draw_order_takes_later_slot_on_ties() {
        let mut w = world();
        w.g.actor_count = 3;
        for (s, y) in [(0, 100u8), (1, 80), (2, 100)] {
            w.actors[s][f::Y] = y;
        }
        assert_eq!(w.draw_order(), vec![1, 2, 0]);
    }
}
