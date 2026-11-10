//! The motion modes (`$3906`, cases through CS:2B3B), for an actor whose motion is
//! above 8:
//!
//! | Mode | Routine | Set by |
//! |---|---|---|
//! | 0C | chase the ego | op 1F |
//! | 0D | the ego down to a y | op 23 |
//! | 0E | to a target | ops 18, 28 |
//! | 0F | approach the ego | op 1D |
//! | 10 | wander | op 22 |
//! | 11 | follow the ego | op 1E |
//!
//! Every call ends by leaving the actor's position in one global "last position",
//! which the next call (for any actor) compares with to see whether it has moved.

use super::World;

/// The direction for each (x side, y side), DS:1D79: `[3 × xs + ys]`.
const DIRECTIONS: usize = 0x1D79;
/// The opposite direction (9 − d), DS:1E50.
const MIRROR: usize = 0x1E50;

/// Where a motion call goes next.
enum Next {
    /// `$3FA2`: done.
    Exit,
    /// `$3DE6`: the target is reached.
    Arrive,
    /// `$3E86`: head for the target.
    MoveTo,
    /// `$3918`: chase the ego.
    Chase,
}

impl World {
    /// `$3906`.
    pub fn motion(&mut self, a: usize) {
        let mut next = match self.actors[a].motion {
            0x0C => Next::Chase,
            0x0D => self.down_to(a),
            0x0E => Next::MoveTo,
            0x0F => {
                self.g.slack.0 = 5;
                Next::Chase
            }
            0x10 => {
                self.wander(a);
                Next::Exit
            }
            0x11 => self.follow(a),
            _ => Next::Exit,
        };
        loop {
            next = match next {
                Next::Exit => break,
                Next::Arrive => self.arrive(a),
                Next::MoveTo => self.move_to(a),
                Next::Chase => self.chase(a),
            };
        }
        self.g.last_pos = (self.actors[a].x, self.actors[a].y);
    }

    /// Has actor `a` stayed where the last motion call left an actor?
    fn still(&self, a: usize) -> bool {
        (self.actors[a].x, self.actors[a].y) == self.g.last_pos
    }

    /// `$3DBD`: mode 0D: down (direction 5, step 4) until y reaches the target, or
    /// the actor stops moving.
    fn down_to(&mut self, a: usize) -> Next {
        let x = &mut self.actors[a];
        if x.y >= self.g.target.1 {
            return Next::Arrive;
        }
        if x.dir == 0 || x.y != self.g.last_pos.1 {
            x.step_y = 4;
            x.dir = 5;
            return Next::Exit;
        }
        Next::Arrive
    }

    /// `$3DE6`: there. The loop 0 header's steps and delay come back; the ego returns
    /// to the player. A follower patrols on, to (0 or 96h, 0) by turns; a chaser or
    /// approacher reports it (v6 = its view), others set v8.
    fn arrive(&mut self, a: usize) -> Next {
        self.select_loop(a, 0);
        if a == 0 {
            self.g.program2 = false;
            self.g.program = false;
        }
        let m = self.actors[a].motion;
        if m == 0x11 {
            self.g.target.1 = 0;
            self.g.follow_state = 0;
            if self.g.patrol_side != 0 {
                self.g.target.0 = 0x96;
                self.g.patrol_side = 0;
            } else {
                self.g.target.0 = 0;
                self.g.patrol_side = 1;
            }
            return Next::MoveTo;
        }
        if m == 0x0C || m == 0x0F {
            self.vars[6] = self.actors[a].view_num;
        } else {
            self.vars[8] = 1;
        }
        let x = &mut self.actors[a];
        if m == 0x0F {
            x.flags = 0;
        }
        x.dir = 0;
        x.motion = 0;
        Next::Exit
    }

    /// `$3E86`: mode 0E: toward the target, within the actor's steps (a window of
    /// −step..=+step on each axis).
    fn move_to(&mut self, a: usize) -> Next {
        let x = &self.actors[a];
        let dx = x.x as i32 - self.g.target.0 as i32;
        let dy = x.y as i32 - self.g.target.1 as i32;
        if dx == 0 && dy == 0 {
            return Next::Arrive;
        }
        let (sx, sy) = (x.step_x as i32, x.step_y as i32);
        let xs = if dx < -sx {
            2
        } else if dx > sx {
            0
        } else {
            1
        };
        let ys = if dy < -sy {
            0
        } else if dy > sy {
            2
        } else {
            1
        };
        let d = self.res.ds[DIRECTIONS + 3 * xs + ys];
        self.actors[a].dir = d;
        if d == 0 {
            Next::Arrive
        } else {
            Next::Exit
        }
    }

