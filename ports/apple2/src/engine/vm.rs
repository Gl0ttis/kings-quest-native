//! Script virtual machine: the rule matcher ($62B0) and action interpreter ($58AF), run
//! over the raw script bytes so that byte-level behaviour (scanning, wraparound) matches.

use super::{f, World, MAX_ACTORS};
use crate::script::{Script, OPERANDS};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Global,
    Room,
}

struct Ctx {
    data: Rc<Vec<u8>>,
    bounds: [usize; 5],
}

/// Matcher states, named after the engine labels they reproduce.
enum St {
    Top,  // $62B0
    Skip, // $62D0: scan to FE, then jump by its offset
    Pass, // $62E9: scan to FE, discard its offset
    Cond, // $62EF
    Act,  // $6322
}

impl World {
    fn ctx(&self, which: Which) -> Option<Ctx> {
        let data = match which {
            Which::Global => self.res.scripts.first().cloned().flatten()?,
            Which::Room => self.room_script.clone()?,
        };
        let bounds = Script::new(&data).ok()?.bounds;
        Some(Ctx { data, bounds })
    }

    /// $40BA: one pass of the cycle rules. The global rules repeat while an action
    /// returns non-zero (for example after `new_room`), then the room rules run once.
    pub fn run_cycle_rules(&mut self) {
        for _ in 0..16 {
            if self.run_rules(Which::Global, 1) == 0 || self.g.abort_pass {
                break;
            }
        }
        if !self.g.abort_pass {
            self.run_rules(Which::Room, 1);
        }
    }

    /// $408E: the parser rules for verb/noun ids in `said`. Returns whether any rule matched.
    pub fn run_parser_rules(&mut self, verb: u8, noun: u8) -> bool {
        self.g.said = [verb, noun];
        self.g.said_matched = 0;
        self.run_rules(Which::Global, 0);
        if !self.g.abort_pass {
            self.run_rules(Which::Room, 0);
        }
        self.g.said_matched != 0
    }

    /// $62B0. `mode` 1 = cycle rules (section 1), 0 = parser rules (section 2).
    pub fn run_rules(&mut self, which: Which, mode: u8) -> u8 {
        let Some(c) = self.ctx(which) else { return 0 };
        let d = c.data.clone();
        let mut pc = c.bounds[if mode == 1 { 1 } else { 2 }];
        let mut st = St::Top;
        macro_rules! fetch {
            () => {{
                match d.get(pc) {
                    Some(&b) => {
                        self.cost.rule_bytes += 1;
                        pc += 1;
                        b
                    }
                    None => {
                        self.note(format!("{which:?} rules: read past end of script"));
                        return 0;
                    }
                }
            }};
        }
        // $63C2: step back one byte, scan forward to FE, return the following u16.
        macro_rules! scan {
            () => {{
                pc -= 1;
                loop {
                    if fetch!() == 0xFE {
                        break;
                    }
                }
                let lo = fetch!() as usize;
                let hi = fetch!() as usize;
                lo | hi << 8
            }};
        }
        // $638D: (var, value) pairs until FE; bit 7 of value = not-equal.
        macro_rules! conds {
            () => {{
                let mut ok = true;
                loop {
                    let v = fetch!();
                    if v == 0xFE {
                        break;
                    }
                    let want = fetch!();
                    let cur = self.vars[v as usize];
                    let pass = if want & 0x80 == 0 { cur == want } else { cur != want & 0x7F };
                    if !pass {
                        ok = false;
                        break;
                    }
                }
                ok
            }};
        }
        // $6363: ego (actor table slot 0) inside x1 y1 x2 y2; stops reading at the first failure.
        macro_rules! rect {
            ($x1:expr) => {{
                let (ex, ey) = (self.actors[0][f::X], self.actors[0][f::Y]);
                let x1: u8 = $x1;
                if x1 > ex {
                    false
                } else if fetch!() > ey {
                    false
                } else if fetch!() < ex {
                    false
                } else {
                    fetch!() >= ey
                }
            }};
        }
        for _ in 0..200_000 {
            st = match st {
                St::Top => {
                    let b = fetch!();
                    match b {
                        0xFF => return 0,
                        0xFC => St::Cond,
                        0xFE => St::Skip,
                        0xFB if mode == 0 => St::Cond,
                        0xFB => St::Act,
                        _ if mode != 0 => {
                            if rect!(b) {
                                St::Act
                            } else {
                                St::Pass
                            }
                        }
                        0xFD => {
                            let x1 = fetch!();
                            if rect!(x1) {
                                St::Cond
                            } else {
                                St::Skip
                            }
                        }
                        _ => {
                            if b != self.g.said[0] {
                                St::Pass
                            } else {
                                let n = fetch!();
                                if n == self.g.said[1] || n == 1 {
                                    St::Act
                                } else {
                                    St::Pass
                                }
                            }
                        }
                    }
                }
                St::Skip => {
                    let off = scan!();
                    pc = (pc + off) & 0xFFFF;
                    St::Top
                }
                St::Pass => {
                    let _ = scan!();
                    St::Top
                }
                St::Cond => {
                    if conds!() {
                        St::Pass
                    } else {
                        St::Skip
                    }
                }
                St::Act => {
                    if !conds!() {
                        St::Pass
                    } else {
                        if mode != 1 {
                            self.g.said_matched = 1;
                        }
                        let lo = fetch!() as usize;
                        let hi = fetch!() as usize;
                        let r = self.run_action(which, lo | hi << 8);
                        if r != 0 {
                            return r;
                        }
                        if d.get(pc) == Some(&0xFE) {
                            St::Skip
                        } else {
                            St::Top
                        }
                    }
                }
            };
        }
        self.note(format!("{which:?} rules: no end after 200000 steps"));
        0
    }

