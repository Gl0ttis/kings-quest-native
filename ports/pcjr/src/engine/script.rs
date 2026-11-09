//! The scripts: the rule matcher (`$47E1`/`$47EE`, hand-written assembly), the action
//! interpreter (`$1AAC`) and the actions (`$01`-`$43`, through the table at CS:23F3).
//!
//! A script has four sections: actions, cycle rules, parser rules, messages. The
//! matcher runs on the raw bytes, as the original does, because rules that fail are
//! skipped by scanning for their `FE`:
//! - `FF`: the end;
//! - `FC conds FE off` (and `FB conds FE off` among the parser rules): if the
//!   conditions fail, skip `off` bytes;
//! - `FE off`: skip `off` bytes;
//! - `FD x1 y1 x2 y2 conds FE off` (parser rules): the same, with the ego in the box;
//! - cycle rules: `FB conds FE act`, or `x1 y1 x2 y2 conds FE act` (the ego in the box):
//!   run the actions at `act`;
//! - parser rules: `verb noun conds FE act` (`FA` matches any word).
//!
//! Conditions are `var value` pairs up to `FE`; a value with bit 7 set tests "not
//! equal".

use super::res::Script;
use super::World;
use std::rc::Rc;

/// What happens to a rule.
enum Then {
    /// Run its actions.
    Act,
    /// Skip to after its action word (scanning from one byte back for its FE).
    Fail,
    /// Take its offset.
    Jump,
    /// Step over its offset (into the block).
    Skip,
}

impl World {
    /// `$47EE`: `s`'s cycle rules. The result of the action that stopped them, or 0.
    pub fn match_cycle_rules(&mut self, s: &Rc<Script>) -> u16 {
        self.checkpoint(0x47EE);
        self.match_rules(s, s.cycle, false)
    }

    /// `$47E1`: `s`'s parser rules, against the verb and noun being tried.
    pub fn match_parser_rules(&mut self, s: &Rc<Script>) -> u16 {
        self.checkpoint(0x47E1);
        self.match_rules(s, s.parser, true)
    }

    /// `$47F8`.
    fn match_rules(&mut self, s: &Rc<Script>, mut si: usize, parser: bool) -> u16 {
        loop {
            self.checkpoint(0x47F8);
            // The ego's position, read again for every rule.
            let (ex, ey) = (self.actors[0].x, self.actors[0].y);
            let al = s.at(si);
            si += 1;
            let then = match al {
                0xFF => return 0,
                0xFC => self.block(s, &mut si),
                0xFE => Then::Jump,
                0xFB if parser => self.block(s, &mut si),
                0xFB => self.test(s, &mut si),
                _ if !parser => {
                    // x1 y1 x2 y2: the first byte is x1, even an FD.
                    if self.ego_in(s, &mut si, al, ex, ey) {
                        self.test(s, &mut si)
                    } else {
                        Then::Fail
                    }
                }
                0xFD => {
                    let x1 = s.at(si);
                    si += 1;
                    if self.ego_in(s, &mut si, x1, ex, ey) {
                        self.block(s, &mut si)
                    } else {
                        Then::Jump
                    }
                }
                verb => {
                    let p = &self.g.parse;
                    if verb != p.verb && verb != 0xFA {
                        Then::Fail
                    } else {
                        let noun = s.at(si);
                        si += 1;
                        if noun != self.g.parse.noun && noun != 0xFA {
                            Then::Fail
                        } else {
                            self.test(s, &mut si)
                        }
                    }
                }
            };
            match then {
                Then::Skip => si += 2,
                Then::Jump => si = Self::jump(s, si),
                Then::Fail => si = Self::scan_fe(s, si - 1) + 2,
                Then::Act => {
                    // $4864
                    if parser {
                        self.g.matched = true;
                        self.g.parse.acted = true;
                    }
                    let act = s.word(si);
                    si += 2;
                    let r = self.run_action(s, act);
                    if r != 0 {
                        return r;
                    }
                    if s.at(si) == 0xFE {
                        si = Self::jump(s, si);
                    }
                }
            }
        }
    }

