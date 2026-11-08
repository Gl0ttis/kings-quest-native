//! kqroute <disk1.do> <disk2.do> <disk3.do> <route-file> [--keys OUT] [--trace OUT]
//!
//! Plays a route through the game with the native engine, using an autopilot, and records
//! every key it presses (at most one per pass) so the same input can be replayed into the
//! original game in refemu for comparison.
//!
//! Route file, one step per line (# starts a comment):
//!   room N [X1 Y1 X2 Y2]  travel to room N through edge exits, arriving where the
//!                     rectangle (if given) can be reached
//!   goto X Y [TOL]    walk to (X, Y) in the current room, within TOL pixels (default 2)
//!   area X1 Y1 X2 Y2  walk into a rectangle (as in the scripts' "ego in" tests)
//!   edge L|R|U|D      walk off that edge of the current room
//!   say TEXT          type a command and RETURN
//!   key HH            press a raw key (hex)
//!   wait N            let N passes go by
//!   waitvar V VAL [MAX]  let passes go by until variable V equals VAL
//!   expect score N    stop with an error unless the score is N
//!   expect room N     stop with an error unless in room N
use kq1_apple2::engine::{f, Resources, Scripted, World};
use kq1_apple2::kq::Disks;
use kq1_apple2::pic::{HEIGHT, WIDTH};
use std::collections::{HashMap, VecDeque};
use std::{env, fs};

#[derive(Clone)]
struct Driver {
    w: World,
    pass: usize,
    keys: Vec<(usize, u8)>,
    trace: String,
    exits: HashMap<u8, [u8; 5]>,
    log_seen: usize,
    #[allow(dead_code)]
    quiet: bool,
    /// Rooms whose scripts react to the ego being on water (v4 == 1).
    water_rooms: Vec<u8>,
    water_room: bool,
    /// Set while travelling; a score drop then means something was stolen.
    travelling: bool,
    /// Per room: rectangles where a cycle rule puts the ego in the water (sets v4), such
    /// as room 33's river lines. The planner keeps out of them.
    deadly: HashMap<u8, Vec<Zone>>,
    /// Messages and room changes of this run (kept with the state, so a retried line
    /// leaves no trace).
    events: Vec<String>,
    /// With KQ_VARLOG=file: every pass's variables, for this branch of the run only.
    varlog: Option<String>,
    /// `keepin`: in this room, the ego must stay inside one of these rectangles (rooms
    /// whose scripts make him fall anywhere else, like the clouds by the beanstalk).
    keep_in: Option<(u8, Vec<[i32; 4]>)>,
    /// Set by `final`: walk into a rectangle that ends the game (deadly zones ignored).
    ignore_deadly: bool,
}