    /// $58AF: run the action at `off` in section 0. Returns the stop value ($08).
    pub fn run_action(&mut self, which: Which, off: usize) -> u8 {
        let Some(c) = self.ctx(which) else { return 0 };
        let mut pc = c.bounds[0] + off;
        loop {
            let Some(&op) = c.data.get(pc) else {
                self.note(format!("{which:?} action {off:04X}: ran off script"));
                return 1;
            };
            pc += 1;
            if op == 0xFF {
                return 0;
            }
            if op >= 0x44 {
                self.note(format!("{which:?} action {off:04X}: bad opcode ${op:02X} (engine would stop)"));
                return 1;
            }
            let n = OPERANDS[op as usize] as usize;
            let Some(args) = c.data.get(pc..pc + n) else {
                self.note(format!("{which:?} action {off:04X}: operands run off script"));
                return 1;
            };
            let args = args.to_vec();
            pc += n;
            let r = self.exec(which, &c, op, &args);
            if r != 0 {
                return r;
            }
        }
    }

    fn message(&self, c: &Ctx, lo: u8, hi: u8) -> String {
        let off = lo as usize | (hi as usize) << 8;
        Script::new(&c.data)
            .ok()
            .and_then(|s| s.message_at(off))
            .unwrap_or_else(|| format!("<message offset {off:04X}>"))
    }

    fn clamp_x(&self, x: u8) -> u8 {
        // $5CD7
        let w = self.g.cel_w;
        if x < 0x0B {
            0x0B
        } else if x as u16 + w as u16 >= 0x96 {
            0x96u8.wrapping_sub(w)
        } else {
            x
        }
    }

    fn clamp_y(&self, y: u8) -> u8 {
        // $5CEE
        let h = self.g.cel_h;
        if y >= 0xA1 {
            0xA1
        } else if y.wrapping_sub(h) < 3 {
            3u8.wrapping_add(h)
        } else {
            y
        }
    }