    /// `$480E`: a block: in if the conditions hold, over it if not.
    fn block(&mut self, s: &Script, si: &mut usize) -> Then {
        match self.conditions(s, si) {
            true => Then::Skip,
            false => Then::Jump,
        }
    }

    fn test(&mut self, s: &Script, si: &mut usize) -> Then {
        match self.conditions(s, si) {
            true => Then::Act,
            false => Then::Fail,
        }
    }

    /// `$4893`: from one byte back, scan to the FE; add the offset after it.
    fn jump(s: &Script, si: usize) -> usize {
        let at = Self::scan_fe(s, si.wrapping_sub(1));
        (at + 2).wrapping_add(s.word(at) as usize) & 0xFFFF
    }

    /// `repne scasb` for FE: the position after it.
    fn scan_fe(s: &Script, mut si: usize) -> usize {
        loop {
            let b = s.at(si);
            si += 1;
            if b == 0xFE || si > s.data.len() + 0x10000 {
                return si;
            }
        }
    }

    /// `$48B7`: the ego at (`ex`, `ey`) in x1..=x2, y1..=y2 (x1 given)? A failed test
    /// leaves `si` where it stopped.
    fn ego_in(&mut self, s: &Script, si: &mut usize, x1: u8, ex: u8, ey: u8) -> bool {
        self.cost.conditions += 1;
        if ex < x1 {
            return false;
        }
        let y1 = s.at(*si);
        *si += 1;
        if ey < y1 {
            return false;
        }
        let x2 = s.at(*si);
        *si += 1;
        if ex > x2 {
            return false;
        }
        let y2 = s.at(*si);
        *si += 1;
        ey <= y2
    }

    /// `$48CD`: `var value` pairs up to FE. A failed test stops there, leaving `si`
    /// after it. A failed "not equal" on v255 counts as passing, and also stops there
    /// (the failure path's `inc bl` wraps to 0).
    fn conditions(&mut self, s: &Script, si: &mut usize) -> bool {
        loop {
            self.cost.conditions += 1;
            let var = s.at(*si);
            *si += 1;
            if var == 0xFE {
                return true;
            }
            let val = s.at(*si);
            *si += 1;
            let v = self.vars[var as usize];
            if val & 0x80 == 0 {
                if v != val {
                    return false;
                }
            } else if v == val - 0x80 {
                return var == 0xFF;
            }
        }
    }

    /// `$1AAC`: run `s`'s actions at `off` until `FF`. 0, or 1 from an action that
    /// stops the rules (a new room, op 2B, 40, 43).
    pub fn run_action(&mut self, s: &Rc<Script>, off: u16) -> u16 {
        self.checkpoint(0x1AAC);
        let mut p = s.actions + off as usize;
        loop {
            self.checkpoint(0x1ABB);
            let op = s.at(p);
            if op == 0xFF {
                return 0;
            }
            p += 1;
            if op == 0 || op > 0x43 {
                continue;
            }
            if self.trace_ops {
                let args: Vec<String> = (0..4).map(|i| format!("{:02X}", s.at(p + i))).collect();
                eprintln!("pass {} op {op:02X} {}", self.passes, args.join(" "));
            }
            match self.action(s, op, p) {
                Ok(n) => p = n,
                Err(r) => return r,
            }
        }
    }

    /// `$2496`: the actor showing object `o`.
    fn sel(&mut self, o: u8) -> usize {
        let slot = self.objects[o as usize].slot as usize;
        if slot >= super::ACTORS {
            self.stop(format!("object {o} names actor slot {slot}"));
        }
        self.g.selected = slot;
        slot
    }

    /// Message `word` of `s`'s messages, queued.
    fn queue_script_message(&mut self, s: &Script, word: u16) {
        let at = s.messages + word as usize;
        self.queue_message(s.data.clone(), at);
    }

