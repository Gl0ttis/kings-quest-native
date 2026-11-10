//! The engine against the original, pass by pass: each reference route's event log
//! (refemu-pc `--events`: the timer ticks and keys, against the checkpoints) is
//! replayed in the port, and every pass's state must equal the original's trace
//! (refemu-pc `--trace`), and its screen the original's (kqframes-pc `--hashes`).
//! Skipped without the original's disk in original/.

use kq1_pc::disk::{Disk, Edition};
use kq1_pc::engine::clock::{Clock, Event, Joystick, Replay, CPU_HZ, CYCLES_PER_TICK};
use kq1_pc::engine::World;
use std::path::Path;

/// The first release's original (reference/ holds its runs).
fn original() -> Option<Disk> {
    original_of(Edition::Cga)
}

/// An edition's original (the second's runs are in reference/cga_rgb/).
fn original_of(edition: Edition) -> Option<Disk> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    std::fs::read_dir(dir).ok()?.find_map(|e| {
        let p = e.ok()?.path();
        (p.extension()?.to_str()? == "mfm").then_some(())?;
        Disk::from_file(std::fs::read(&p).ok()?).ok().filter(|d| d.edition == edition)
    })
}

/// The user's BIOS image (original/bios.rom: the IBM PC's, for its font), if there.
fn bios() -> Option<Vec<u8>> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("original").join("bios.rom")).ok()
}

/// Replay route `name` and compare every pass; the number compared.
fn replay(disk: &Disk, name: &str) -> usize {
    replay_with(disk, name, None).0
}

/// The same, with `play` as the Play diskette (else a blank one); and the game after.
fn replay_with(disk: &Disk, name: &str, play: Option<Vec<u8>>) -> (usize, World) {
    replay_in(disk, "reference", name, play)
}

/// Replay route `name` of the runs in `folder` (reference/, or reference/cga_rgb/ for
/// the second edition's).
fn replay_in(disk: &Disk, folder: &str, name: &str, play: Option<Vec<u8>>) -> (usize, World) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(folder);
    let events = std::fs::read_to_string(dir.join(format!("{name}.events"))).expect("the event log");
    let trace = std::fs::read_to_string(dir.join(format!("{name}.csv"))).expect("the trace");
    let reference: Vec<Vec<&str>> = trace.lines().skip(1).map(|l| l.split(',').collect()).collect();
    // The screen at each pass (refemu's kqframes-pc --hashes), where recorded.
    // The screens were recorded with the BIOS's font (refemu-pc, bios.rom next to the
    // disk); without the player's own BIOS image in original/ they are left out.
    let font = bios().map(|b| std::rc::Rc::new(kq1_pc::font::Font::from_bios(&b).expect("bios.rom")));
    let frames: Option<Vec<u64>> = std::fs::read_to_string(dir.join(format!("{name}.frames")))
        .ok()
        .filter(|_| font.is_some())
        .map(|t| t.lines().map(|l| u64::from_str_radix(l, 16).expect("a frame hash")).collect());
    let mut w = World::new(disk, Box::new(Replay::parse(&events))).expect("engine");
    w.font = font;
    if let Some(img) = play {
        w.disks[1].image = img;
    }
    // With a front end watching, as the player's: pictures are then shown as they are
    // drawn (their work charged as it goes), which mustn't change the game.
    w.on_frame = Some(Box::new(|_: &mut World| {}));
    w.max_passes = reference.len() as u64;
    w.frame_hashes = frames.is_some().then(Vec::new);
    let why = w.run();
    // The passes the port ran, compared first: where it stopped short, the first pass
    // that differs says more than the stop.
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
    assert!(w.trace.len() >= reference.len(), "{name}: stopped after {} passes: {why}", w.trace.len());
    if let (Some(orig), Some(port)) = (&frames, &w.frame_hashes) {
        assert_eq!(orig.len(), reference.len(), "{name}: a frame hash per pass");
        for (k, (a, b)) in orig.iter().zip(port).enumerate() {
            assert_eq!(a, b, "{name}: the screen at pass {} differs (kqframes-pc shows how)", k + 1);
        }
    }
    // The speaker's events, in order, where recorded (refemu-pc --speaker).
    if let Ok(t) = std::fs::read_to_string(dir.join(format!("{name}.speaker"))) {
        let orig: Vec<&str> = t.lines().collect();
        let port: Vec<String> = w.speaker.iter().map(|s| s.log_line()).collect();
        let n = orig.len().min(port.len());
        if let Some(k) = (0..n).find(|&k| orig[k] != port[k]) {
            panic!("{name}: the speaker's event {k} differs: original {}, port {}", orig[k], port[k]);
        }
        assert!(
            port.len() >= orig.len(),
            "{name}: the port's speaker stops after {} of {} events",
            port.len(),
            orig.len()
        );
    }
    (reference.len(), w)
}

