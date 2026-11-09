//! The engine against the original, pass by pass: each reference route's event log
//! (refemu-pcjr `--events`: the timer ticks and keys, against the checkpoints) is
//! replayed in the port, and every pass's state must equal the original's trace
//! (refemu-pcjr `--trace`), and its screen the original's (kqframes-pcjr `--hashes`).
//! Skipped without the original's disk in original/.

use kq1_pcjr::disk::Disk;
use kq1_pcjr::engine::clock::{Clock, Event, Joystick, Replay, CPU_HZ, CYCLES_PER_TICK};
use kq1_pcjr::engine::World;
use std::path::Path;

fn original() -> Option<Disk> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    std::fs::read_dir(dir).ok()?.find_map(|e| {
        let p = e.ok()?.path();
        (p.extension()?.to_str()? == "mfm").then_some(())?;
        Disk::from_file(std::fs::read(&p).ok()?).ok()
    })
}

/// Replay route `name` and compare every pass; the number compared.
fn replay(disk: &Disk, name: &str) -> usize {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference");
    let events = std::fs::read_to_string(dir.join(format!("{name}.events"))).expect("the event log");
    let trace = std::fs::read_to_string(dir.join(format!("{name}.csv"))).expect("the trace");
    let reference: Vec<Vec<&str>> = trace.lines().skip(1).map(|l| l.split(',').collect()).collect();
    // The screen at each pass (refemu's kqframes-pcjr --hashes), where recorded.
    let frames: Option<Vec<u64>> = std::fs::read_to_string(dir.join(format!("{name}.frames")))
        .ok()
        .map(|t| t.lines().map(|l| u64::from_str_radix(l, 16).expect("a frame hash")).collect());
    let mut w = World::new(disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = reference.len() as u64;
    w.frame_hashes = frames.is_some().then(Vec::new);
    let why = w.run();
    assert!(w.trace.len() >= reference.len(), "{name}: stopped after {} passes: {why}", w.trace.len());
    for (row, r) in w.trace.iter().zip(&reference) {
        let mine = [
            row.room.to_string(),
            row.ego_x.to_string(),
            row.ego_y.to_string(),
            row.ego_dir.to_string(),
            row.score.to_string(),
            format!("{:016x}", row.vars_hash),
        ];
        assert_eq!(mine.as_slice(), &r[3..9], "{name}: pass {} differs", row.pass);
    }
    if let (Some(orig), Some(port)) = (&frames, &w.frame_hashes) {
        assert_eq!(orig.len(), reference.len(), "{name}: a frame hash per pass");
        for (k, (a, b)) in orig.iter().zip(port).enumerate() {
            assert_eq!(a, b, "{name}: the screen at pass {} differs (kqframes-pcjr shows how)", k + 1);
        }
    }
    reference.len()
}

#[test]
fn the_walk_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "walk_rooms_1_16"), 900);
}

#[test]
fn the_save_restore_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "save_restore"), 1000);
}

/// The death routes (routes/death_*.route, played by kqroute-pcjr): the river, the rock
/// and the stream, each with 100 passes after the death.
#[test]
fn the_death_routes_match_the_original() {
    let Some(disk) = original() else { return };
    for (name, passes) in [("death_river", 463), ("death_rock", 454), ("death_stream", 569)] {
        assert_eq!(replay(&disk, name), passes, "{name}");
    }
}

/// The gold key route (routes/gold_key.route, played by kqroute-pcjr): from the start to
/// the fall from the clouds, through most of the game's scenes and 21744 passes.
#[test]
fn the_gold_key_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "gold_key"), 21744);
}

/// The maximum-score route (routes/max_score.route, played by kqroute-pcjr): the whole
/// game, crowned with 158 points, 21368 passes.
#[test]
fn the_max_score_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "max_score"), 21368);
}

/// The magic-ring route (routes/magic_ring.route): the max-score route with the ring
/// put on and rubbed (invisible until the spell runs out, -3), ending at 155; 22462
/// passes.
#[test]
fn the_magic_ring_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "magic_ring"), 22462);
}

/// The giant-with-the-sling route (routes/giant_sling.route): the giant killed with the
/// sling instead of left to sleep, ending at 153; 20463 passes.
#[test]
fn the_giant_sling_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "giant_sling"), 20463);
}

/// A clock for play without pacing: the timer's ticks by the game's time, keys at given
/// times, and a joystick: at rest, then pushed right from a given time.
struct StickClock {
    ticks: u64,
    keys: Vec<(u64, u16)>,
    push_at: u64,
    cycles: u64,
}

impl Clock for StickClock {
    fn at(&mut self, _n: u64, cycles: u64) -> Vec<Event> {
        self.cycles = cycles;
        let mut out = Vec::new();
        while self.keys.first().is_some_and(|&(t, _)| t <= cycles) {
            out.push(Event::Key(self.keys.remove(0).1));
        }
        while (self.ticks + 1) * CYCLES_PER_TICK <= cycles {
            self.ticks += 1;
            out.push(Event::Tick(0xFFFF));
        }
        out
    }
    fn waiting(&mut self, n: u64, _cycles: u64) -> Vec<Event> {
        self.at(n, (self.ticks + 1) * CYCLES_PER_TICK)
    }
    fn poll(&mut self, cycles: u64) -> Vec<Event> {
        self.at(0, cycles)
    }
    fn joystick(&mut self) -> Option<Joystick> {
        let x = if self.cycles >= self.push_at { 1.0 } else { 0.5 };
        Some(Joystick::from_position(x, 0.5, [false; 2]))
    }
}

/// Past the title and the credits (Enter at 4, 8, 12 and 16 s) into the first room,
/// then right from 22 s, by the stick or the keypad's right arrow (or not at all): the
/// ego's x at 22 s and at 30 s.
fn ego_walk(disk: &Disk, stick: bool, arrow: bool) -> (u8, u8) {
    use std::{cell::Cell, rc::Rc};
    let s = |t: f64| (t * CPU_HZ) as u64;
    let mut keys: Vec<(u64, u16)> = [4.0, 8.0, 12.0, 16.0].map(|t| (s(t), 0x1C0D)).to_vec();
    if arrow {
        keys.push((s(22.0), 0x4D00));
    }
    let clock = StickClock { ticks: 0, keys, push_at: if stick { s(22.0) } else { u64::MAX }, cycles: 0 };
    let mut w = World::new(disk, Box::new(clock)).expect("engine");
    let at22 = Rc::new(Cell::new(None));
    let seen = at22.clone();
    w.on_frame = Some(Box::new(move |w: &mut World| {
        if w.cycles >= s(22.0) && seen.get().is_none() {
            seen.set(Some(w.actors[0].x));
        }
        if w.cycles >= s(30.0) {
            w.stop("30 s".into());
        }
    }));
    w.run();
    assert_eq!(w.vars[1], 1, "in the first room");
    (at22.get().expect("22 s reached"), w.actors[0].x)
}

/// The joystick (none in refemu-pcjr's PCjr): at rest it leaves Graham standing, and
/// pushed right it walks him right as far as the keypad's right arrow does (to the
/// moat's bank under the tree).
#[test]
fn the_joystick_walks_graham() {
    let Some(disk) = original() else { return };
    let (a, b) = ego_walk(&disk, false, false);
    assert_eq!(a, b, "the stick at rest");
    let (_, by_arrow) = ego_walk(&disk, false, true);
    assert!(by_arrow > a, "the arrow: x {a} to {by_arrow}");
    let (a2, by_stick) = ego_walk(&disk, true, false);
    assert_eq!((a2, by_stick), (a, by_arrow), "the stick");
}