    /// `$3918`: modes 0C, 0F and 11: after the ego. With a barrier hit (by anyone),
    /// along the barrier toward it; when stuck, a detour (a state machine shared by
    /// every actor); otherwise straight at it, within the slack. A follower (11) also
    /// strikes when it reaches the ego.
    fn chase(&mut self, a: usize) -> Next {
        let m = self.actors[a].motion;
        let v6 = self.vars[6];
        if v6 != 0 && v6 == self.actors[a].view_num {
            if m != 0x11 {
                self.g.slack = (0, 0);
                return Next::Arrive;
            }
            self.vars[6] = 0;
        }
        // The barrier latch: once any chaser is stopped by a barrier, chasers follow it
        // while it stands.
        if self.actors[a].no_move == 2 {
            self.actors[a].no_move = 0;
            self.g.barrier_latch = 1;
        }
        if self.g.barrier_latch != 0 {
            let (ex, ey) = (self.actors[0].x, self.actors[0].y);
            let x = &mut self.actors[a];
            if self.g.barrier_x != 0 {
                x.dir = if ey < x.y {
                    1
                } else if ey > x.y {
                    5
                } else if ex < x.x {
                    7
                } else {
                    3
                };
                return Next::Exit;
            }
            if self.g.barrier_y != 0 {
                x.dir = if ex < x.x {
                    7
                } else if ex > x.x {
                    3
                } else if ey < x.y {
                    1
                } else {
                    5
                };
                return Next::Exit;
            }
            self.g.barrier_latch = 0;
        }
        // $3A42: stuck? Turn, and start (or restart) the detour.
        if self.still(a) {
            if self.g.detour_state == 0 {
                self.g.detour_turn = !self.g.detour_turn;
                self.g.detour_state = 1;
            } else {
                self.g.detour_state = if self.g.detour_state == 3 { 4 } else { 1 };
            }
            self.rotate(a);
        }
        loop {
            match self.g.detour_state {
                0 => return self.chase_straight(a),
                1 => {
                    self.g.detour_state = 2;
                    self.g.detour_count = 1;
                    self.g.detour_len = 1;
                    return Next::Exit;
                }
                2 => {
                    if self.g.detour_count != 0 {
                        self.g.detour_count -= 1;
                        return Next::Exit;
                    }
                    self.g.detour_state = 3;
                    return self.chase_straight(a);
                }
                3 => {
                    self.g.detour_len = 1;
                    self.g.detour_state = 0;
                    return self.chase_straight(a);
                }
                4 => {
                    let d = self.actors[a].dir as usize;
                    self.actors[a].dir = self.res.ds[MIRROR + d];
                    self.g.detour_state = 5;
                    self.g.detour_count = self.g.detour_len;
                    return Next::Exit;
                }
                5 => {
                    if self.g.detour_count != 0 {
                        self.g.detour_count -= 1;
                        return Next::Exit;
                    }
                    self.g.detour_turn = !self.g.detour_turn;
                    self.g.detour_state = 2;
                    self.g.detour_len = self.g.detour_len.wrapping_add(1);
                    self.g.detour_count = self.g.detour_len;
                    self.rotate(a);
                }
                _ => return Next::Exit,
            }
        }
    }

    /// `$3A90`: one step round, the way the detour turns.
    fn rotate(&mut self, a: usize) {
        let x = &mut self.actors[a];
        if self.g.detour_turn != 0 {
            x.dir = if x.dir < 2 { 8 } else { x.dir - 1 };
        } else {
            x.dir = x.dir.wrapping_add(1);
            if x.dir >= 9 {
                x.dir = 1;
            }
        }
    }