const DIR_KEYS: [u8; 9] = [0, 0x0B, b'\'', 0x15, b'/', 0x0A, b'.', 0x08, b';'];
const DIRS: [(i32, i32); 9] = [(0, 0), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

impl Driver {
    fn dead(&self) -> bool {
        self.w.g.flag_6f05 != 0
    }

    /// One pass, optionally pressing one key first.
    fn step(&mut self, key: Option<u8>) -> Result<(), String> {
        if let Some(k) = key {
            self.keys.push((self.pass + 1, k));
            self.w.key(k);
        }
        let room = self.w.vars[1];
        let score_before = self.w.vars[19];
        self.w.cycle();
        self.pass += 1;
        self.water_room = self.water_rooms.contains(&self.w.vars[1]);
        let hash =
            self.w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
        if let Some(log) = &mut self.varlog {
            let vals: Vec<String> = self.w.vars[..0xDF].iter().map(|v| v.to_string()).collect();
            let act: Vec<String> = self.w.actors[..4].iter().flat_map(|r| r.iter()).map(|v| v.to_string()).collect();
            *log += &format!(
                "{}:{}|{}|{},{},{},{},{},{},{},{},{}\n",
                self.pass,
                vals.join(","),
                act.join(","),
                self.w.g.cel_w,
                self.w.g.cel_h,
                self.w.g.target_x,
                self.w.g.target_y,
                self.w.g.follow_state,
                self.w.g.patrol_side,
                self.w.g.follow_dist,
                self.w.g.last_pos[0],
                self.w.g.last_pos[1]
            );
        }
        let model = self.w.take_cost().cycles();
        let e = &self.w.actors[0];
        self.trace += &format!(
            "{},{},{},{},{},{},{model:.0},{hash:016x}\n",
            self.pass,
            self.w.vars[1],
            e[f::X],
            e[f::Y],
            e[f::DIR],
            self.w.vars[19]
        );
        if std::env::var("KQ_DUMPVARS").ok().and_then(|v| v.parse::<usize>().ok()) == Some(self.pass) {
            let vals: Vec<String> = self.w.vars[..0xDF].iter().map(|v| v.to_string()).collect();
            eprintln!("vars after pass {}: {}", self.pass, vals.join(","));
        }
        if std::env::var("KQ_WATCH").ok().and_then(|v| v.parse::<usize>().ok()).is_some_and(|n| self.pass >= n) {
            eprintln!(
                "pass {} room {} ego ({},{}) dir {} v3={} v4={} v24={} log {:?}",
                self.pass,
                self.w.vars[1],
                e[f::X],
                e[f::Y],
                e[f::DIR],
                self.w.vars[3],
                self.w.vars[4],
                self.w.vars[24],
                &self.w.log[self.log_seen..]
            );
        }
        for i in self.log_seen..self.w.log.len() {
            if self.w.log[i].starts_with("print") {
                let line = format!("    [{}] {}", self.pass, self.w.log[i]);
                self.events.push(line);
            }
        }
        self.log_seen = self.w.log.len();
        if self.w.vars[1] != room {
            let line =
                format!("  pass {}: room {} -> {}  (score {})", self.pass, room, self.w.vars[1], self.w.vars[19]);
            self.events.push(line);
        }
        if self.dead() {
            return Err(format!(
                "Graham died at pass {} in room {} at ({}, {})",
                self.pass,
                self.w.vars[1],
                e[f::X],
                e[f::Y]
            ));
        }
        // The sorcerer (rooms 29 and nearby) freezes Graham for a long time; better to
        // come by with different timing.
        if self.travelling
            && self
                .events
                .iter()
                .rev()
                .take(3)
                .any(|e| e.contains(&format!("[{}]", self.pass)) && e.contains("freezing"))
        {
            return Err(format!("robbed of time by the sorcerer at pass {} in room {}", self.pass, self.w.vars[1]));
        }
        if self.travelling && self.w.vars[19] < score_before {
            return Err(format!(
                "robbed at pass {} in room {}: score {} -> {}",
                self.pass, self.w.vars[1], score_before, self.w.vars[19]
            ));
        }
        Ok(())
    }

    fn wait(&mut self, n: usize) -> Result<(), String> {
        for _ in 0..n {
            self.step(None)?;
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        if self.w.g.key_dir != 0 {
            let k = DIR_KEYS[self.w.g.key_dir as usize];
            self.step(Some(k))?;
        }
        Ok(())
    }

    fn say(&mut self, text: &str) -> Result<(), String> {
        self.stop()?;
        for c in text.bytes() {
            self.step(Some(c))?;
        }
        self.step(Some(0x0D))?;
        self.wait(3)
    }

    /// Can the ego stand with its bottom-left at (x, y)? Mirrors aux $1280/$12C7 for the
    /// ego, and also refuses water (priority 0), which the engine allows but which starts
    /// swimming or drowning.
    fn standable(&self, x: i32, y: i32, w: i32) -> bool {
        if !self.standable_in(&self.w.base, x, y, w) {
            return false;
        }
        if let Some((room, rects)) = &self.keep_in {
            if *room == self.w.vars[1] && !rects.iter().any(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]) {
                return false;
            }
        }
        if let Some(zones) = self.deadly.get(&self.w.vars[1]).filter(|_| !self.ignore_deadly) {
            let live = |conds: &Vec<(usize, bool, u8)>| conds.iter().all(|&(v, eq, val)| (self.w.vars[v] == val) == eq);
            if zones.iter().any(|(z, c)| x >= z[0] && x <= z[2] && y >= z[1] && y <= z[3] && live(c)) {
                return false;
            }
        }
        // aux $0AC5: the ego can't stand on the same bottom row as another on-screen actor
        // when their x ranges touch.
        for s in 1..(self.w.g.actor_count as usize).min(4) {
            if !self.w.participates(s) {
                continue;
            }
            let a = &self.w.actors[s];
            let (ax, ay) = (a[f::X] as i32, a[f::Y] as i32);
            let aw = self.w.cel_dims(s).map_or(0, |d| d.0 as i32);
            if ay == y && ax < x + w && ax + aw >= x - 1 {
                return false;
            }
        }
        true
    }

    fn standable_in(&self, buf: &[u8], x: i32, y: i32, w: i32) -> bool {
        let fixed_flags = self.w.actors[0][f::FLAGS];
        let above_horizon = fixed_flags & 8 != 0;
        // x + w = 150 is where the engine clamps a step to the right edge; it only matters
        // (as a room change) when the room has a right-hand exit.
        let right_exit = self.exits.get(&self.w.vars[1]).is_none_or(|e| e[2] != 0);
        if x < 11 || x + w > 150 || (x + w == 150 && right_exit) || y > 162 || (!above_horizon && y < 41) || y < 3 {
            return false;
        }
        let prio = self.w.priority_at(0, y as u8);
        // Water is judged with the narrowest cel (cels are left-aligned): turning to face
        // up or down can put a narrower bottom row entirely on the water.
        let narrow = self.ego_min_width().min(w);
        let mut all_zero = true;
        for i in 0..w {
            let k = (y as usize - 3) * WIDTH + (x + i) as usize - 10;
            let p = buf[k] & 0xF0;
            if p != 0 && i < narrow {
                all_zero = false;
            }
            if p == 0xF0 && fixed_flags & 2 == 0 {
                return false;
            }
            if p >= 0x20 && p != 0xF0 && p == prio {
                return false;
            }
        }
        // Priority 0 doesn't block the ego. The whole bottom row on priority 0 means water
        // (v4 = 1), which starts swimming or drowning in rooms that react to it.
        !(all_zero && self.water_room)
    }

    fn ego(&self) -> (i32, i32) {
        (self.w.actors[0][f::X] as i32, self.w.actors[0][f::Y] as i32)
    }

    /// The widest cel in the ego's current view: the cel changes as Graham walks, so the
    /// planner needs a width that stays valid while he moves.
    fn ego_width(&self) -> i32 {
        let current = self.w.cel_dims(0).map_or(7, |d| d.0 as i32);
        let view = self.w.actor_view[0].and_then(|v| self.w.res.views.get(v as usize).cloned().flatten());
        view.map_or(current, |v| {
            v.loops.iter().flat_map(|l| l.cels.iter()).map(|c| c.width as i32).max().unwrap_or(current)
        })
        .max(current)
    }

    /// The width the ego has when stepping in each direction: the engine picks the loop
    /// for the direction before the move is checked (4 loops: right, left, down, up;
    /// diagonals take the side loop). Unknown cases use the widest cel.
    fn dir_widths(&self) -> [i32; 9] {
        let widest = self.ego_width();
        let mut out = [widest; 9];
        let view = self.w.actor_view[0].and_then(|v| self.w.res.views.get(v as usize).cloned().flatten());
        let Some(v) = view else { return out };
        let lw = |l: usize| v.loops.get(l).map(|l| l.cels.iter().map(|c| c.width as i32).max().unwrap_or(widest));
        for d in 1..=8usize {
            let l = match d {
                2..=4 => Some(0),
                6..=8 => Some(1),
                5 if v.loops.len() >= 4 => Some(2),
                1 if v.loops.len() >= 4 => Some(3),
                _ => None,
            };
            if let Some(w) = l.and_then(lw) {
                out[d] = w;
            }
        }
        out
    }

    /// The narrowest cel in the ego's current view.
    fn ego_min_width(&self) -> i32 {
        let current = self.w.cel_dims(0).map_or(7, |d| d.0 as i32);
        let view = self.w.actor_view[0].and_then(|v| self.w.res.views.get(v as usize).cloned().flatten());
        view.map_or(current, |v| {
            v.loops.iter().flat_map(|l| l.cels.iter()).map(|c| c.width as i32).min().unwrap_or(current)
        })
        .min(current)
    }

    /// Breadth-first search from the ego to any cell accepted by `goal`; returns the first
    /// direction to take, or 0 when already there, or None if unreachable.
    fn first_step(&self, goal: &dyn Fn(i32, i32) -> bool) -> Option<u8> {
        let wd = self.dir_widths();
        let (sx, sy) = self.ego();
        if goal(sx, sy) {
            return Some(0);
        }
        let idx = |x: i32, y: i32| (y as usize) * 160 + x as usize;
        let mut first = vec![255u8; 160 * 170];
        let mut q = VecDeque::new();
        first[idx(sx, sy)] = 0;
        q.push_back((sx, sy));
        while let Some((x, y)) = q.pop_front() {
            for d in 1..=8u8 {
                let (dx, dy) = DIRS[d as usize];
                let (nx, ny) = (x + dx, y + dy);
                if !(0..160).contains(&nx) || !(0..170).contains(&ny) || first[idx(nx, ny)] != 255 {
                    continue;
                }
                if !self.standable(nx, ny, wd[d as usize]) {
                    continue;
                }
                let fd = if (x, y) == (sx, sy) { d } else { first[idx(x, y)] };
                first[idx(nx, ny)] = fd;
                if goal(nx, ny) {
                    return Some(fd);
                }
                q.push_back((nx, ny));
            }
        }
        None
    }

    /// Walk until `goal` holds for the ego's position.
    fn walk(&mut self, goal: &dyn Fn(i32, i32) -> bool, what: &str) -> Result<(), String> {
        let room = self.w.vars[1];
        for _ in 0..3000 {
            if self.w.vars[1] != room {
                return Err(format!("left room {room} unexpectedly while walking to {what}"));
            }
            match self.first_step(goal) {
                Some(0) => return self.stop(),
                Some(d) => {
                    let key = if self.w.g.key_dir != d { Some(DIR_KEYS[d as usize]) } else { None };
                    self.step(key)?;
                }
                None => {
                    let (x, y) = self.ego();
                    let w = self.ego_width();
                    let around: Vec<String> = (1..=8)
                        .map(|d| {
                            let (dx, dy) = DIRS[d];
                            format!("{d}:{}", if self.standable(x + dx, y + dy, w) { "ok" } else { "no" })
                        })
                        .collect();
                    let dist = self.distances();
                    let reach: Vec<(i32, i32)> = (0..170)
                        .flat_map(|y| (0..160).map(move |x| (x, y)))
                        .filter(|&(x, y)| dist[(y as usize) * 160 + x as usize] != u32::MAX)
                        .collect();
                    if std::env::var_os("KQ_MAP").is_some() {
                        for y in 95..163 {
                            let row: String = (80..150)
                                .map(|x| {
                                    if dist[(y as usize) * 160 + x as usize] != u32::MAX {
                                        'r'
                                    } else if self.standable(x, y, w) {
                                        '.'
                                    } else {
                                        '#'
                                    }
                                })
                                .collect();
                            eprintln!("{y:3} {row}");
                        }
                    }
                    let bbox = reach
                        .iter()
                        .fold((999, 999, -1, -1), |b, &(x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)));
                    return Err(format!("no path to {what} in room {room} from {:?} (width {w}, neighbours {}; {} cells reachable within {:?})", self.ego(), around.join(" "), reach.len(), bbox));
                }
            }
        }
        Err(format!("gave up walking to {what}"))
    }

    fn goto(&mut self, tx: i32, ty: i32, tol: i32) -> Result<(), String> {
        self.walk(&|x, y| (x - tx).abs() <= tol && (y - ty).abs() <= tol, &format!("({tx}, {ty})"))
    }

    /// Distances from the ego to every reachable cell.
    fn distances(&self) -> Vec<u32> {
        let wd = self.dir_widths();
        let (sx, sy) = self.ego();
        let idx = |x: i32, y: i32| (y as usize) * 160 + x as usize;
        let mut dist = vec![u32::MAX; 160 * 170];
        let mut q = VecDeque::from([(sx, sy)]);
        dist[idx(sx, sy)] = 0;
        while let Some((x, y)) = q.pop_front() {
            for d in 1..=8 {
                let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                if (0..160).contains(&nx)
                    && (0..170).contains(&ny)
                    && dist[idx(nx, ny)] == u32::MAX
                    && self.standable(nx, ny, wd[d])
                {
                    dist[idx(nx, ny)] = dist[idx(x, y)] + 1;
                    q.push_back((nx, ny));
                }
            }
        }
        dist
    }

    /// Walk to the edge and keep going until the room changes. The crossing point is the
    /// reachable edge cell that best balances path length and closeness to the middle of
    /// the edge, so the ego avoids corners and enters the next room somewhere open.
    /// `next`: the exit to be taken in the destination room, if known; landing spots must
    /// be able to reach it.
    fn edge(&mut self, e: u8, next: Option<u8>) -> Result<(), String> {
        self.edge_to(e, next, None)
    }

    /// As `edge`; `goal`: a rectangle in the destination room that the landing spot must
    /// be able to reach.
    fn edge_to(&mut self, e: u8, next: Option<u8>, goal: Option<[i32; 4]>) -> Result<(), String> {
        let w = self.ego_width();
        // At or beyond the edge (a wider new sprite can already overhang it).
        let at_edge = move |x: i32, y: i32| match e {
            1 => x <= 11,
            2 => x + w >= 149,
            3 => y <= 41,
            _ => y >= 162,
        };
        let dist = self.distances();
        // Look ahead: the destination room's picture, and where the ego would land in it.
        let dest = self.exits.get(&self.w.vars[1]).map_or(0, |x| x[e as usize]);
        let here_water = self.water_room;
        self.water_room = self.water_rooms.contains(&dest);
        let dest_buf = self.w.res.pictures.get(dest as usize).cloned().flatten().map(|d| kq1_apple2::pic::draw(&d).buf);
        let open_area = |lx: i32, ly: i32| -> u32 {
            let Some(buf) = &dest_buf else { return 1000 };
            if !self.standable_in(buf, lx, ly, w) {
                return 0;
            }
            // Size of the region reachable from the landing spot (capped). With a known next
            // exit, the landing only counts if that exit can be reached from it.
            let mut seen = vec![false; 160 * 170];
            let mut q = VecDeque::from([(lx, ly)]);
            seen[(ly as usize) * 160 + lx as usize] = true;
            let mut n = 0;
            while let Some((x, y)) = q.pop_front() {
                n += 1;
                if let Some(g) = goal {
                    if x >= g[0] && x <= g[2] && y >= g[1] && y <= g[3] {
                        return 1500;
                    }
                } else if let Some(ne) = next {
                    let reached = match ne {
                        1 => x <= 11,
                        2 => x + w >= 149,
                        3 => y <= 41,
                        _ => y >= 162,
                    };
                    if reached {
                        return 1500;
                    }
                } else if n >= 1500 {
                    break;
                }
                for d in 1..=8 {
                    let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                    if (0..160).contains(&nx)
                        && (0..170).contains(&ny)
                        && !seen[(ny as usize) * 160 + nx as usize]
                        && self.standable_in(buf, nx, ny, w)
                    {
                        seen[(ny as usize) * 160 + nx as usize] = true;
                        q.push_back((nx, ny));
                    }
                }
            }
            if next.is_some() || goal.is_some() {
                0
            } else {
                n
            }
        };
        let landing = |x: i32, y: i32| -> (i32, i32) {
            match e {
                1 => (149 - w, y),
                2 => (11, y),
                3 => (x, 161),
                _ => (x, 41),
            }
        };
        let (ex, ey) = self.ego();
        // Already at or past the edge: cross here if the landing is good.
        if at_edge(ex, ey) && {
            let (lx, ly) = landing(ex, ey);
            open_area(lx, ly) >= 1500
        } {
            self.water_room = here_water;
            let room = self.w.vars[1];
            let d = [0, 7, 3, 1, 5][e as usize];
            if self.w.g.key_dir != d as u8 {
                self.step(Some(DIR_KEYS[d]))?;
            }
            for _ in 0..40 {
                if self.w.vars[1] != room {
                    self.stop()?;
                    return self.wait(4);
                }
                self.step(None)?;
            }
            return Err(format!("room {room}: pressing toward edge {e} did not change room"));
        }
        let mut best: Option<(u32, i32, i32)> = None;
        for y in 0..170 {
            for x in 0..160 {
                let d = dist[(y as usize) * 160 + x as usize];
                if d == u32::MAX || !at_edge(x, y) {
                    continue;
                }
                let (lx, ly) = landing(x, y);
                if open_area(lx, ly) < 1500 {
                    continue;
                }
                let off_centre = if e <= 2 { (y - 110).abs() } else { (x - 80).abs() } as u32;
                let cost = d + 3 * off_centre;
                if best.is_none_or(|b| cost < b.0) {
                    best = Some((cost, x, y));
                }
            }
        }
        self.water_room = here_water;
        if std::env::var_os("KQ_HOPS").is_some() {
            eprintln!("  edge {e} in room {}: crossing at {:?} (dest {dest}, w {w})", self.w.vars[1], best);
        }
        let Some((_, tx, ty)) = best else {
            return Err(format!("no path to edge {e} in room {} from {:?}", self.w.vars[1], self.ego()));
        };
        let room = self.w.vars[1];
        if let Err(err) = self.walk(&|x, y| x == tx && y == ty, &format!("edge {e} at ({tx}, {ty})")) {
            // Reaching the edge with the key still held carries the ego over it: fine.
            if self.w.vars[1] == room {
                return Err(err);
            }
            self.stop()?;
            return self.wait(4);
        }
        let d = [0, 7, 3, 1, 5][e as usize];
        self.step(Some(DIR_KEYS[d]))?;
        for _ in 0..40 {
            if self.w.vars[1] != room {
                self.stop()?;
                // Let the new room set itself up.
                return self.wait(4);
            }
            self.step(None)?;
        }
        Err(format!("room {room}: walking off edge {e} did not change room"))
    }

    /// Travel to room `to` through edge exits (shortest path in the exit graph).
    fn room(&mut self, to: u8, goal: Option<[i32; 4]>) -> Result<(), String> {
        self.travelling = true;
        let r = self.room_inner(to, goal);
        self.travelling = false;
        r
    }

    fn room_inner(&mut self, to: u8, goal: Option<[i32; 4]>) -> Result<(), String> {
        // Exits found unreachable from where the ego stood; the route is recomputed
        // without them.
        let mut blocked: Vec<(u8, u8)> = Vec::new();
        let mut visits: HashMap<u8, u32> = HashMap::new();
        loop {
            let from = self.w.vars[1];
            if from == to {
                return Ok(());
            }
            let v = visits.entry(from).or_insert(0);
            *v += 1;
            if *v > 3 {
                return Err(format!("going round in circles at room {from} (heading for room {to})"));
            }
            let mut prev: HashMap<u8, (u8, u8)> = HashMap::new();
            let mut q = VecDeque::from([from]);
            while let Some(r) = q.pop_front() {
                if r == to {
                    break;
                }
                for e in 1..=4u8 {
                    let n = self.exits.get(&r).map_or(0, |x| x[e as usize]);
                    if n != 0 && n != from && !prev.contains_key(&n) && !blocked.contains(&(r, e)) {
                        prev.insert(n, (r, e));
                        q.push_back(n);
                    }
                }
            }
            if !prev.contains_key(&to) {
                return Err(format!("no exit route from room {from} to room {to}"));
            }
            // The room path, as (room, exit) hops from here.
            let mut hops = Vec::new();
            let mut r = to;
            while r != from {
                let (p, e) = prev[&r];
                hops.push((p, e));
                r = p;
            }
            hops.reverse();
            if std::env::var_os("KQ_HOPS").is_some() {
                eprintln!("pass {} room {from} at {:?}: hops {hops:?} blocked {blocked:?}", self.pass, self.ego());
            }
            let e = hops[0].1;
            let next = hops.get(1).map(|h| h.1);
            let last_goal = if hops.len() == 1 { goal } else { None };
            match self.edge_to(e, next, last_goal) {
                Err(err) if err.starts_with("no path") && self.w.vars[1] == from && blocked.len() < 8 => {
                    blocked.push((from, e));
                }
                r => r.map_err(|err| format!("{err} (heading for room {to})"))?,
            }
        }
    }
}

