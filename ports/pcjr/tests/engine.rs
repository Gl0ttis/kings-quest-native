//! The engine against the original, pass by pass: each reference route's event log
//! (refemu-pcjr `--events`: the timer ticks and keys, against the checkpoints) is
//! replayed in the port, and every pass's state must equal the original's trace
//! (refemu-pcjr `--trace`). Skipped without the original's disk in original/.

use kq1_pcjr::disk::Disk;
use kq1_pcjr::engine::clock::Replay;
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
    let mut w = World::new(disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = reference.len() as u64;
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