    /// `$3AD5`: straight at the ego (or the target, for mode 0E, which never comes
    /// here), within the slack (raised to the steps): a window of (−slack, +slack].
    fn chase_straight(&mut self, a: usize) -> Next {
        let x = &self.actors[a];
        let m = x.motion;
        let (tx, ty) = if m == 0x0E { self.g.target } else { (self.actors[0].x, self.actors[0].y) };
        let dx = x.x as i32 - tx as i32;
        let dy = x.y as i32 - ty as i32;
        let sx = (self.g.slack.0 as i32).max(x.step_x as i32);
        let sy = (self.g.slack.1 as i32).max(x.step_y as i32);
        let xs = if dx <= -sx {
            2
        } else if dx > sx {
            0
        } else {
            1
        };
        let ys = if dy <= -sy {
            0
        } else if dy > sy {
            2
        } else {
            1
        };
        let d = self.res.ds[DIRECTIONS + 3 * xs + ys];
        self.actors[a].dir = d;
        self.g.chase_dir = d;
        if d == 0 {
            return Next::Arrive;
        }
        if m != 0x11 {
            return Next::Exit;
        }
        self.strike(a)
    }

    /// `$3BE8`: a follower near the ego. Past the edge report (v21 = its view) it has
    /// arrived; touching the ego, it has caught it (v6 = its view; near the right edge
    /// the ego is moved to the follower's x first); close below the ego, it rises
    /// toward it.
    fn strike(&mut self, a: usize) -> Next {
        let x = &self.actors[a];
        let (ex, ey) = (self.actors[0].x, self.actors[0].y);
        let reach = x.y as i32 + x.step_y as i32 + self.g.follow_dist as i32;
        if reach > ey as i32 {
            let v21 = self.vars[21];
            if v21 != 0 {
                self.vars[21] = 0;
                if v21 == self.actors[a].view_num {
                    return Next::Arrive;
                }
            }
        }
        if self.touches_ego(a) == 0 {
            let w = self.actors[a].cel_new.size().1;
            if w.wrapping_add(ex) > 0x9D {
                self.g.current = 0;
                self.erase_actor();
                self.actors[0].x = self.actors[a].x;
                self.nudge_actor(0);
            }
            self.vars[6] = self.actors[a].view_num;
            return Next::Arrive;
        }
        let x = &mut self.actors[a];
        if (ey as i32 - x.y as i32) < self.g.follow_dist as i32 - x.step_y as i32 {
            x.dir = if x.x < ex { 2 } else { 8 };
        }
        Next::Exit
    }

    /// `$3F42`: mode 11: chasing the ego (with the slack's y the follow distance) while
    /// the follow state is set; otherwise patrolling toward the target until it stops
    /// moving, which sets the state.
    fn follow(&mut self, a: usize) -> Next {
        loop {
            if self.g.follow_state != 0 {
                self.g.slack.1 = self.g.follow_dist;
                return Next::Chase;
            }
            if !self.still(a) {
                return Next::MoveTo;
            }
            self.vars[21] = 0;
            self.g.follow_state = 1;
        }
    }

    /// `$4122`: mode 10: wander. A new direction when the timer runs out, when there
    /// is none, or when the trial step is blocked: from a byte of the engine's code,
    /// (b & 7) + 1, until one isn't blocked; then the timer is ((b >> 4) + 1) × 4.
    fn wander(&mut self, a: usize) {
        self.g.wander_timer = self.g.wander_timer.wrapping_sub(1);
        if self.g.wander_timer != 0 && self.actors[a].dir != 0 && !self.wander_blocked(a) {
            return;
        }
        let mut si = self.g.random;
        let b = loop {
            let b = self.res.code_byte(si);
            si = si.wrapping_add(1);
            self.actors[a].dir = (b & 7) + 1;
            if !self.wander_blocked(a) {
                break b;
            }
        };
        if si >= 0x5000 {
            si = 0x0B00;
        }
        self.g.random = si;
        self.g.wander_timer = ((b >> 4) as u16 + 1) * 4;
    }

    /// `$416F`: is the wanderer's trial step blocked? By the picture or another actor;
    /// or at an edge it reported itself (v21 = its view), every second time.
    fn wander_blocked(&mut self, a: usize) -> bool {
        self.vars[21] = 0;
        self.vars[22] = 0;
        if self.move_actor(a) {
            return true;
        }
        if self.vars[22] == 0 || self.vars[21] != self.actors[a].view_num {
            return false;
        }
        if self.g.wander_toggle == 0 {
            self.g.wander_toggle = 1;
            false
        } else {
            self.g.wander_toggle = 0;
            true
        }
    }
}
