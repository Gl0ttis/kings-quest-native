//! kqroute-pcjr DISK ROUTE [--events OUT] [--trace OUT] [--keys OUT]
//!
//! Plays a route through the original PCjr game, in refemu-pcjr, with an autopilot, and
//! writes the event log and the trace of that run: the reference the port is replayed
//! against (tests/engine.rs). It is the Apple port's `kqroute`, reading the original's
//! own memory, with the PCjr's geometry (src/engine/sprite.rs, `move_actor`).
//!
//! Route file, one step per line (# starts a comment):
//!   room N [X1 Y1 X2 Y2]  travel to room N through edge exits, arriving where the
//!                     rectangle (if given) can be reached
//!   goto X Y [TOL]    walk to (X, Y) in the current room, within TOL pixels (default 2)
//!   area X1 Y1 X2 Y2  walk into a rectangle (as in the scripts' "ego in" tests)
//!   enter ROOM X1 Y1 X2 Y2 [water]  walk into a doorway until the room changes
//!   edge L|R|U|D      walk off that edge of the current room
//!   hold U|D|L|R      keep walking that way for a pass
//!   say TEXT          type a command and RETURN
//!   prompt TEXT       the keys to type at the next prompt (the save dialog's letter)
//!   key HHHH          press a raw key (scan << 8 | character, hex)
//!   wait N            let N passes go by
//!   waitvar V VAL [MAX]  let passes go by until variable V equals VAL
//!   require V VAL     a condition set by chance (the line is retried with other timing,
//!                     or after round trips to a neighbouring room, which draw v14 anew)
//!   keepin X1 Y1 X2 Y2 ...  keep the planner in these rectangles in this room
//!   final X1 Y1 X2 Y2  walk into the game-ending rectangle
//!   dies N            wait up to N passes for Graham to die
//!   if V VAL STEP     the step only when variable V equals VAL
//!   map [Y]           print the planner's map of the room, or row Y's priorities (stderr)
//!   expect score N | expect room N

use kq1_pcjr::disk::Disk;
use kq1_pcjr_refemu::runner::{Runner, Stopped};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