#[test]
fn the_walk_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "walk_rooms_1_16"), 781);
}

/// The second edition (CGA+RGB): its runs (reference/cga_rgb/, recorded from its
/// original, in composite colour), each compared on every pass as the first's are.
fn second(name: &str, play: Option<Vec<u8>>) -> Option<(usize, World)> {
    let disk = original_of(Edition::CgaRgb)?;
    Some(replay_in(&disk, "reference/cga_rgb", name, play))
}

#[test]
fn the_second_editions_walk_matches_its_original() {
    let Some((n, _)) = second("walk_rooms_1_16", None) else { return };
    assert_eq!(n, 781);
}

/// The second edition's save is its original's, byte for byte (the block's hash, at
/// its slot A: sector 6), and a fresh start restores it as its original did.
#[test]
fn the_second_editions_play_diskette_is_its_originals() {
    use kq1_pc::engine::frame_hash;
    let Some((n, w)) = second("save_restore", None) else { return };
    assert_eq!(n, 319);
    let play = w.disks[1].image.clone();
    assert_eq!(frame_hash(&play[6 * 512..6 * 512 + 1306]), 0x1a79_b76c_de2e_6bec, "the save block differs");
    assert_eq!(second("restore_play", Some(play)).unwrap().0, 155);
}

/// The second edition in RGB colour (chosen at its first screen): the walk and a death,
/// against its original in RGB. The game differs from the composite runs (v5 holds
/// the RGB patterns), and the screens are the RGB patterns.
#[test]
fn the_second_editions_rgb_runs_match_its_original() {
    for (name, passes) in [("rgb_walk_rooms_1_16", 781), ("rgb_death_river", 429)] {
        let Some((n, _)) = second(name, None) else { return };
        assert_eq!(n, passes, "{name}");
    }
}

#[test]
fn the_second_editions_deaths_match_its_original() {
    for (name, passes) in [("death_river", 429), ("death_rock", 438), ("death_stream", 565)] {
        let Some((n, _)) = second(name, None) else { return };
        assert_eq!(n, passes, "{name}");
    }
}

#[test]
fn the_second_editions_whole_games_match_its_original() {
    for (name, passes) in [("gold_key", 16757), ("max_score", 20197), ("magic_ring", 21262), ("giant_sling", 19268)] {
        let Some((n, _)) = second(name, None) else { return };
        assert_eq!(n, passes, "{name}");
    }
}

#[test]
fn the_save_restore_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "save_restore"), 319);
}

/// The Play diskette is the original's: the save the port writes in the save/restore
/// route is the original's, byte for byte (its hash; the block holds game data, so only
/// the hash is kept), view and cel pointers included, which the original's restore takes
/// as they are. And on a fresh start the game restores it as the original did
/// (routes/restore_play.plan, recorded with the original's own diskette from that run;
/// the PC's slot A is 3 sectors from 3Fh, compared whole).
/// The hash of the original's slot A block in this route (refemu-pc's Play diskette).
const BLOCK: u64 = 0x668e_f7c2_6f70_9057;

#[test]
fn the_play_diskette_is_the_originals() {
    use kq1_pc::engine::frame_hash;
    let Some(disk) = original() else { return };
    let (_, w) = replay_with(&disk, "save_restore", None);
    let play = w.disks[1].image.clone();
    // Slot A: 3 sectors from 3Fh (the PC's load_slot, `$0867`), of which the block
    // (1,306 bytes) is read back; the rest of the sectors is whatever was in memory after
    // it, which the game never reads.
    let block = &play[0x3F * 512..0x3F * 512 + 1306];
    assert_eq!(frame_hash(block), BLOCK, "the save block differs from the original's");
    assert_eq!(replay_with(&disk, "restore_play", Some(play)).0, 155);
}

/// The death routes (routes/death_*.route, played by kqroute-pc): the river, the rock
/// and the stream, each with 100 passes after the death.
#[test]
fn the_death_routes_match_the_original() {
    let Some(disk) = original() else { return };
    for (name, passes) in [("death_river", 428), ("death_rock", 438), ("death_stream", 569)] {
        assert_eq!(replay(&disk, name), passes, "{name}");
    }
}