    /// An engine string (DS offset) queued.
    pub fn queue_engine_string(&mut self, at: usize) {
        let ds = self.res.ds.clone();
        self.queue_message(ds, at);
    }

    /// The action `op` with its operands at `p`: the position after them, or Err to
    /// stop.
    fn action(&mut self, s: &Rc<Script>, op: u8, p: usize) -> Result<usize, u16> {
        let a0 = s.at(p);
        let a1 = s.at(p + 1);
        let a2 = s.at(p + 2);
        if matches!(op, 0x08 | 0x09 | 0x10 | 0x11 | 0x12 | 0x35 | 0x36) {
            self.site(crate::engine::clock::Site::Op(op));
        }
        Ok(match op {
            0x01 => {
                self.reset_ego();
                p
            }
            0x02 => {
                self.add_actor(a0);
                p + 1
            }
            0x03 => {
                self.clear_actors();
                p
            }
            0x04 => {
                self.new_room(a0);
                return Err(1);
            }
            0x05 => {
                // The picture only: the PCjr's op 05 doesn't reset the ego.
                self.wait_print_done();
                self.load_picture(a0);
                p + 1
            }
            0x06 => {
                self.draw_room_picture();
                p
            }
            0x07 => {
                let slot = self.objects[a0 as usize].slot;
                self.nudge_actor(slot);
                let o = &mut self.objects[a0 as usize];
                o.room = if o.room >= 0x80 { self.vars[1].wrapping_add(0x80) } else { self.vars[1] };
                p + 1
            }
            0x08 => {
                self.vars[a0 as usize] = 1;
                p + 1
            }
            0x09 => {
                self.vars[a0 as usize] = 0;
                p + 1
            }
            0x0A => {
                // The message is queued for the timer to print, after a record that
                // puts the cursor at the start of row 24.
                self.g.prompt_shown = true;
                self.queue_engine_string(0x17DA);
                self.queue_script_message(s, s.word(p));
                p + 2
            }
            0x0B => {
                let o = &mut self.objects[a0 as usize];
                if o.room < 0x80 {
                    o.room = o.room.wrapping_add(0x80);
                }
                p + 1
            }
            0x0C => {
                let a = self.sel(a0);
                self.g.current = a;
                self.erase_actor();
                let o = &mut self.objects[a0 as usize];
                o.room = if o.room >= 0x80 { 0x80 } else { 0 };
                p + 1
            }
            0x0D => {
                let o = &mut self.objects[a0 as usize];
                if o.room >= 0x80 {
                    o.room -= 0x80;
                }
                p + 1
            }
            0x0E => {
                // $454D: b to a's position, then placed.
                let (sa, sb) = (self.objects[a0 as usize].slot as usize, self.objects[a1 as usize].slot);
                let (x, y) = (self.actors[sa].x, self.actors[sa].y);
                let b = &mut self.actors[sb as usize];
                b.x = x;
                b.y = y;
                self.nudge_actor(sb);
                p + 2
            }
            0x0F => {
                self.play_sound(a0);
                p + 1
            }
            0x10 => {
                let v = &mut self.vars[a0 as usize];
                if *v < 0xFA {
                    *v += 1;
                }
                p + 1
            }
            0x11 => {
                let v = &mut self.vars[a0 as usize];
                if *v > 0 {
                    *v -= 1;
                }
                p + 1
            }
            0x12 => {
                self.vars[a0 as usize] = a1;
                p + 2
            }
            0x13 => {
                let a = self.sel(a0);
                let lp = self.actors[a].lp;
                self.actors[a].cel = a1;
                self.select_loop_cel(a, lp, a1);
                self.actors[a].cycle = 1;
                p + 2
            }
            0x14 => {
                self.vars[19] = self.vars[19].wrapping_add(a0);
                p + 1
            }
            0x15 => {
                self.vars[19] = self.vars[19].wrapping_sub(a0);
                p + 1
            }
            0x16 => {
                self.vars[8] = 0;
                if a0 != 0 {
                    let a = self.sel(a0);
                    self.actors[a].cycle = 4;
                } else {
                    self.g.program = true;
                    self.actors[0].cycle = 4;
                }
                p + 1
            }
            0x17 => {
                self.vars[8] = 0;
                let a = self.sel(a0);
                self.actors[a].cel = self.actors[a].cel_count;
                self.actors[a].cycle = 2;
                p + 1
            }
            0x18 => {
                self.vars[8] = 0;
                let a = self.sel(a0);
                self.actors[a].motion = 0x0E;
                self.g.target = (a1, a2);
                if a == 0 {
                    self.g.program = true;
                }
                p + 3
            }
            0x19 => {
                let a = self.sel(a0);
                self.actors[a].x = a1;
                self.actors[a].y = a2;
                self.objects[a0 as usize].x = a1;
                self.objects[a0 as usize].y = a2;
                p + 3
            }
            0x1A => {
                let a = self.sel(a0);
                self.actors[a].no_move = 1;
                p + 1
            }
            0x1B => {
                let a = self.sel(a0);
                self.actors[a].no_move = 0;
                p + 1
            }
            0x1C => {
                self.set_ego_view(a0, false);
                p + 1
            }
            0x1D => {
                self.vars[8] = 0;
                let a = self.sel(a0);
                let x = &mut self.actors[a];
                x.motion = 0x0F;
                x.flags |= 1;
                x.priority = 0x0F;
                p + 1
            }
            0x1E => {
                // Follow: v8 isn't cleared (the Apple version clears it).
                let a = self.sel(a0);
                let x = &mut self.actors[a];
                x.motion = 0x11;
                x.flags |= 1;
                x.priority = 0x0F;
                self.g.follow_state = 1;
                self.g.follow_dist = a1;
                p + 2
            }
            0x1F => {
                let a = self.sel(a0);
                self.actors[a].motion = 0x0C;
                self.actors[a].cycle = 0;
                self.g.slack = (5, 5);
                p + 1
            }
            0x20 => {
                // No clamping; a barrier of 0 (none) gets bands of 2.
                self.g.barrier_x = a0;
                self.g.band_x = if a0 == 0 { (2, 2) } else { (a1, a2) };
                p + 3
            }
            0x21 => {
                self.g.barrier_y = a0;
                self.g.band_y = if a0 == 0 { (2, 2) } else { (a1, a2) };
                p + 3
            }
            0x22 => {
                let a = self.sel(a0);
                self.actors[a].motion = 0x10;
                p + 1
            }
            0x23 => {
                let e = &mut self.actors[0];
                e.motion = 0x0D;
                e.step_y = 4;
                self.g.program = true;
                self.g.target = (e.x, a0);
                p + 1
            }
            0x24 => {
                self.wait_print_done();
                let v = self.load_view(a0);
                self.view_table[a0 as usize] = v;
                p + 1
            }
            0x25 => {
                self.overlay_picture(a0);
                p + 1
            }
            0x26 => {
                self.place_object(a0, a1, a2);
                p + 3
            }
            0x27 => {
                let a = self.sel(a0);
                self.actors[a].room = self.vars[1];
                let n = self.select_loop_cel(a, a1, a2);
                let x = &mut self.actors[a];
                x.cel_count = n;
                x.lp = a1;
                x.cel = a2;
                p + 3
            }
            0x28 => {
                let e = &mut self.actors[0];
                self.g.kept_step28 = e.step_x;
                e.step_x = 4;
                e.motion = 0x0E;
                self.g.target = (a0, e.y);
                p + 1
            }
            0x29 => p + 1,
            0x2A => {
                self.actors[0].cycle = 0;
                self.g.program2 = true;
                self.g.program = true;
                p
            }
            0x2B => {
                self.g.input_off = true;
                return Err(1);
            }
            0x2C => {
                self.load_sound_group(a0);
                p + 1
            }
            0x2D => {
                self.ego_special_view(a0, a1);
                p + 2
            }
            0x2E => {
                self.g.program2 = false;
                self.g.program = false;
                p
            }
            0x2F => {
                let a = self.sel(a0);
                self.actors[a].flags |= 1;
                self.actors[a].priority = a1;
                p + 2
            }
            0x30 => {
                // set_view: o's current view goes into the view table under o (when o
                // isn't v); then the selected actor takes the table's view v (loaded
                // or not).
                if a0 != a1 {
                    let a = self.sel(a0);
                    self.view_table[a0 as usize] = self.actors[a].view.clone();
                }
                let a = self.g.selected;
                self.actors[a].view = self.view_table[a1 as usize].clone();
                self.actors[a].cel = 0;
                self.actors[a].lp = 0;
                let n = self.select_loop_cel(a, 0, 0);
                let x = &mut self.actors[a];
                x.cel_count = n;
                x.lp = 0;
                x.cel = 0;
                p + 2
            }
            0x31 => {
                self.inventory_screen();
                while self.poll_key_bios() == 0 {}
                self.leave_text_mode();
                self.g.key_flags[4] = false;
                self.g.prompt_due = true;
                p
            }
            0x32 => {
                self.g.save_requested = true;
                p
            }
            0x33 => {
                // Restore: deferred to the main loop; the actions go on.
                self.g.key_flags[1] = true;
                p
            }
            0x34 => {
                let a = self.sel(a0);
                self.actors[a].flags &= 0xFE;
                p + 1
            }
            0x35 => {
                self.vars[a0 as usize] = self.vars[1];
                p + 1
            }
            0x36 => {
                self.vars[a1 as usize] = (self.vars[a0 as usize] == self.vars[1]) as u8;
                p + 2
            }
            0x37 => {
                let r = self.vars[a0 as usize];
                self.new_room(r);
                return Err(1);
            }
            0x38 => {
                // set_loop: the loop now; the cel and its count at the actor's next
                // update (`$16FF`, matched by view number).
                self.g.pending_loop = a0;
                let a = self.sel(a0);
                self.actors[a].lp = a1;
                p + 2
            }
            0x39 => {
                let a = self.sel(a0);
                self.g.current = a;
                self.add_to_picture();
                let a = self.g.selected;
                self.objects[a0 as usize].x = self.actors[a].x;
                self.objects[a0 as usize].y = self.actors[a].y;
                p + 1
            }
            0x3A => {
                let a = self.sel(a0);
                self.actors[a].flags |= 6;
                p + 1
            }
            0x3B => {
                let a = self.sel(a0);
                self.actors[a].flags &= 0xF9;
                p + 1
            }
            0x3C => {
                self.wait_print_done();
                self.text_screen(a0, a1);
                p + 2
            }
            0x3D => {
                self.print_at(s, a0, a1, s.word(p + 2));
                p + 4
            }
            0x3E => {
                self.print_text(s, s.word(p));
                p + 2
            }
            0x3F => {
                while self.poll_key_bios() == 0 {}
                self.leave_text_mode();
                p
            }
            0x40 => {
                // Another room's script, loaded as a third script that nothing runs.
                let keep = (self.vars[1], self.vars[2]);
                self.wait_print_done();
                self.load_room_script(a0);
                (self.vars[1], self.vars[2]) = keep;
                return Err(1);
            }
            0x41 => {
                self.set_ego_view(a0, true);
                p + 1
            }
            0x42 => {
                self.g.play_disk = true;
                p
            }
            0x43 => {
                // Game over: the restart key is cancelled and input switched off.
                self.g.key_flags[2] = false;
                self.g.input_off = true;
                return Err(1);
            }
            _ => p,
        })
    }
}