/// The keypad keys for directions 1-8 (scan codes; DS:0D04 maps them back).
const DIR_KEYS: [u16; 9] = [0, 0x4800, 0x4900, 0x4D00, 0x5100, 0x5000, 0x4F00, 0x4B00, 0x4700];
const DIRS: [(i32, i32); 9] = [(0, 0), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
/// The picture.
const W: i32 = 160;
const H: i32 = 168;

/// A rectangle where a cycle rule kills or drowns Graham, with the rule's other
/// conditions (variable, equal?, value).
type Zone = ([i32; 4], Vec<(usize, bool, u8)>);
/// A room's entry rule "when v18 == FROM ... -> position 0 X Y".
type Entry = (Vec<(usize, bool, u8)>, (i32, i32));

/// What the route knows about the game, from the disk.
struct Game {
    disk: Disk,
    exits: HashMap<u8, [u8; 5]>,
    water_rooms: Vec<u8>,
    deadly: HashMap<u8, Vec<Zone>>,
    entries: HashMap<(u8, u8), Vec<Entry>>,
    /// The loop for each direction, for views with 2 and 4 loops (DS:174F, DS:1758).
    loops2: [u8; 9],
    loops4: [u8; 9],
    frame: Vec<u8>,
}

#[derive(Clone)]
struct Driver {
    r: Runner,
    g: Rc<Game>,
    water_room: bool,
    travelling: bool,
    events: Vec<String>,
    keep_in: Option<(u8, Vec<[i32; 4]>)>,
    ignore_deadly: bool,
    /// Keys to type at the next prompt.
    prompt: VecDeque<u16>,
    /// Keys pressed, by pass.
    keys: Vec<(u64, u16)>,
    /// The ego's narrowest cel, for the water test (set before each search).
    narrow: std::cell::Cell<i32>,
    /// Water counts as ground (`final`, `enter … water`).
    wade: bool,
    /// Graham is dead and the run goes on (`dies`).
    mourning: bool,
}

/// A key as INT 16h returns it.
fn ascii(c: u8) -> u16 {
    let scan = match c {
        b'\r' => 0x1C,
        b' ' => 0x39,
        0x1B => 0x01,
        0x08 => 0x0E,
        _ => 0,
    };
    scan << 8 | c as u16
}

impl Driver {
    fn v(&self, n: usize) -> u8 {
        self.r.v(n as u8)
    }
    /// Graham can't act: the game is over (op 43 or 2B switch input off, DS:125A).
    fn dead(&self) -> bool {
        self.r.ds(0x125A) != 0
    }
    fn ego(&self) -> (i32, i32) {
        (self.r.ds(0x11F9) as i32, self.r.ds(0x11FA) as i32)
    }
    fn key_dir(&self) -> u8 {
        self.r.ds(0x1279)
    }
    /// The direction once the keys still in the BIOS buffer are read (the timer takes
    /// them; the same direction again stops).
    fn intended_dir(&self) -> u8 {
        let mut d = self.key_dir();
        for &k in &self.r.m.keys {
            if let Some(n) = DIR_KEYS.iter().position(|&x| x == k && x != 0) {
                d = if d == n as u8 { 0 } else { n as u8 };
            }
        }
        d
    }
    fn actor(&self, s: u16) -> u16 {
        0x11E1 + 30 * s
    }
    /// An actor's current cel's (height, width).
    fn cel(&self, s: u16) -> (i32, i32) {
        let p = self.r.ds_w(self.actor(s) + 0x16);
        (self.r.ds(p) as i32, self.r.ds(p.wrapping_add(1)) as i32)
    }
    /// The ego's view, decoded.
    fn ego_view(&self) -> Option<kq1_apple2::view::View> {
        let v = self.r.ds(self.actor(0) + 5) as usize;
        self.g.disk.view(v).and_then(|raw| kq1_pcjr::view::decode(raw).ok())
    }

    /// Where the ego will be after this pass. The direction it walks in this pass is
    /// the one held now: a key pressed now reaches the game through the timer during the
    /// pass, and the game cycle reads it at the start of the next one. So the planner
    /// works from there.
    fn ahead(&self) -> (i32, i32) {
        let (x, y) = self.ego();
        let d = self.key_dir() as usize;
        if d == 0 || d > 8 {
            return (x, y);
        }
        let a = 0x11E1;
        let (sx, sy) = (self.r.ds(a + 0x0C) as i32, self.r.ds(a + 0x0D) as i32);
        let (nx, ny) = (x + DIRS[d].0 * sx, y + DIRS[d].1 * sy);
        match self.engine_ok(&|x, y| self.r.priority(x, y), nx, ny, self.dir_widths()[d]) {
            Some(_) => (nx, ny),
            None => (x, y),
        }
    }

    /// One pass, optionally pressing a key at its start.
    fn step(&mut self, key: Option<u16>) -> Result<(), String> {
        if let Some(k) = key {
            self.keys.push((self.r.passes, k));
            self.r.press(k);
        }
        let room = self.v(1);
        let score = self.v(19);
        loop {
            match self.r.run(1_500_000, 2_000_000_000)? {
                Stopped::Pass => break,
                Stopped::Prompt => {
                    if self.prompt.is_empty() {
                        self.r.press(0x1C0D);
                    } else {
                        for k in std::mem::take(&mut self.prompt) {
                            self.r.press(k);
                        }
                    }
                }
            }
        }
        self.water_room = self.g.water_rooms.contains(&self.v(1));
        if std::env::var("KQ_WATCH").ok().and_then(|v| v.parse::<u64>().ok()).is_some_and(|n| self.r.passes >= n) {
            eprintln!(
                "pass {} room {} ego {:?} dir {} key {:?} ahead {:?} program {} motion {:02X} no_move {} cycle {}",
                self.r.passes,
                self.v(1),
                self.ego(),
                self.key_dir(),
                key,
                self.ahead(),
                self.r.ds(0x1262),
                self.r.ds(0x11F0),
                self.r.ds(0x11E5),
                self.r.ds(0x11EF)
            );
        }
        if self.v(1) != room {
            let line = format!("  pass {}: room {} -> {}  (score {})", self.r.passes, room, self.v(1), self.v(19));
            self.events.push(line);
        }
        if self.dead() && !self.mourning {
            let (x, y) = self.ego();
            return Err(format!("Graham died at pass {} in room {} at ({x}, {y})", self.r.passes, self.v(1)));
        }
        if self.travelling && self.v(19) < score {
            return Err(format!(
                "robbed at pass {} in room {}: score {} -> {}",
                self.r.passes,
                self.v(1),
                score,
                self.v(19)
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

    /// Stop walking: the direction key again.
    fn stop(&mut self) -> Result<(), String> {
        let d = self.intended_dir();
        if d != 0 && d < 9 {
            self.step(Some(DIR_KEYS[d as usize]))?;
        }
        Ok(())
    }

    fn say(&mut self, text: &str) -> Result<(), String> {
        self.stop()?;
        for c in text.bytes() {
            self.step(Some(ascii(c)))?;
        }
        self.step(Some(ascii(b'\r')))?;
        // The timer takes the keys into the game's ring and prints the replies a word at
        // a time: wait until the line is read and the printing is done.
        for _ in 0..600 {
            if self.r.m.keys.is_empty() && self.r.ds(0x0E98) == 0 && self.v(23) == 0 {
                break;
            }
            self.step(None)?;
        }
        self.wait(3)
    }

    /// Can the ego stand with its bottom-left at (x, y), `w` wide? `move_actor`
    /// (`$2F2D`) for the ego on dry land, plus the planner's own rules: no water in rooms
    /// that react to it, no deadly zones, no other actor's row.
    fn standable(&self, x: i32, y: i32, w: i32) -> bool {
        self.standable_near(x, y, w, false)
    }

    /// As `standable`; `near` (within two steps of where a search starts) lets the ego
    /// back out of the edge margin.
    fn standable_near(&self, x: i32, y: i32, w: i32, near: bool) -> bool {
        if !self.standable_in(&|x, y| self.r.priority(x, y), x, y, w) {
            return false;
        }
        // A pass of walking is committed before a key takes effect: on a side with an
        // exit, the two outermost cells are a step or two from leaving the room (a key can take
        // two passes to act, when a pass goes by without a timer tick), so keep off them.
        if !near && self.in_margin(x, y, w) {
            return false;
        }
        if let Some((room, rects)) = &self.keep_in {
            if *room == self.v(1) && !rects.iter().any(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]) {
                return false;
            }
        }
        if let Some(zones) = self.g.deadly.get(&self.v(1)).filter(|_| !self.ignore_deadly) {
            let live = |c: &Vec<(usize, bool, u8)>| c.iter().all(|&(v, eq, val)| (self.v(v) == val) == eq);
            if zones.iter().any(|(z, c)| x >= z[0] && x <= z[2] && y >= z[1] && y <= z[3] && live(c)) {
                return false;
            }
        }
        // `$3103`: another actor on screen on the same bottom row, x ranges touching.
        for s in 1..=(self.r.ds(0x125B) as u16).min(3) {
            let a = self.actor(s);
            if self.r.ds(a) != self.v(1) {
                continue;
            }
            let (ax, ay) = (self.r.ds(a + 0x18) as i32, self.r.ds(a + 0x19) as i32);
            let aw = self.cel(s).1;
            if ay == y && ax <= x + w && ax + aw >= x - 1 {
                return false;
            }
        }
        true
    }

    /// `move_actor`'s rules for the ego on dry land, on a priority map: inside 2 ≤ x,
    /// x + w ≤ 158, 28h ≤ y ≤ A5h; priority 15 blocks, so does the ego's own band (from
    /// 2). Whether the whole bottom row (by the narrowest cel) is on priority 0, water;
    /// None if blocked.
    fn engine_ok(&self, pri: &dyn Fn(usize, usize) -> u8, x: i32, y: i32, w: i32) -> Option<bool> {
        if x < 2 || x + w > 158 || !(0x28..=0xA5).contains(&y) {
            return None;
        }
        let flags = self.r.ds(self.actor(0) + 0x0B);
        let mut band = if y < 0x30 { 1 } else { ((y - 0x28) >> 3) as u8 & 0x0F };
        if flags & 1 != 0 {
            band = self.r.ds(self.actor(0) + 0x1D);
        }
        let narrow = self.narrow.get().min(w);
        let mut all_zero = true;
        for i in 0..w {
            let p = pri((x + i) as usize, y as usize);
            if p != 0 && i < narrow {
                all_zero = false;
            }
            if p == 0x0F && flags & 2 == 0 {
                return None;
            }
            if p > 1 && p == band && (p != 0x0F || flags & 2 == 0) {
                return None;
            }
        }
        Some(all_zero)
    }

    /// Would the engine let the ego stand at (x, y) where the planner must not: in the
    /// water of a room that reacts to it, or in a deadly zone?
    fn perilous(&self, x: i32, y: i32, w: i32) -> bool {
        let Some(water) = self.engine_ok(&|x, y| self.r.priority(x, y), x, y, w) else {
            return false;
        };
        if water && self.water_room && !self.wade {
            return true;
        }
        let live = |c: &Vec<(usize, bool, u8)>| c.iter().all(|&(v, eq, val)| (self.v(v) == val) == eq);
        self.g.deadly.get(&self.v(1)).filter(|_| !self.ignore_deadly).is_some_and(|zones| {
            zones.iter().any(|(z, c)| x >= z[0] && x <= z[2] && y >= z[1] && y <= z[3] && live(c))
        })
    }

    /// Is (x, y) in the edge margin (where only backing out is allowed)? The margin
    /// is waived near the start only then: otherwise the ego would creep into it a
    /// step at a time.
    fn in_margin(&self, x: i32, y: i32, w: i32) -> bool {
        self.g.exits.get(&self.v(1)).is_some_and(|ex| {
            ex[1] != 0 && x <= 3 || ex[2] != 0 && x + w >= 157 || ex[3] != 0 && y <= 0x29 || ex[4] != 0 && y >= 0xA4
        })
    }

    /// A step `d` onto (x, y): standable, and (a turn can come a pass late, when a pass
    /// goes by without a timer tick) not one step short of peril the same way.
    fn step_ok(&self, x: i32, y: i32, d: usize, w: i32, near: bool) -> bool {
        self.standable_near(x, y, w, near) && !self.perilous(x + DIRS[d].0, y + DIRS[d].1, w)
    }

    /// As `engine_ok`, and the planner keeps out of the water in rooms that react to it.
    fn standable_in(&self, pri: &dyn Fn(usize, usize) -> u8, x: i32, y: i32, w: i32) -> bool {
        match self.engine_ok(pri, x, y, w) {
            None => false,
            Some(water) => !(water && self.water_room && !self.wade),
        }
    }

    fn widths(&self) -> Vec<i32> {
        self.ego_view().map_or(vec![self.cel(0).1], |v| {
            v.loops.iter().flat_map(|l| l.cels.iter()).map(|c| c.width as i32).collect()
        })
    }
    fn ego_width(&self) -> i32 {
        self.widths().into_iter().max().unwrap_or(7).max(self.cel(0).1)
    }
    fn ego_min_width(&self) -> i32 {
        self.widths().into_iter().min().unwrap_or(7).min(self.cel(0).1)
    }

    /// The width the ego has stepping each way: the loop for the direction
    /// (`$16AB`: DS:1758 for 4 loops, DS:174F for 2), its widest cel.
    fn dir_widths(&self) -> [i32; 9] {
        let widest = self.ego_width();
        let mut out = [widest; 9];
        let Some(v) = self.ego_view() else { return out };
        for (d, o) in out.iter_mut().enumerate().skip(1) {
            let lp = match v.loops.len() {
                4 => Some(self.g.loops4[d]),
                2 => Some(self.g.loops2[d]),
                _ => None,
            };
            if let Some(w) =
                lp.and_then(|l| v.loops.get(l as usize)).and_then(|l| l.cels.iter().map(|c| c.width as i32).max())
            {
                *o = w;
            }
        }
        out
    }

    fn first_step(&self, goal: &dyn Fn(i32, i32) -> bool) -> Option<u8> {
        self.narrow.set(self.ego_min_width());
        let wd = self.dir_widths();
        let (sx, sy) = self.ahead();
        if goal(sx, sy) {
            return Some(0);
        }
        let in_margin = self.in_margin(sx, sy, self.ego_width());
        let idx = |x: i32, y: i32| (y * W + x) as usize;
        let mut first = vec![255u8; (W * H) as usize];
        let mut q = VecDeque::new();
        first[idx(sx, sy.min(H - 1))] = 0;
        q.push_back((sx, sy));
        while let Some((x, y)) = q.pop_front() {
            for d in 1..=8u8 {
                let (nx, ny) = (x + DIRS[d as usize].0, y + DIRS[d as usize].1);
                if !(0..W).contains(&nx) || !(0..H).contains(&ny) || first[idx(nx, ny)] != 255 {
                    continue;
                }
                let near = in_margin && (nx - sx).abs() <= 2 && (ny - sy).abs() <= 2;
                if !self.step_ok(nx, ny, d as usize, wd[d as usize], near) {
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

    fn walk(&mut self, goal: &dyn Fn(i32, i32) -> bool, what: &str) -> Result<(), String> {
        let room = self.v(1);
        for _ in 0..3000 {
            if self.v(1) != room {
                return Err(format!("left room {room} unexpectedly while walking to {what}"));
            }
            match self.first_step(goal) {
                Some(0) => return self.stop(),
                Some(d) => {
                    let key = if self.intended_dir() != d { Some(DIR_KEYS[d as usize]) } else { None };
                    self.step(key)?;
                }
                None => {
                    self.show_map(&self.distances());
                    return Err(format!("no path to {what} in room {room} from {:?}", self.ego()));
                }
            }
        }
        Err(format!("gave up walking to {what}"))
    }

    /// With KQ_MAP: the reachable ground (r), standable (.) and not (#), every other
    /// column, with the priorities from x 60.
    fn show_map(&self, dist: &[u32]) {
        if std::env::var_os("KQ_MAP").is_none() {
            return;
        }
        let wd = self.dir_widths();
        for y in (0x26..H).step_by(2) {
            let row: String = (0..W)
                .step_by(2)
                .map(|x| {
                    if dist[(y * W + x) as usize] != u32::MAX {
                        'r'
                    } else if self.standable(x, y, wd[5]) {
                        '.'
                    } else {
                        '#'
                    }
                })
                .collect();
            let pri: String =
                (0..W).step_by(2).map(|x| format!("{:X}", self.r.priority(x as usize, y as usize))).collect();
            eprintln!("{y:3} {row}  {pri}");
        }
    }

    fn goto(&mut self, tx: i32, ty: i32, tol: i32) -> Result<(), String> {
        self.walk(&|x, y| (x - tx).abs() <= tol && (y - ty).abs() <= tol, &format!("({tx}, {ty})"))
    }

    fn distances(&self) -> Vec<u32> {
        self.narrow.set(self.ego_min_width());
        let wd = self.dir_widths();
        let (sx, sy) = self.ahead();
        let in_margin = self.in_margin(sx, sy, self.ego_width());
        let idx = |x: i32, y: i32| (y * W + x) as usize;
        let mut dist = vec![u32::MAX; (W * H) as usize];
        let mut q = VecDeque::from([(sx, sy)]);
        dist[idx(sx, sy.min(H - 1))] = 0;
        while let Some((x, y)) = q.pop_front() {
            for d in 1..=8 {
                let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                if (0..W).contains(&nx)
                    && (0..H).contains(&ny)
                    && dist[idx(nx, ny)] == u32::MAX
                    && self.step_ok(nx, ny, d, wd[d], in_margin && (nx - sx).abs() <= 2 && (ny - sy).abs() <= 2)
                {
                    dist[idx(nx, ny)] = dist[idx(x, y)] + 1;
                    q.push_back((nx, ny));
                }
            }
        }
        dist
    }

    /// Walk off edge `e` (1 left, 2 right, 3 top, 4 bottom), where the landing in the
    /// next room is open and leads on to `next` (or to `goal`).
    fn edge_to(&mut self, e: u8, next: Option<u8>, goal: Option<[i32; 4]>) -> Result<(), String> {
        let w = self.ego_width();
        self.narrow.set(self.ego_min_width());
        // The cells next to the edge (the two outermost are kept off; see `standable`).
        let at_edge = move |x: i32, y: i32| match e {
            1 => x <= 4,
            2 => x + w >= 156,
            3 => y <= 0x2A,
            _ => y >= 0xA3,
        };
        let dist = self.distances();
        let dest = self.g.exits.get(&self.v(1)).map_or(0, |x| x[e as usize]);
        let here_water = self.water_room;
        self.water_room = self.g.water_rooms.contains(&dest);
        let dest_pic = self.g.disk.room(dest as usize).map(|r| kq1_pcjr::pic::draw_room(&self.g.frame, r.picture).buf);
        let dpri = |x: usize, y: usize| -> u8 { dest_pic.as_ref().map_or(4, |b| b[y * 160 + x] >> 4) };
        let open_area = |lx: i32, ly: i32| -> u32 {
            if dest_pic.is_none() {
                return 1000;
            }
            if !self.standable_in(&dpri, lx, ly, w) {
                return 0;
            }
            let mut seen = vec![false; (W * H) as usize];
            let mut q = VecDeque::from([(lx, ly)]);
            seen[(ly * W + lx) as usize] = true;
            let mut n = 0;
            while let Some((x, y)) = q.pop_front() {
                n += 1;
                if let Some(g) = goal {
                    if x >= g[0] && x <= g[2] && y >= g[1] && y <= g[3] {
                        return 1500;
                    }
                } else if let Some(ne) = next {
                    let reached = match ne {
                        1 => x <= 2,
                        2 => x + w >= 158,
                        3 => y <= 0x28,
                        _ => y >= 0xA5,
                    };
                    if reached {
                        return 1500;
                    }
                } else if n >= 1500 {
                    break;
                }
                for d in 1..=8 {
                    let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                    if (0..W).contains(&nx)
                        && (0..H).contains(&ny)
                        && !seen[(ny * W + nx) as usize]
                        && self.standable_in(&dpri, nx, ny, w)
                    {
                        seen[(ny * W + nx) as usize] = true;
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
        let scripted = self.g.entries.get(&(dest, self.v(1))).and_then(|list| {
            list.iter().find(|(c, _)| c.iter().all(|&(v, eq, val)| (self.v(v) == val) == eq)).map(|e| e.1)
        });
        let scripted_ok = scripted.is_some_and(|(sx, sy)| open_area(sx, sy) >= 1500);
        // `$457B`: the ego comes in at x 157 (then kept inside, 157 − width), x 2, y 165
        // or y 42.
        let landing = |x: i32, y: i32| -> (i32, i32) {
            match e {
                1 => (157 - w, y),
                2 => (2, y),
                3 => (x, 0xA5),
                _ => (x, 0x2A),
            }
        };
        let (ex, ey) = self.ahead();
        let mut best: Option<(u32, i32, i32)> = None;
        if !(at_edge(ex, ey) && open_area(landing(ex, ey).0, landing(ex, ey).1) >= 1500) {
            for fallback in [false, true] {
                if best.is_some() || (fallback && !scripted_ok) {
                    break;
                }
                for y in 0..H {
                    for x in 0..W {
                        let d = dist[(y * W + x) as usize];
                        if d == u32::MAX || !at_edge(x, y) {
                            continue;
                        }
                        let (lx, ly) = landing(x, y);
                        if !fallback && open_area(lx, ly) < 1500 {
                            continue;
                        }
                        let off = if e <= 2 { (y - 110).abs() } else { (x - 80).abs() } as u32;
                        let cost = d + 3 * off;
                        if best.is_none_or(|b| cost < b.0) {
                            best = Some((cost, x, y));
                        }
                    }
                }
            }
        } else {
            best = Some((0, ex, ey));
        }
        self.water_room = here_water;
        if std::env::var_os("KQ_HOPS").is_some() {
            eprintln!(
                "  edge {e} in room {} from {:?}: crossing at {best:?} (to room {dest}, w {w})",
                self.v(1),
                self.ego()
            );
        }
        let Some((_, tx, ty)) = best else {
            self.show_map(&dist);
            return Err(format!("no path to edge {e} in room {} from {:?}", self.v(1), self.ego()));
        };
        let room = self.v(1);
        if (tx, ty) != (ex, ey) {
            if let Err(err) = self.walk(&|x, y| x == tx && y == ty, &format!("edge {e} at ({tx}, {ty})")) {
                if self.v(1) == room {
                    return Err(err);
                }
                self.stop()?;
                return self.wait(4);
            }
        }
        let d = [0, 7, 3, 1, 5][e as usize];
        if self.intended_dir() != d {
            self.step(Some(DIR_KEYS[d as usize]))?;
        }
        for _ in 0..40 {
            if self.v(1) != room {
                self.stop()?;
                return self.wait(4);
            }
            self.step(None)?;
        }
        Err(format!("room {room}: walking off edge {e} did not change room"))
    }

    fn room(&mut self, to: u8, goal: Option<[i32; 4]>) -> Result<(), String> {
        self.travelling = true;
        let r = self.room_inner(to, goal);
        self.travelling = false;
        r
    }

    fn room_inner(&mut self, to: u8, goal: Option<[i32; 4]>) -> Result<(), String> {
        let mut blocked: Vec<(u8, u8)> = Vec::new();
        let mut visits: HashMap<u8, u32> = HashMap::new();
        loop {
            let from = self.v(1);
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
                    let n = self.g.exits.get(&r).map_or(0, |x| x[e as usize]);
                    if n != 0 && n != from && !prev.contains_key(&n) && !blocked.contains(&(r, e)) {
                        prev.insert(n, (r, e));
                        q.push_back(n);
                    }
                }
            }
            if !prev.contains_key(&to) {
                return Err(format!("no exit route from room {from} to room {to}"));
            }
            let mut hops = Vec::new();
            let mut r = to;
            while r != from {
                let (p, e) = prev[&r];
                hops.push((p, e));
                r = p;
            }
            hops.reverse();
            if std::env::var_os("KQ_HOPS").is_some() {
                eprintln!("pass {} room {from} at {:?}: hops {hops:?} blocked {blocked:?}", self.r.passes, self.ego());
            }
            let e = hops[0].1;
            let next = hops.get(1).map(|h| h.1);
            let last_goal = if hops.len() == 1 { goal } else { None };
            match self.edge_to(e, next, last_goal) {
                Err(err) if err.starts_with("no path") && self.v(1) == from && blocked.len() < 8 => {
                    blocked.push((from, e))
                }
                r => r.map_err(|err| format!("{err} (heading for room {to})"))?,
            }
        }
    }
}

/// The deadly zones, from the cycle rules whose actions set v4 (water) or lead to the
/// death flag v33 (as the Apple port's kqroute works them out, v32 there).
fn deadly_zones(listings: &HashMap<u8, String>) -> HashMap<u8, Vec<Zone>> {
    let mut out: HashMap<u8, Vec<Zone>> = HashMap::new();
    for (&room, text) in listings {
        let mut actions: HashMap<String, Vec<String>> = HashMap::new();
        let mut rules: Vec<(String, String)> = Vec::new();
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
        let mut deadly: Vec<(u32, u32)> = vec![(33, 1)];
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
        for (cond, target) in &rules {
            if !cond.contains("ego in [") || !action_deadly(target, &deadly) {
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
    out
}

/// Entry rules: "when v18 == FROM ... -> A" whose action starts with `position 0 X Y`.
fn entry_positions(listings: &HashMap<u8, String>) -> HashMap<(u8, u8), Vec<Entry>> {
    let mut out: HashMap<(u8, u8), Vec<Entry>> = HashMap::new();
    for (&room, text) in listings {
        let mut first: HashMap<String, String> = HashMap::new();
        let mut label: Option<String> = None;
        let mut in_actions = false;
        for l in text.lines() {
            if let Some(name) = l.strip_prefix("== ") {
                in_actions = name.starts_with("ACTIONS");
                continue;
            }
            if !in_actions {
                continue;
            }
            if let Some(rest) = l.strip_prefix("  A") {
                label = Some(format!("A{}", rest.trim_end_matches(':')));
            } else if let (Some(lb), Some((_, op))) = (&label, l.trim_start().split_once("  ")) {
                first.entry(lb.clone()).or_insert_with(|| op.trim().to_string());
            }
        }
        for l in text.lines() {
            let Some((cond, target)) = l.split_once("-> ") else { continue };
            let Some(cond) = cond.split_once("when ").map(|c| c.1) else { continue };
            let Some(op) = first.get(target.trim()) else { continue };
            let w: Vec<&str> = op.split_whitespace().collect();
            if w.len() != 4 || w[0] != "position" || w[1] != "0" {
                continue;
            }
            let (Ok(x), Ok(y)) = (w[2].parse::<i32>(), w[3].parse::<i32>()) else { continue };
            let mut from = None;
            let mut conds = Vec::new();
            for c in cond.split("&&") {
                let c = c.trim();
                let (eq, (v, val)) = match c.split_once("!=") {
                    Some(t) => (false, t),
                    None => match c.split_once("==") {
                        Some(t) => (true, t),
                        None => continue,
                    },
                };
                let (Ok(v), Ok(val)) = (v.trim_start_matches('v').parse::<usize>(), val.trim().parse::<u8>()) else {
                    continue;
                };
                match (v, eq) {
                    (18, true) => from = Some(val),
                    (29, _) => {}
                    _ => conds.push((v, eq, val)),
                }
            }
            if let Some(f) = from {
                out.entry((room, f)).or_default().push((conds, (x, y)));
            }
        }
    }
    out
}

fn load_exits() -> HashMap<u8, [u8; 5]> {
    let text = std::fs::read_to_string("routes/room_exits.json").expect("routes/room_exits.json");
    let mut out = HashMap::new();
    for part in text.trim_matches(|c| c == '{' || c == '}').split("}, ") {
        let Some((room, rest)) = part.split_once(": {") else { continue };
        let room: u8 = room.trim_matches('"').parse().unwrap();
        let mut ex = [0u8; 5];
        for kv in rest.trim_end_matches('}').split(", ").filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once(": ").unwrap();
            ex[k.trim_matches('"').parse::<usize>().unwrap()] = v.trim().parse().unwrap();
        }
        out.insert(room, ex);
    }
    out
}

fn rect(nums: &[i32]) -> [i32; 4] {
    [nums[0], nums[1], nums[2], nums[3]]
}

fn run_line(d: &mut Driver, line: &str) -> Result<(), String> {
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let nums: Vec<i32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    match cmd {
        "room" => d.room(nums[0] as u8, (nums.len() == 5).then(|| rect(&nums[1..]))),
        "goto" => d.goto(nums[0], nums[1], *nums.get(2).unwrap_or(&2)),
        "area" => {
            let ix = 2.min((nums[2] - nums[0]) / 2);
            let iy = 2.min((nums[3] - nums[1]) / 2);
            let (x1, y1, x2, y2) = (nums[0] + ix, nums[1] + iy, nums[2] - ix, nums[3] - iy);
            d.walk(&|x, y| x >= x1 && x <= x2 && y >= y1 && y <= y2, &format!("area [{x1},{y1},{x2},{y2}]"))
        }
        "hold" => {
            let dir = match rest.trim() {
                "U" => 1,
                "R" => 3,
                "D" => 5,
                _ => 7,
            };
            if d.intended_dir() == dir {
                d.step(None)
            } else {
                d.step(Some(DIR_KEYS[dir as usize]))
            }
        }
        "jumpuntil" => {
            // jumpuntil V VALUE MAX: keep jumping ("jump" whenever Graham is in his own
            // view, view 0, not in a jump) until vV == VALUE.
            let (v, val, max) = (nums[0] as usize, nums[1] as u8, nums[2] as u64);
            let start = d.r.passes;
            while d.r.passes < start + max {
                if d.v(v) == val {
                    return Ok(());
                }
                if d.r.ds(0x11E1 + 5) == 0 {
                    d.say("jump")?;
                } else {
                    d.step(None)?;
                }
            }
            Err(format!("v{v} did not become {val} within {max} passes of jumping"))
        }
        "final" => {
            let r = rect(&nums);
            d.ignore_deadly = true;
            d.wade = true;
            let res = d.walk(&|x, y| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3], "the final area");
            d.ignore_deadly = false;
            d.wade = false;
            res.or_else(|e| if d.dead() { Ok(()) } else { Err(e) })
        }
        "map" => {
            // The planner's view of the room (stderr), as KQ_MAP shows it on a failure.
            // `map Y`: row Y's priorities, every column, instead.
            if let Some(&y) = nums.first() {
                let row: String = (0..W as usize).map(|x| format!("{:X}", d.r.priority(x, y as usize))).collect();
                eprintln!("{y:3} {row}  ego min width {}, widths {:?}", d.ego_min_width(), d.dir_widths());
                d.narrow.set(d.ego_min_width());
                let ok: String = (0..W).map(|x| if d.standable(x, y, 6) { '.' } else { '#' }).collect();
                eprintln!("    {ok}  water room {} wade {}", d.water_room, d.wade);
                return Ok(());
            }
            std::env::set_var("KQ_MAP", "1");
            d.show_map(&d.distances());
            std::env::remove_var("KQ_MAP");
            Ok(())
        }
        "keepin" => {
            let rects: Vec<[i32; 4]> = nums.chunks(4).filter(|c| c.len() == 4).map(rect).collect();
            d.keep_in = if rects.is_empty() { None } else { Some((d.v(1), rects)) };
            Ok(())
        }
        "require" => {
            let (v, val) = (nums[0] as usize, nums[1] as u8);
            if d.v(v) == val {
                Ok(())
            } else {
                Err(format!("required v{v} == {val}, found {}", d.v(v)))
            }
        }
        "enter" => {
            let (to, r) = (nums[0] as u8, rect(&nums[1..]));
            let ix = 2.min((r[2] - r[0]) / 2);
            let iy = 2.min((r[3] - r[1]) / 2);
            let (x1, y1, x2, y2) = (r[0] + ix, r[1] + iy, r[2] - ix, r[3] - iy);
            d.wade = rest.trim_end().ends_with("water");
            let res = d.walk(&|x, y| x >= x1 && x <= x2 && y >= y1 && y <= y2, "the doorway");
            d.wade = false;
            for _ in 0..40 {
                if d.v(1) == to {
                    d.stop()?;
                    return d.wait(4);
                }
                d.step(None)?;
            }
            res.and(Err(format!("did not enter room {to} (in room {})", d.v(1))))
        }
        "edge" => {
            let e = match rest.trim() {
                "L" => 1,
                "R" => 2,
                "U" => 3,
                _ => 4,
            };
            d.edge_to(e, None, None)
        }
        "dies" => {
            // dies MAX: let passes go by until Graham dies; then 100 more, for the death
            // sequence (its message, input switched off).
            let max = *nums.first().unwrap_or(&500) as usize;
            for _ in 0..max {
                if d.dead() {
                    d.mourning = true;
                    let r = d.wait(100);
                    d.mourning = false;
                    return r;
                }
                if let Err(e) = d.step(None) {
                    if !d.dead() {
                        return Err(e);
                    }
                }
            }
            Err(format!("Graham did not die within {max} passes"))
        }
        "probe" => {
            let (x, y) = d.ego();
            let a = 0x11E1;
            let row: Vec<String> =
                (x - 2..x + 10).map(|i| format!("{:X}", d.r.priority(i as usize, y as usize))).collect();
            eprintln!(
                "probe: ego ({x}, {y}) flags {:02X} pri {:02X} cel {:?} widths {:?} v4 {} under {}",
                d.r.ds(a + 0x0B),
                d.r.ds(a + 0x1D),
                d.cel(0),
                d.dir_widths(),
                d.v(4),
                row.join("")
            );
            Ok(())
        }
        "if" => {
            // if V VALUE STEP: the step only when vV == VALUE.
            let mut w = rest.splitn(3, ' ');
            let (v, val) = (w.next().and_then(|x| x.parse::<usize>().ok()), w.next().and_then(|x| x.parse::<u8>().ok()));
            let (Some(v), Some(val), Some(then)) = (v, val, w.next()) else { return Err("if V VALUE STEP".into()) };
            if d.v(v) == val {
                run_line(d, then)
            } else {
                Ok(())
            }
        }
        "say" => d.say(rest.trim()),
        "prompt" => {
            d.prompt = rest.trim().bytes().map(ascii).chain([ascii(b'\r')]).collect();
            Ok(())
        }
        "key" => d.step(Some(u16::from_str_radix(rest.trim(), 16).map_err(|e| e.to_string())?)),
        "wait" => d.wait(nums[0] as usize),
        "waitvar" => {
            let (v, val, max) = (nums[0] as usize, nums[1] as u8, *nums.get(2).unwrap_or(&500) as usize);
            for _ in 0..max {
                if d.v(v) == val {
                    return Ok(());
                }
                d.step(None)?;
            }
            Err(format!("v{v} did not become {val} within {max} passes"))
        }
        "expect" => {
            let (what, v) = rest.split_once(' ').ok_or("expect what?")?;
            let v: u8 = v.trim().parse().map_err(|_| "expect a number")?;
            let actual = if what == "score" { d.v(19) } else { d.v(1) };
            if actual == v {
                Ok(())
            } else {
                Err(format!("expected {what} {v}, found {actual}"))
            }
        }
        _ => Err(format!("unknown step {cmd}")),
    }
}

/// Chance went against the line: a death, the dwarf, a condition set by chance, or a
/// crossing spoiled (a random visitor on entry, such as the witch in room 46, who
/// carries Graham off).
/// Out of the current room to a neighbouring one and back in.
fn bounce(d: &mut Driver) -> Result<(), String> {
    let here = d.v(1);
    let exits = d.g.exits.get(&here).copied().unwrap_or_default();
    for &n in exits.iter().filter(|&&n| n != 0) {
        let mut t = d.clone();
        if t.room(n, None).is_ok() && !t.dead() && t.room(here, None).is_ok() && !t.dead() {
            *d = t;
            return Ok(());
        }
    }
    Err(format!("no way out of room {here} and back"))
}

/// Line `i` again from the snapshot before it after 1-30 passes of waiting (`back` 0),
/// or the line before it too, after 0-20 passes, then the line after 0-30 (`back` 1).
fn retry_waits(snaps: &mut Vec<Driver>, lines: &[(usize, String)], i: usize, back: usize) -> Option<Driver> {
    let line = &lines[i].1;
    for extra_prev in 0..=(if back == 1 { 20 } else { 0 }) {
        for extra in 0..=30usize {
            if back == 0 && extra == 0 {
                continue;
            }
            let mut t = snaps[i - back].clone();
            let mut ok = true;
            if back == 1 {
                let (_, pline) = &lines[i - 1];
                ok = t.wait(extra_prev).and_then(|_| run_line(&mut t, pline)).is_ok();
            }
            if ok && t.wait(extra).and_then(|_| run_line(&mut t, line)).is_ok() {
                if back == 1 {
                    snaps.truncate(i);
                    snaps.push(t.clone());
                }
                return Some(t);
            }
        }
    }
    None
}

fn retryable(d: &Driver, e: &str) -> bool {
    d.dead()
        || e.contains("robbed")
        || e.starts_with("required")
        || e.contains("going round in circles")
        || e.contains("no path to edge")
}

/// Play the route; each line from a snapshot, retried with 1-30 extra passes of waiting
/// (and then with the line before retried too) when chance goes against it.
fn play(d0: Driver, route: &str) -> (Driver, Option<(usize, String)>) {
    let mut d = d0;
    for _ in 0..30 {
        if d.v(25) != 0 && d.r.ds(0x11E1) == d.v(1) {
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
        snaps.push(d.clone());
        d.events.push(format!("{:4}: {line}   [pass {}, room {}, score {}]", n + 1, d.r.passes, d.v(1), d.v(19)));
        let Err(e) = run_line(&mut d, line) else {
            i += 1;
            continue;
        };
        if !retryable(&d, &e) {
            return (d, Some((*n, e)));
        }
        // Waiting 1-30 passes first (that keeps the random draws as they were), then
        // round trips, then the line before again with waits.
        let mut fixed = false;
        if let Some(t) = retry_waits(&mut snaps, &lines, i, 0) {
            d = t;
            fixed = true;
        }
        if !fixed {
            // The random bit v14 comes from the next byte of the engine's code at each room
            // change, whatever the timing: out to a neighbouring room and back (an even number
            // of times first: some rooms count their entries; up to 11), from this line or up to five
            // before it (a doorway's room can't be left and re-entered by its exits), and
            // the lines from there played again.
            'trips: for back in 0..=5usize.min(i) {
                    for k in [2, 4, 6, 8, 10, 1, 3, 5, 7, 9, 11] {
                    let from = i - back;
                    let mut t = snaps[from].clone();
                    let mut redone = Vec::new();
                    let mut r = (0..k).try_for_each(|_| bounce(&mut t));
                    for (j, (m, l)) in lines.iter().enumerate().take(i + 1).skip(from) {
                        if r.is_err() {
                            break;
                        }
                        if j > from {
                            redone.push(t.clone());
                        }
                        let at = format!("{:4}: {l}   [pass {}, room {}, score {}]", m + 1, t.r.passes, t.v(1), t.v(19));
                        t.events.push(at);
                        r = run_line(&mut t, l);
                    }
                    if std::env::var_os("KQ_RETRY").is_some() {
                        eprintln!("line {}: {k} round trips from line {}: {r:?}", n + 1, lines[from].0 + 1);
                    }
                    if r.is_ok() {
                        t.events.push(format!("      (after {k} round trips to a neighbouring room from line {})", lines[from].0 + 1));
                        snaps.truncate(from + 1);
                        snaps.extend(redone);
                        d = t;
                        fixed = true;
                        break 'trips;
                    }
                }
            }
        }
        if !fixed && i >= 1 {
            if let Some(t) = retry_waits(&mut snaps, &lines, i, 1) {
                d = t;
                fixed = true;
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let events_out = opt("--events");
    let trace_out = opt("--trace");
    let keys_out = opt("--keys");
    if args.len() != 2 {
        eprintln!("usage: kqroute-pcjr DISK ROUTE [--events OUT] [--trace OUT] [--keys OUT]");
        std::process::exit(2);
    }
    let raw = std::fs::read(&args[0]).unwrap_or_else(|e| panic!("{}: {e}", args[0]));
    let disk = Disk::from_file(raw.clone()).expect("the game disk");
    let listings: HashMap<u8, String> =
        (1..=84u8).filter_map(|r| kq1_pcjr::script::listing(&disk, r as usize).map(|t| (r, t))).collect();
    let water_rooms = listings.iter().filter(|(_, t)| t.contains("v4==1")).map(|(&r, _)| r).collect();
    let cs = disk.code_segment();
    let ds = &cs[kq1_pcjr::vocab::DATA_SEGMENT..];
    let g = Game {
        exits: load_exits(),
        water_rooms,
        deadly: deadly_zones(&listings),
        entries: entry_positions(&listings),
        loops2: ds[0x174F..0x174F + 9].try_into().unwrap(),
        loops4: ds[0x1758..0x1758 + 9].try_into().unwrap(),
        frame: ds[kq1_pcjr::pic::FRAME..kq1_pcjr::pic::FRAME + 11].to_vec(),
        disk,
    };
    let route = std::fs::read_to_string(&args[1]).expect("the route");
    let d = Driver {
        r: Runner::new(&raw).expect("refemu"),
        g: Rc::new(g),
        water_room: true,
        travelling: false,
        events: Vec::new(),
        keep_in: None,
        ignore_deadly: false,
        prompt: VecDeque::new(),
        keys: Vec::new(),
        narrow: std::cell::Cell::new(7),
        wade: false,
        mourning: false,
    };
    let (d, failed) = play(d, &route);
    for e in &d.events {
        println!("{e}");
    }
    println!("end: pass {}, room {}, score {}, ego {:?}", d.r.passes, d.v(1), d.v(19), d.ego());
    if let Ok(v) = std::env::var("KQ_VARS") {
        let vals: Vec<String> =
            v.split(',').filter_map(|x| x.parse::<usize>().ok()).map(|i| format!("v{i}={}", d.v(i))).collect();
        println!("vars: {}", vals.join(" "));
    }
    if let Some(p) = events_out {
        std::fs::write(p, d.r.events()).expect("write events");
    }
    if let Some(p) = trace_out {
        std::fs::write(p, &d.r.trace).expect("write trace");
    }
    if let Some(p) = keys_out {
        let text: String = d.keys.iter().map(|(p, k)| format!("{p} {k:04X}\n")).collect();
        std::fs::write(p, text).expect("write keys");
    }
    if let Some((n, e)) = failed {
        eprintln!("route stopped: line {}: {e}", n + 1);
        std::process::exit(1);
    }
}