    /// One action opcode. Returns the handler's stop value ($08).
    fn exec(&mut self, _which: Which, c: &Ctx, op: u8, a: &[u8]) -> u8 {
        let slot = |w: &World, o: u8| w.slot_of(o);
        // $58C7-$58D3: the handler address is stored into the JSR at $58D6.
        if op < 0x44 {
            self.g.action_jsr = self.engine_word(0x6AF3 + (op as u16) * 2);
        }
        match op {
            0x00 | 0x29 => {}
            0x01 => {
                // $46C8 reset_ego
                if let Some(s) = self.bind_view(0) {
                    let (l, cel) = (self.actors[s][f::LOOP], self.actors[s][f::NO_MOVE]);
                    self.set_loop_cel(s, l, cel);
                    self.actors[s][f::ROOM] = self.vars[1];
                    self.actors[s][f::MOTION] = self.g.saved_motion;
                }
                self.vars[26] = 0;
            }
            0x02 => {
                // $46FB add_actor
                if let Some(s) = self.bind_view(a[0]) {
                    self.set_loop_cel(s, 0, 0);
                }
            }
            0x03 => self.clear_actors(),
            0x04 => {
                self.new_room(a[0]);
                return 1;
            }
            0x05 => {
                // $4208 redraw_room: load picture v1 to $D800, reset the ego
                self.pic_d800 = Some(self.vars[1]);
                self.cost_load(0, self.vars[1]);
                return self.exec(_which, c, 0x01, &[]);
            }
            0x06 => {
                // $5FA4: clear, draw the $D800 picture
                self.base = self.picture(self.pic_d800, None);
                // The cleared screen and new picture cover any text drawn over the old one.
                self.text.written = [[false; super::text::COLS]; super::text::ROWS];
                // The clear (aux $10) blanks the text rows too; then $6EF0 asks for the
                // prompt and typed line to be reprinted, from row 20 column 0 ($57EF).
                for r in 20..super::text::ROWS {
                    self.text.hires[r] = [b' '; super::text::COLS];
                }
                self.text.need_reprint = true;
                self.text.cv = 20;
                self.text.ch = 0;
            }
            0x07 => self.draw_object(a[0]),
            0x08 => self.vars[a[0] as usize] = 1,
            0x09 => self.vars[a[0] as usize] = 0,
            0x0A => {
                let m = self.message(c, a[0], a[1]);
                self.note(format!("print: {}", m.replace('\r', " / ")));
                self.print_message(m.as_bytes());
            }
            0x3E | 0x3D => {
                // $575C / $573D: print raw (optionally at row/column), then a new line.
                let (lo, hi) = if op == 0x3D { (a[2], a[3]) } else { (a[0], a[1]) };
                if op == 0x3D {
                    self.text.cv = a[0];
                    self.text.ch = a[1];
                }
                let m = self.message(c, lo, hi);
                self.note(format!("print_text: {}", m.replace('\r', " / ")));
                self.put_str(m.as_bytes());
                self.put_char(0x0D);
            }
            0x0B => self.objects[a[0] as usize][2] |= 0x80,
            0x0D => self.objects[a[0] as usize][2] &= 0x7F,
            0x0C => {
                // $6511 -> $5479: erase from screen (restoring its background), move and
                // redraw the others; then clear the stored room, keeping the carried bit.
                let s = slot(self, a[0]);
                if self.actors[s][f::DRAWN] != 0 {
                    self.actors[s][f::ROOM] = self.vars[1];
                    self.restore_all();
                    self.move_all();
                    self.save_all();
                    self.actors[s][f::ROOM] = 0xFF;
                    // $4E42 decodes the others' cels; $4E69 updates every rectangle,
                    // including the erased actor's.
                    for t in 0..self.n_actors() {
                        if self.actors[t][f::ROOM] == self.vars[1] && self.actors[t][f::DRAWN] != 0 {
                            self.cost_cel(t);
                        }
                    }
                    self.screen = self.compose_at_new();
                    self.actors[s][f::ROOM] = self.vars[1];
                    for t in 0..self.n_actors() {
                        if self.participates(t) {
                            self.cost_screen(t);
                        }
                    }
                    self.commit_all();
                    self.actors[s][f::ROOM] = 0xFF;
                    self.actors[s][f::DRAWN] = 0;
                }
                self.objects[a[0] as usize][2] &= 0x80;
            }
            0x0E => {
                // $652C copy_position, then draw the target
                let (sa, sb) = (slot(self, a[0]), slot(self, a[1]));
                let (x, y) = (self.actors[sa][f::X], self.actors[sa][f::Y]);
                self.actors[sb][f::X] = x;
                self.actors[sb][f::Y] = y;
                let v = self.actors[sb][f::VIEW];
                self.draw_object(v);
            }
            0x10 => {
                let v = &mut self.vars[a[0] as usize];
                if *v < 0xFA {
                    *v += 1;
                }
            }
            0x11 => {
                let v = &mut self.vars[a[0] as usize];
                if *v != 0 {
                    *v -= 1;
                }
            }
            0x12 => self.vars[a[0] as usize] = a[1],
            0x13 => {
                let s = slot(self, a[0]);
                self.actors[s][f::CEL] = a[1];
                self.select_loop_cel(s, true);
                self.actors[s][f::CYCLE] = 1;
            }
            0x14 => self.vars[19] = self.vars[19].wrapping_add(a[0]),
            0x15 => self.vars[19] = self.vars[19].wrapping_sub(a[0]),
            0x16 => {
                // $5A47: intends to clear v8 but indexes with Y (= 0), so clears v0.
                self.vars[0] = 0;
                let s = slot(self, a[0]);
                self.actors[s][f::CYCLE] = 4;
                if a[0] == 0 {
                    self.g.ego_auto = 1;
                }
            }
            0x17 => {
                self.vars[8] = 0;
                let s = slot(self, a[0]);
                self.actors[s][f::CEL] = self.actors[s][f::CEL_COUNT];
                self.actors[s][f::CYCLE] = 2;
            }
            0x18 => {
                if a[0] == 0 {
                    self.g.ego_auto = 1;
                    self.g.ego_auto2 = 1;
                }
                let s = slot(self, a[0]);
                self.actors[s][f::MOTION] = 0x0E;
                self.vars[8] = 0;
                self.g.target_x = self.clamp_x(a[1]);
                self.g.target_y = self.clamp_y(a[2]);
            }
            0x19 => {
                // $5A18 position
                let s = slot(self, a[0]);
                self.select_cel_dims(s);
                let x = self.clamp_x(a[1]);
                self.actors[s][f::X] = x;
                self.actors[s][f::NEW_X] = x;
                self.objects[a[0] as usize][5] = x;
                let y = self.clamp_y(a[2]);
                self.actors[s][f::Y] = y;
                self.actors[s][f::NEW_Y] = y;
                self.objects[a[0] as usize][6] = y;
            }
            0x1A => self.actors[slot(self, a[0])][f::NO_MOVE] = 1,
            0x1B => self.actors[slot(self, a[0])][f::NO_MOVE] = 0,
            0x1C | 0x41 => self.set_ego_view(a[0]),
            0x1D | 0x1E => {
                self.vars[8] = 0;
                let s = slot(self, a[0]);
                self.actors[s][f::MOTION] = if op == 0x1D { 0x0F } else { 0x11 };
                self.actors[s][f::FLAGS] |= 1;
                self.actors[s][f::PRIORITY] = 0xF0;
                if op == 0x1E {
                    self.g.follow_dist = a[1];
                    self.g.follow_state = 1;
                }
            }
            0x1F => {
                let s = slot(self, a[0]);
                self.actors[s][f::MOTION] = 0x0C;
                self.actors[s][f::CYCLE] = 0;
                self.g.wander_timer = 0;
            }
            0x20 => {
                self.g.barrier_x = if a[0] != 0 { self.clamp_x(a[0]) } else { 0 };
                self.g.barrier[0] = a[1];
                self.g.barrier[1] = a[2];
            }
            0x21 => {
                self.g.barrier_y = if a[0] != 0 { self.clamp_y(a[0]) } else { 0 };
                self.g.barrier[2] = a[1];
                self.g.barrier[3] = a[2];
            }
            0x22 => {
                let s = slot(self, a[0]);
                self.actors[s][f::MOTION] = 0x10;
                self.g.wander_timer = 0;
            }
            0x23 => {
                self.vars[8] = 0;
                self.actors[0][f::MOTION] = 0x0D;
                self.actors[0][f::STEP_Y] = 4;
                self.g.target_x = self.actors[0][f::X];
                self.g.target_y = self.clamp_y(a[0]);
                self.g.ego_auto = 1;
                self.g.ego_auto2 = 1;
            }
            0x24 => {
                self.loaded_views[a[0] as usize] = true;
                self.cost_load(1, a[0]);
            }
            0x25 => {
                // $5FBF: draw the $D000 picture over the current one
                let cur = std::mem::take(&mut self.base);
                self.base = self.picture(self.pic_d000, Some(cur));
                // The original draws it into the buffer on display, over the sprites, so
                // it shows at once rather than at the next sprite redraw.
                let data = self.pic_d000.and_then(|n| self.res.pictures.get(n as usize).cloned().flatten());
                if let Some(data) = data.filter(|_| self.screen.len() == self.base.len()) {
                    self.screen = crate::pic::draw_onto(std::mem::take(&mut self.screen), &data).buf;
                }
            }
            0x26 => {
                // $63FC place_object: set loop/cel, draw (nudging), add to the picture
                let s = slot(self, a[0]);
                let o = a[0] as usize;
                let room = (self.objects[o][2] & 0x80) | self.vars[1];
                self.objects[o][2] = room;
                self.actors[s][f::ROOM] = room;
                self.objects[o][5] = self.actors[s][f::X];
                self.objects[o][6] = self.actors[s][f::Y];
                self.actors[s][f::LOOP] = a[1];
                self.actors[s][f::CEL] = a[2];
                let n = self.select_loop_cel(s, false);
                self.actors[s][f::CEL_COUNT] = n;
                self.actors[s][f::DRAWN] = 0;
                self.draw_object(a[0]);
                let s = slot(self, a[0]);
                self.add_to_picture(s);
            }
            0x27 => {
                let s = slot(self, a[0]);
                self.actors[s][f::ROOM] = self.vars[1];
                self.actors[s][f::LOOP] = a[1];
                self.actors[s][f::CEL] = a[2];
                let n = self.select_loop_cel(s, true);
                self.actors[s][f::CEL_COUNT] = n;
            }
            0x28 => {
                self.g.target_x = self.clamp_x(a[0]);
                self.g.target_y = self.actors[0][f::Y];
                self.actors[0][f::STEP_X] = 4;
                self.actors[0][f::MOTION] = 0x0E;
                self.g.ego_auto = 1;
                self.g.ego_auto2 = 1;
            }
            0x2A => {
                self.actors[0][f::CYCLE] = 0;
                self.g.ego_auto = 1;
                self.g.ego_auto2 = 1;
            }
            0x2B => {
                // Disable typed input; only Esc, 5 (restart), 3 (restore) and Tab still work.
                self.g.flag_6f05 = 1;
                return 1;
            }
            0x2C => {
                self.pic_d000 = Some(a[0]);
                self.cost_load(0, a[0]);
            }
            0x2D => {
                // $41BD ego_special_view
                self.vars[8] = 0;
                self.g.ego_auto = 1;
                self.g.ego_auto2 = 1;
                self.vars[26] = 1;
                self.actors[0][f::CYCLE] = 5;
                self.actors[0][f::VIEW] = a[0];
                self.actor_view[0] = Some(a[0]);
                self.g.special = a[1];
                self.g.saved_loop = self.actors[0][f::LOOP];
                self.g.saved_cel = 0;
                let l = self.actors[0][f::LOOP];
                self.set_loop_cel(0, l, 0);
            }
            0x2E => {
                self.g.ego_auto = 0;
                self.g.ego_auto2 = 0;
            }
            0x2F => {
                let s = slot(self, a[0]);
                self.actors[s][f::FLAGS] |= 1;
                self.actors[s][f::PRIORITY] = a[1] << 4;
            }
            0x30 => {
                let s = slot(self, a[0]);
                if self.loaded_views[a[1] as usize] {
                    self.actor_view[s] = Some(a[1]);
                }
                self.set_loop_cel(s, 0, 0);
            }
            0x34 => self.actors[slot(self, a[0])][f::FLAGS] &= !1,
            0x35 => self.vars[a[0] as usize] = self.vars[1],
            0x36 => {
                let eq = self.vars[a[0] as usize] == self.vars[1];
                self.vars[a[1] as usize] = eq as u8;
            }
            0x37 => {
                let r = self.vars[a[0] as usize];
                self.new_room(r);
                return 1;
            }
            0x38 => {
                let s = slot(self, a[0]);
                self.set_loop_cel(s, a[1], 0);
            }
            0x39 => {
                let s = slot(self, a[0]);
                self.add_to_picture(s);
                self.objects[a[0] as usize][5] = self.actors[s][f::X];
                self.objects[a[0] as usize][6] = self.actors[s][f::Y];
            }
            0x3A => self.actors[slot(self, a[0])][f::FLAGS] |= 6,
            0x3B => self.actors[slot(self, a[0])][f::FLAGS] &= !6,
            0x40 => self.actors[slot(self, a[0])][f::FLAGS] |= 8,
            0x33 => {
                self.g.restore_request = 1;
                return 1;
            }
            0x31 => self.inventory_screen(),
            0x3C => self.enter_text_mode(),
            0x3F => self.leave_text_mode(),
            0x0F => {
                // $507A: read a decimal number, echoing it, until RETURN.
                let mut n: u8 = 0;
                loop {
                    let k = self.io_wait_key();
                    self.put_char(k);
                    if k == 0x0D {
                        break;
                    }
                    n = n.wrapping_mul(10).wrapping_add(k.wrapping_sub(b'0'));
                }
                self.vars[a[0] as usize] = n;
            }
            0x43 => {
                // $4A44: "Do you want to start over?" Y restarts, N continues.
                self.put_str(b"Do you want to start over? (Y or N)");
                loop {
                    match self.io_wait_key() | 0x20 {
                        b'n' => break,
                        b'y' => {
                            self.restart();
                            return 1;
                        }
                        _ => {}
                    }
                }
            }
            0x32 => self.save_game(),
            0x42 => {
                // Formatting a save disk has no counterpart when saves are files.
                self.note("init_save_disk: not needed with file saves");
            }
            _ => self.note(format!("unimplemented opcode ${op:02X}")),
        }
        0
    }