/// Cycle rules "ego in [x1, y1, x2, y2] ... -> A" whose action leads to drowning or
/// death: it sets v4 (script-drawn water lines the priority map doesn't show) or v32 (the
/// death flag), or sets a room flag that triggers such an action later (followed through
/// rules like room 25's stream: v112 -> v110 -> timer -> v32). Widened by one pixel.
/// A deadly rectangle and the rule's other conditions (variable, equal?, value).
type Zone = ([i32; 4], Vec<(usize, bool, u8)>);

fn deadly_zones() -> HashMap<u8, Vec<Zone>> {
    let mut out: HashMap<u8, Vec<Zone>> = HashMap::new();
    for room in 1..=84u8 {
        let Ok(text) = fs::read_to_string(format!("extracted/scripts/{room:03}.txt")) else { continue };
        // Action bodies, by label.
        let mut actions: HashMap<String, Vec<String>> = HashMap::new();
        let mut rules: Vec<(String, String)> = Vec::new(); // (condition text, target label)
        let mut section = "";
        let mut label = String::new();
        for l in text.lines() {
            if let Some(name) = l.strip_prefix("== ") {
                section = if name.starts_with("ACTIONS") {
                    "a"
                } else if name.starts_with("CYCLE") {
                    "c"
                } else {
                    ""
                };
                continue;
            }
            match section {
                "a" => {
                    if let Some(rest) = l.strip_prefix("  A") {
                        label = format!("A{}", rest.trim_end_matches(':'));
                    } else if let Some(op) = l.trim_start().split_once("  ").map(|x| x.1.trim().to_string()) {
                        actions.entry(label.clone()).or_default().push(op);
                    }
                }
                "c" => {
                    if let Some((cond, target)) = l.split_once("-> ") {
                        rules.push((cond.to_string(), target.trim().to_string()));
                    }
                }
                _ => {}
            }
        }
        // Flag values (room variables 32 and up) whose setting leads to death; v32 = 1 is
        // the death flag itself. `set V` gives V = 1, `assign V X` gives X, and
        // `increment V` counts as any value.
        let mut deadly: Vec<(u32, u32)> = vec![(32, 1)];
        let sets = |op: &str, dv: &[(u32, u32)]| {
            let w: Vec<&str> = op.split_whitespace().collect();
            let num = |i: usize| w.get(i).and_then(|x| x.parse::<u32>().ok());
            match w.first().copied() {
                Some("set") => num(1).is_some_and(|v| dv.contains(&(v, 1))),
                Some("assign") => matches!((num(1), num(2)), (Some(v), Some(x)) if dv.contains(&(v, x))),
                Some("increment") => num(1).is_some_and(|v| dv.iter().any(|d| d.0 == v)),
                _ => false,
            }
        };
        let action_deadly = |a: &str, dv: &[(u32, u32)]| {
            actions.get(a).is_some_and(|ops| ops.iter().any(|op| op == "set 4" || sets(op, dv)))
        };
        loop {
            let mut added = false;
            for (cond, target) in &rules {
                if !action_deadly(target, &deadly) {
                    continue;
                }
                // The rule's trigger flags: room variables tested for a non-zero value.
                let tail = cond.rsplit(']').next().unwrap_or(cond);
                for t in tail.split([' ', '&']) {
                    let Some((v, val)) = t.strip_prefix('v').and_then(|t| t.split_once("==")) else { continue };
                    let (Ok(v), Ok(val)) = (v.parse::<u32>(), val.parse::<u32>()) else { continue };
                    if v >= 32 && val != 0 && !deadly.contains(&(v, val)) {
                        deadly.push((v, val));
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        let deadly_vars = deadly;
        for (cond, target) in &rules {
            if !cond.contains("ego in [") || !action_deadly(target, &deadly_vars) {
                continue;
            }
            let inner = &cond[cond.find('[').unwrap() + 1..cond.find(']').unwrap()];
            let n: Vec<i32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let tail = cond.rsplit(']').next().unwrap_or("");
            let conds: Vec<(usize, bool, u8)> = tail
                .split([' ', '&'])
                .filter_map(|t| {
                    let t = t.strip_prefix('v')?;
                    let (v, eq, val) = if let Some((v, val)) = t.split_once("==") {
                        (v, true, val)
                    } else {
                        let (v, val) = t.split_once("!=")?;
                        (v, false, val)
                    };
                    Some((v.parse().ok()?, eq, val.parse().ok()?))
                })
                .collect();
            if n.len() == 4 {
                out.entry(room).or_default().push(([n[0] - 1, n[1] - 1, n[2] + 1, n[3] + 1], conds));
            }
        }
    }
    if std::env::var_os("KQ_DEADLY").is_some() {
        let mut rooms: Vec<_> = out.iter().collect();
        rooms.sort();
        for (r, z) in rooms {
            eprintln!("deadly zones room {r}: {z:?}");
        }
    }
    out
}

/// Rooms whose scripts test v4 == 1 (the ego on water).
fn water_rooms() -> Vec<u8> {
    (1..=84u8)
        .filter(|r| fs::read_to_string(format!("extracted/scripts/{r:03}.txt")).is_ok_and(|t| t.contains("v4==1")))
        .collect()
}

fn load_exits() -> HashMap<u8, [u8; 5]> {
    // Built by tools/room_map.py from the room scripts' v3 rules, plus a few by hand.
    let text = fs::read_to_string("routes/room_exits.json").expect("routes/room_exits.json (run tools/room_map.py)");
    let mut out = HashMap::new();
    for part in text.trim_matches(|c| c == '{' || c == '}').split("}, ") {
        let Some((room, rest)) = part.split_once(": {") else { continue };
        let room: u8 = room.trim_matches('"').parse().unwrap();
        let mut ex = [0u8; 5];
        for kv in rest.trim_end_matches('}').split(", ").filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once(": ").unwrap();
            ex[k.trim_matches('"').parse::<usize>().unwrap()] = v.parse().unwrap();
        }
        out.insert(room, ex);
    }
    out
}

/// Run one route line.
fn run_line(d: &mut Driver, line: &str) -> Result<(), String> {
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let nums: Vec<i32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    match cmd {
        "room" => d.room(nums[0] as u8, (nums.len() == 5).then(|| [nums[1], nums[2], nums[3], nums[4]])),
        "goto" => d.goto(nums[0], nums[1], *nums.get(2).unwrap_or(&2)),
        "area" => {
            // Walk to the nearest reachable point inside a script rectangle (inset by up
            // to 2 so small drifts keep the ego inside).
            let ix = 2.min((nums[2] - nums[0]) / 2);
            let iy = 2.min((nums[3] - nums[1]) / 2);
            let (x1, y1, x2, y2) = (nums[0] + ix, nums[1] + iy, nums[2] - ix, nums[3] - iy);
            d.walk(&|x, y| x >= x1 && x <= x2 && y >= y1 && y <= y2, &format!("area [{x1},{y1},{x2},{y2}]"))
        }
        "hold" => {
            // hold U|D|L|R: keep walking that way (pressing a held direction again stops).
            let dir = match rest.trim() {
                "U" => 1,
                "R" => 3,
                "D" => 5,
                _ => 7,
            };
            if d.w.g.key_dir == dir as u8 {
                d.step(None)
            } else {
                d.step(Some(DIR_KEYS[dir]))
            }
        }
        "jumpuntil" => {
            // jumpuntil V VALUE MAX: keep jumping (typing "jump" whenever Graham is on the
            // ground) until vV == VALUE.
            let (v, val, max) = (nums[0] as usize, nums[1] as u8, nums[2] as usize);
            let start = d.pass;
            while d.pass < start + max {
                if d.w.vars[v] == val {
                    return Ok(());
                }
                if d.w.vars[20] == 0 && d.w.vars[45] == 0 {
                    d.say("jump")?;
                } else {
                    d.step(None)?;
                }
            }
            Err(format!("v{v} did not become {val} within {max} passes of jumping"))
        }
        "final" => {
            // final X1 Y1 X2 Y2: walk into the rectangle that ends the game; the death flag
            // it leads to is the end of the story, so deadly zones don't count.
            let (x1, y1, x2, y2) = (nums[0], nums[1], nums[2], nums[3]);
            d.ignore_deadly = true;
            let r =
                d.walk(&|x, y| x >= x1 && x <= x2 && y >= y1 && y <= y2, &format!("final area [{x1},{y1},{x2},{y2}]"));
            d.ignore_deadly = false;
            r
        }
        "keepin" => {
            // keepin X1 Y1 X2 Y2 [...]: confine the planner to these rectangles in this room.
            // With no rectangles, the restriction is lifted.
            let rects: Vec<[i32; 4]> =
                nums.chunks(4).filter(|c| c.len() == 4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
            d.keep_in = if rects.is_empty() { None } else { Some((d.w.vars[1], rects)) };
            Ok(())
        }
        "require" => {
            // require V VALUE: a condition set by chance (retried with different timing).
            let (v, val) = (nums[0] as usize, nums[1] as u8);
            if d.w.vars[v] == val {
                Ok(())
            } else {
                Err(format!(
                    "required v{v} == {val}, found {} (v14 {}, counter {:02X}, rng {:04X})",
                    d.w.vars[v], d.w.vars[14], d.w.g.cycle, d.w.g.rng_ptr
                ))
            }
        }
        "enter" => {
            // enter ROOM X1 Y1 X2 Y2: walk into a doorway rectangle until the room changes.
            let (to, r) = (nums[0] as u8, [nums[1], nums[2], nums[3], nums[4]]);
            let ix = 2.min((r[2] - r[0]) / 2);
            let iy = 2.min((r[3] - r[1]) / 2);
            let (x1, y1, x2, y2) = (r[0] + ix, r[1] + iy, r[2] - ix, r[3] - iy);
            // With "water" at the end, priority-0 ground (a hole, a pool) may be walked on.
            let here = d.w.vars[1];
            let saved = d.water_rooms.clone();
            if rest.trim_end().ends_with("water") {
                d.water_rooms.retain(|&r| r != here);
                d.water_room = false;
            }
            let res =
                d.walk(&|x, y| x >= x1 && x <= x2 && y >= y1 && y <= y2, &format!("doorway [{x1},{y1},{x2},{y2}]"));
            d.water_rooms = saved;
            for _ in 0..40 {
                if d.w.vars[1] == to {
                    d.stop()?;
                    return d.wait(4);
                }
                d.step(None)?;
            }
            res.and(Err(format!("did not enter room {to} (in room {})", d.w.vars[1])))
        }
        "edge" => d.edge(
            match rest.trim() {
                "L" => 1,
                "R" => 2,
                "U" => 3,
                _ => 4,
            },
            None,
        ),
        "say" => d.say(rest.trim()),
        "key" => d.step(Some(u8::from_str_radix(rest.trim(), 16).unwrap())),
        "wait" => d.wait(nums[0] as usize),
        "waitvar" => {
            // waitvar V VALUE MAX: let passes go by until vV == VALUE (at most MAX).
            let (v, val, max) = (nums[0] as usize, nums[1] as u8, *nums.get(2).unwrap_or(&500) as usize);
            for _ in 0..max {
                if d.w.vars[v] == val {
                    return Ok(());
                }
                d.step(None)?;
            }
            Err(format!("v{v} did not become {val} within {max} passes"))
        }
        "expect" => {
            let (what, v) = rest.split_once(' ').unwrap();
            let v: u8 = v.trim().parse().unwrap();
            let actual = if what == "score" { d.w.vars[19] } else { d.w.vars[1] };
            if actual == v {
                Ok(())
            } else {
                Err(format!("expected {what} {v}, found {actual}"))
            }
        }
        _ => Err(format!("unknown step {cmd}")),
    }
}

fn retryable(d: &Driver, e: &str) -> bool {
    d.dead() || e.contains("robbed") || e.starts_with("required")
}

/// A copy of the driver with a fresh headless front end.
fn snapshot(d: &Driver) -> Driver {
    let mut c = d.clone();
    c.w.io = Some(Box::new(Scripted::default()));
    c
}

/// Play the route. Each line starts from a snapshot; if Graham dies or is robbed during
/// it, the line is retried from the snapshot after 1-30 extra passes of waiting (which
/// changes the random encounters). If that fails, the previous line is retried with
/// extra waiting too. The result is one ordinary playthrough.
fn play(disks: &Disks, route: &str) -> (Driver, Option<(usize, String)>) {
    let mut w = World::new(Resources::load(disks, true).expect("resources"));
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let mut d = Driver {
        w,
        pass: 0,
        keys: Vec::new(),
        trace: String::from("pass,room,ego_x,ego_y,ego_dir,score,model_cycles,vars_hash\n"),
        exits: load_exits(),
        log_seen: 0,
        quiet: true,
        water_rooms: water_rooms(),
        water_room: true,
        travelling: false,
        deadly: deadly_zones(),
        events: Vec::new(),
        keep_in: None,
        ignore_deadly: false,
        varlog: std::env::var_os("KQ_VARLOG").map(|_| String::new()),
    };
    d.log_seen = d.w.log.len();
    for _ in 0..20 {
        if d.w.vars[25] != 0 && d.w.actors[0][f::DRAWN] != 0 {
            break;
        }
        d.step(None).expect("start");
    }
    let lines: Vec<(usize, String)> = route
        .lines()
        .enumerate()
        .map(|(n, l)| (n, l.split('#').next().unwrap().trim().to_string()))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    let mut snaps: Vec<Driver> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (n, line) = &lines[i];
        snaps.truncate(i);
        snaps.push(snapshot(&d));
        d.events.push(format!("{:4}: {line}   [pass {}, room {}, score {}]", n + 1, d.pass, d.w.vars[1], d.w.vars[19]));
        let first = run_line(&mut d, line);
        let Err(e) = first else {
            i += 1;
            continue;
        };
        if !retryable(&d, &e) {
            return (d, Some((*n, e)));
        }
        // Retry this line with extra waiting, then with extra waiting on the line before.
        let mut fixed = false;
        'search: for back in 0..=1usize {
            if back > i {
                break;
            }
            for extra_prev in 0..=(if back == 1 { 20 } else { 0 }) {
                for extra in 0..=30usize {
                    if back == 0 && extra == 0 {
                        continue;
                    }
                    let mut t = snapshot(&snaps[i - back]);
                    let mut ok = true;
                    if back == 1 {
                        let (pn, pline) = &lines[i - 1];
                        t.events.push(format!("{:4}: {pline}   (retried with {extra_prev} extra passes)", pn + 1));
                        ok = t.wait(extra_prev).and_then(|_| run_line(&mut t, pline)).is_ok();
                    }
                    if ok {
                        t.events.push(format!("{:4}: {line}   (after {extra} extra passes)", n + 1));
                        let r = t.wait(extra).and_then(|_| run_line(&mut t, line));
                        if std::env::var_os("KQ_RETRY").is_some() {
                            eprintln!("retry line {} back {back} prev+{extra_prev} +{extra}: {r:?}", n + 1);
                        }
                        if r.is_ok() {
                            if back == 1 {
                                snaps.truncate(i);
                                snaps.push(snapshot(&t));
                            }
                            d = t;
                            fixed = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        // Then with extra waiting two or three lines back (a chance condition decided at a
        // room change a few steps earlier).
        if !fixed {
            'deep: for back in 2..=3usize {
                if back > i {
                    break;
                }
                for extra in 1..=30usize {
                    let mut t = snapshot(&snaps[i - back]);
                    let mut new_snaps = Vec::new();
                    let mut ok = t.wait(extra).is_ok();
                    for j in i - back..=i {
                        if !ok {
                            break;
                        }
                        if j > i - back {
                            new_snaps.push(snapshot(&t));
                        }
                        let (jn, jline) = &lines[j];
                        let note = if j == i - back {
                            format!("   (retried with {extra} extra passes)")
                        } else {
                            String::new()
                        };
                        t.events.push(format!("{:4}: {jline}{note}", jn + 1));
                        ok = run_line(&mut t, jline).is_ok();
                    }
                    if ok {
                        snaps.truncate(i - back + 1);
                        snaps.extend(new_snaps);
                        d = t;
                        fixed = true;
                        break 'deep;
                    }
                }
            }
        }
        if !fixed {
            return (d, Some((*n, e)));
        }
        i += 1;
    }
    (d, None)
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args[p + 1].clone();
        args.drain(p..p + 2);
        Some(v)
    };
    let keys_out = opt("--keys");
    let trace_out = opt("--trace");
    if args.len() != 4 {
        eprintln!("usage: kqroute disk1.do disk2.do disk3.do ROUTE [--keys OUT] [--trace OUT]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args[..3]).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let route = fs::read_to_string(&args[3]).expect("read route");
    let (d, failed) = play(&disks, &route);
    for e in &d.events {
        println!("{e}");
    }
    println!("end: pass {}, room {}, score {}, ego {:?}", d.pass, d.w.vars[1], d.w.vars[19], d.ego());
    if let Ok(v) = std::env::var("KQ_VARS") {
        let vals: Vec<String> =
            v.split(',').filter_map(|x| x.parse::<usize>().ok()).map(|i| format!("v{i}={}", d.w.vars[i])).collect();
        println!("vars: {}", vals.join(" "));
    }
    if let (Ok(p), Some(log)) = (std::env::var("KQ_VARLOG"), &d.varlog) {
        fs::write(p, log).expect("write var log");
    }
    if let Ok(p) = std::env::var("KQ_BASE_OUT") {
        fs::write(p, &d.w.base).expect("write base buffer");
    }
    if let Some(p) = keys_out {
        let text: String = d.keys.iter().map(|(p, k)| format!("{p} {k:02X}\n")).collect();
        fs::write(p, text).expect("write keys");
    }
    if let Some(p) = trace_out {
        fs::write(p, &d.trace).expect("write trace");
    }
    if let Some((n, e)) = failed {
        eprintln!("route stopped: line {}: {e}", n + 1);
        std::process::exit(1);
    }
}

#[allow(dead_code)]
const _: usize = HEIGHT;
