//! Compares the port with traces recorded from the original game running in refemu
//! (refemu/README.md), pass by pass: room, ego position and direction, and a hash of all
//! variables v0-v222. Each case is skipped unless its trace and the decoded disk images
//! are present.
//!
//! A trace is recorded from the repository root with, for example:
//!   refemu/target/release/refemu original/disk1.nib original/disk2.nib original/disk3.nib \
//!     --plan "w5,D,w70" --passes 90 --trace extracted/reference/moat_fall.csv
//! The refemu plan "wN, X" corresponds to the port running N passes, then pressing X.
use kq1_apple2::engine::{f, Resources, Scripted, World};
use kq1_apple2::kq::Disks;
use std::fs;

/// `plan`: (direction to press, 0 = none / stop the current one, passes to run).
fn compare(trace_file: &str, plan: &[(u8, usize)]) {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/");
    let Ok(trace) = fs::read_to_string(format!("{base}reference/{trace_file}")) else { return };
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{base}disk{n}.do")).ok()).collect();
    let Some(imgs) = imgs else { return };
    let mut w = World::new(Resources::load(&Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();

    let lines: Vec<Vec<String>> = trace.lines().skip(1).map(|l| l.split(',').map(String::from).collect()).collect();
    let mut pass = 0;
    for &(dir, n) in plan {
        if dir != 0 {
            w.press_direction(dir);
        } else if w.g.key_dir != 0 {
            let d = w.g.key_dir;
            w.press_direction(d); // pressing the current direction stops
        }
        for _ in 0..n {
            w.cycle();
            pass += 1;
            // refemu samples at the start of a pass, the port at the end:
            // original pass k+1 corresponds to port pass k.
            let Some(o) = lines.get(pass) else { return };
            let hash =
                w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
            let port = (
                w.vars[1].to_string(),
                w.actors[0][f::X].to_string(),
                w.actors[0][f::Y].to_string(),
                w.actors[0][f::DIR].to_string(),
                format!("{hash:016x}"),
            );
            let orig = (o[3].clone(), o[4].clone(), o[5].clone(), o[6].clone(), o[8].clone());
            assert_eq!(port, orig, "{trace_file}, pass {pass}: (room, x, y, dir, vars hash) differ from the original");
        }
    }
}

/// Walk up into room 16, right, down, back into room 1 (two room changes).
#[test]
fn walk_through_rooms_1_and_16() {
    compare("walk_rooms_1_16.csv", &[(0, 5), (1, 120), (3, 100), (5, 70), (0, 5)]);
}

/// Walk down into the castle moat: water check, the drowning view (with the sprite
/// re-fit), the death message and the end of the game.
#[test]
fn fall_into_the_moat() {
    compare("moat_fall.csv", &[(0, 5), (5, 80)]);
}

/// Die in the moat, press 5 (restart) and answer Y, then walk up into room 16 and right
/// into room 9: where Graham starts after a death and a restart, and that the restarted
/// game runs on identically. Recorded with the refemu plan
/// "w5,D,w150,k35,k79,w60,U,w100,R,w60".
#[test]
fn restart_after_death() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/");
    let Ok(trace) = fs::read_to_string(format!("{base}reference/death_restart.csv")) else { return };
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{base}disk{n}.do")).ok()).collect();
    let Some(imgs) = imgs else { return };
    let mut w = World::new(Resources::load(&Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let lines: Vec<Vec<String>> = trace.lines().skip(1).map(|l| l.split(',').map(String::from).collect()).collect();
    let mut pass = 0;
    for (key, n) in [(None, 5), (Some(0x0A), 150), (Some(b'5'), 60), (Some(0x0B), 100), (Some(0x15), 60)] {
        if let Some(k) = key {
            if k == b'5' {
                // The Y for "Do you want to start over?" (earlier waits got RETURN).
                w.io = Some(Box::new(Scripted { keys: b"y".iter().copied().collect(), ..Default::default() }));
            }
            w.key(k);
        }
        for _ in 0..n {
            w.cycle();
            pass += 1;
            let Some(o) = lines.get(pass) else { return };
            let hash =
                w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
            let port = (
                w.vars[1].to_string(),
                w.actors[0][f::X].to_string(),
                w.actors[0][f::Y].to_string(),
                w.actors[0][f::DIR].to_string(),
                format!("{hash:016x}"),
            );
            let orig = (o[3].clone(), o[4].clone(), o[5].clone(), o[6].clone(), o[8].clone());
            assert_eq!(
                port, orig,
                "death_restart.csv, pass {pass}: (room, x, y, dir, vars hash) differ from the original"
            );
        }
    }
}

/// Replay a key log (lines "pass key-hex", the key pressed just before that pass) and
/// compare every pass with the original's trace.
fn compare_keys(trace_file: &str, keys_file: &str) {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/");
    let Ok(trace) = fs::read_to_string(format!("{base}extracted/reference/{trace_file}")) else { return };
    let Ok(keys) = fs::read_to_string(format!("{base}{keys_file}")) else { return };
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{base}extracted/disk{n}.do")).ok()).collect();
    let Some(imgs) = imgs else { return };
    let keys: std::collections::HashMap<usize, u8> = keys
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let mut w = World::new(Resources::load(&Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let lines: Vec<Vec<String>> = trace.lines().skip(1).map(|l| l.split(',').map(String::from).collect()).collect();
    for pass in 1..lines.len() {
        if let Some(&k) = keys.get(&pass) {
            w.key(k);
        }
        w.cycle();
        let o = &lines[pass];
        let hash =
            w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
        let port = (
            w.vars[1].to_string(),
            w.actors[0][f::X].to_string(),
            w.actors[0][f::Y].to_string(),
            w.actors[0][f::DIR].to_string(),
            w.vars[19].to_string(),
            format!("{hash:016x}"),
        );
        let orig = (o[3].clone(), o[4].clone(), o[5].clone(), o[6].clone(), o[7].clone(), o[8].clone());
        assert_eq!(
            port, orig,
            "{trace_file}, pass {pass}: (room, x, y, dir, score, vars hash) differ from the original"
        );
    }
}

/// The whole maximum-score route (routes/max_score.route, played by kqroute): 158
/// points, from power-on to the coronation, against the original pass by pass.
#[test]
fn max_score_playthrough() {
    compare_keys("max_score.csv", "routes/max_score.keys");
}

/// The speed model (engine::cost) against the original's measured cycles per pass over
/// the whole maximum-score run. Passes where the original waited for a key (disk-swap and
/// "more" prompts, answered by refemu after a delay) are left out.
#[test]
fn speed_model_matches_original() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/");
    let Ok(trace) = fs::read_to_string(format!("{base}extracted/reference/max_score.csv")) else { return };
    let Ok(keys) = fs::read_to_string(format!("{base}routes/max_score.keys")) else { return };
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{base}extracted/disk{n}.do")).ok()).collect();
    let Some(imgs) = imgs else { return };
    let keys: std::collections::HashMap<usize, u8> = keys
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let mut w = World::new(Resources::load(&Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let deltas: Vec<f64> = trace.lines().skip(1).map(|l| l.split(',').nth(2).unwrap().parse().unwrap()).collect();
    let (mut errs, mut orig, mut model) = (Vec::new(), 0.0, 0.0);
    for pass in 1..deltas.len() {
        if let Some(&k) = keys.get(&pass) {
            w.key(k);
        }
        w.cycle();
        let m = w.take_cost().cycles();
        let d = deltas[pass]; // trace row k+1 (the original's pass k+1) is the port's pass k
        if pass < 2 || d - m > 2_000_000.0 {
            continue;
        }
        errs.push((m - d).abs() / d);
        orig += d;
        model += m;
    }
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = errs[errs.len() / 2];
    let p90 = errs[errs.len() * 9 / 10];
    let total = (model - orig) / orig;
    assert!(median < 0.015 && p90 < 0.04 && total.abs() < 0.01, "median {median:.4}, p90 {p90:.4}, total {total:+.4}");
}