    /// $40F2 set_ego_view
    pub(crate) fn set_ego_view(&mut self, v: u8) {
        self.vars[26] = v;
        if !self.loaded_views[v as usize] {
            return;
        }
        self.actor_view[0] = Some(v);
        self.actors[0][f::DIR] = 0;
        let old = self.actors[0][f::VIEW];
        self.actors[0][f::VIEW] = v;
        let (l, c) = if old == 0 {
            // $412B-$4135: the loop ([$03]), and for the cel the next byte, [$04] (the
            // no-move flag, normally 0) rather than [$08]; so coming back to view 0
            // restores the loop at cel 0, or at that flag's value.
            self.g.saved_loop = self.actors[0][f::LOOP];
            self.g.saved_cel = self.actors[0][f::NO_MOVE];
            (0, 0)
        } else if v == 0 {
            (self.g.saved_loop, self.g.saved_cel)
        } else {
            (0, 0)
        };
        self.set_loop_cel(0, l, c);
        if self.actors[0][f::FLAGS] & 1 == 0 && self.actors[0][f::DRAWN] != 0 {
            self.refit_ego();
        }
    }

    /// $4165: after the ego's view changes on screen, make sure the new cel fits: the move
    /// check must pass and the water state must agree with the swimming flag (v4 on water,
    /// v24 swimming). Otherwise erase the ego and nudge it in a spiral until it fits, then
    /// draw it again.
    fn refit_ego(&mut self) {
        let mut moved = false;
        self.select_cel_dims(0);
        self.g.nudge_limit = 0x94u8.wrapping_sub(self.g.cel_w);
        self.g.nudge = [1, 1, 1];
        for _ in 0..20000 {
            let blocked = self.move_actor(0);
            let fits = !blocked && ((self.vars[24] != 0) == (self.vars[4] != 0));
            if fits {
                break;
            }
            if !moved {
                // $5479: take the ego off the screen.
                self.actors[0][f::ROOM] = 0xFF;
                self.actors[0][f::DRAWN] = 0;
                moved = true;
            }
            self.nudge(0);
        }
        if moved {
            self.try_draw(0);
        }
        self.vars[3] = 0;
    }