/// The gold key route (routes/gold_key.route, played by kqroute-pc): from the start to
/// the fall from the clouds, through most of the game's scenes and 17151 passes.
#[test]
fn the_gold_key_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "gold_key"), 17151);
}

/// The maximum-score route (routes/max_score.route, played by kqroute-pc): the whole
/// game, crowned with 158 points, 20501 passes.
#[test]
fn the_max_score_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "max_score"), 20501);
}

/// The magic-ring route (routes/magic_ring.route): the max-score route with the ring
/// put on and rubbed (invisible until the spell runs out, -3), ending at 155; 21568
/// passes.
#[test]
fn the_magic_ring_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "magic_ring"), 21568);
}

/// The giant-with-the-sling route (routes/giant_sling.route): the giant killed with the
/// sling instead of left to sleep, ending at 153; 19550 passes.
#[test]
fn the_giant_sling_route_matches_the_original() {
    let Some(disk) = original() else { return };
    assert_eq!(replay(&disk, "giant_sling"), 19550);
}

/// The speed model: the walk route replayed, its whole time by the model (each pass's
/// work by the counts, and its waits to their ticks) within 1% of the original's in
/// refemu-pc (examples/speedcmp.rs for the details).
#[test]
fn the_walk_takes_the_originals_time() {
    let Some(disk) = original() else { return };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference");
    let events = std::fs::read_to_string(dir.join("walk_rooms_1_16.events")).expect("the event log");
    let trace = std::fs::read_to_string(dir.join("walk_rooms_1_16.csv")).expect("the trace");
    let orig: Vec<u64> = trace.lines().skip(1).map(|l| l.split(',').nth(1).unwrap().parse().unwrap()).collect();
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = orig.len() as u64;
    w.run();
    let last = w.trace.len().min(orig.len()) - 1;
    let (mine, theirs) = (w.trace[last].cycles - w.trace[1].cycles, orig[last] - orig[1]);
    let off = mine as f64 / theirs as f64 - 1.0;
    assert!(
        off.abs() < 0.01,
        "{:.1} s by the model, {:.1} s in the original",
        mine as f64 / CPU_HZ,
        theirs as f64 / CPU_HZ
    );
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

/// The joystick (none in refemu-pc's PCjr): at rest it leaves Graham standing, and
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

/// The title's credits (no key pressed), from the original's code (`$4F36`): every line
/// in colour 0Fh (the headings' 10h stands for DS:1167, 0Fh), XORed onto the picture
/// (attribute bit 7) as the BIOS does. With the BIOS's font, the frame has the glyph's
/// dots XORed onto the picture's bytes there.
#[test]
fn the_credits_are_xored_onto_the_picture() {
    use kq1_pc::engine::screen::WIDTH;
    let Some(disk) = original() else { return };
    let clock = StickClock { ticks: 0, keys: Vec::new(), push_at: u64::MAX, cycles: 0 };
    let mut w = World::new(&disk, Box::new(clock)).expect("engine");
    let font = bios().map(|b| std::rc::Rc::new(kq1_pc::font::Font::from_bios(&b).expect("bios.rom")));
    w.font = font.clone();
    w.on_frame = Some(Box::new(|w: &mut World| {
        if w.cycles >= (14.0 * CPU_HZ) as u64 {
            w.stop("14 s".into());
        }
    }));
    w.run();
    let g = w.text.grid();
    assert_eq!(w.text.row(9).trim(), "Roberta Williams");
    assert_eq!(g.attrs[9][14], 0x8F, "a name");
    assert_eq!(g.attrs[7][13], 0x8F, "a heading");
    let Some(font) = font else { return };
    // The cell of Roberta's R (row 9, column 14): the picture's dot patterns there, the
    // glyph's dots in colour 3 XORed onto them.
    let f = w.frame();
    for line in 0..8 {
        let y = 9 * 8 + line;
        let p = |x: usize| kq1_pc::composite::PATTERN[(w.screen.shown[y * WIDTH + x] & 15) as usize];
        let picture = [p(56) << 4 | p(57), p(58) << 4 | p(59)];
        let glyph = kq1_pc::font::expand(font.row(b'R', line), 3);
        let at = 1 + y * 80 + 14 * 2;
        assert_eq!([f[at], f[at + 1]], [picture[0] ^ glyph[0], picture[1] ^ glyph[1]], "line {line}");
    }
}