    /// Draw a picture resource (by number) onto `onto`, or onto a cleared buffer.
    fn picture(&mut self, n: Option<u8>, onto: Option<Vec<u8>>) -> Vec<u8> {
        let blank = || vec![0x1Fu8; crate::pic::WIDTH * crate::pic::HEIGHT];
        let Some(n) = n else {
            self.note("draw picture: none loaded");
            return onto.unwrap_or_else(blank);
        };
        match self.res.pictures.get(n as usize).cloned().flatten() {
            Some(data) => {
                let onto = onto.unwrap_or_else(blank);
                let before = self.cost.clone();
                let p = crate::pic::draw_onto(onto.clone(), &data);
                self.cost.plots += p.plots as u64;
                self.cost.fill_plots += p.fill_plots as u64;
                self.pic_draws.push(super::cost::PicDraw {
                    before,
                    onto,
                    data,
                    plots: p.plots,
                    runs: p.runs,
                    timing: None,
                });
                p.buf
            }
            None => {
                self.note(format!("draw picture {n}: missing"));
                onto.unwrap_or_else(blank)
            }
        }
    }

    /// Enter a room the way a room change does, then run cycles (rules only, no motion)
    /// until the room's set-up has finished (v25 set) or `max_cycles` pass.
    pub fn enter_room(&mut self, r: u8, max_cycles: usize) -> usize {
        // Start as the game does: v1 = 0, and the global script's first cycle rule loads
        // the title picture and calls new_room 1. Then, for other rooms, leave room 1 the
        // way scripts do (clear_actors, reset v25) and enter room r.
        self.run_cycle_rules();
        if r != self.vars[1] {
            self.clear_actors();
            self.vars[25] = 0;
            self.new_room(r);
        }
        for n in 0..max_cycles {
            self.g.cycle = self.g.cycle.wrapping_add(1);
            self.run_cycle_rules();
            if self.vars[25] != 0 && n > 0 {
                return n + 1;
            }
        }
        max_cycles
    }

    /// Debug summary of the active actors.
    pub fn actor_summary(&self) -> String {
        (0..(self.g.actor_count as usize).min(MAX_ACTORS))
            .map(|s| {
                let a = &self.actors[s];
                format!(
                    "[{s}: view {} loop {} cel {} at {},{} room {} drawn {} prio {:X}]",
                    a[f::VIEW],
                    a[f::LOOP],
                    a[f::CEL],
                    a[f::X],
                    a[f::Y],
                    a[f::ROOM],
                    a[f::DRAWN],
                    a[f::PRIORITY] >> 4
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
