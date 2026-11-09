//! kqframes-pc DISK EVENTS --hashes OUT [--trace OUT]: the screen's hash at every pass of a
//! run kqroute-pc recorded (its `--events` log), for the reference (reference/*.frames).
//! kqframes-pc DISK EVENTS [--from N]: the original's screen against the port's.
//!
//! The run is followed exactly (runner `follow`: the log's keys typed where the run
//! stood when they were typed), and at the start of every pass the screen (runner
//! `frame`) is hashed (FNV-1a), one a line in hex, as kqplan-pc's `--frames`. The run
//! must stay on the log (it is checked): `--trace` writes the followed run's trace,
//! which is the recorded one's.
//!
//! Without `--hashes`, the log is replayed in the port too (its `World::frame`), and at
//! the first pass whose screens differ (from pass N) both are shown: the CGA lines and
//! bytes that differ (each byte two picture pixels' dot patterns), and the text rows.

use kq1_pc::disk::Disk;
use kq1_pc::engine::clock::Replay;
use kq1_pc::engine::{frame_hash, World};
use kq1_pc_refemu::runner::{Runner, Stopped};

/// The CGA's picture area in a frame: 168 lines of 80 bytes.
const LINE: usize = 80;
const LINES: usize = 168;

/// The port's frame hashes for every pass, or its whole frame at pass `at`.
fn port(disk: &Disk, events: &str, passes: u64, at: Option<u64>) -> (Vec<u64>, Option<Vec<u8>>) {
    let mut w = World::new(disk, Box::new(Replay::parse(events))).expect("engine");
    w.max_passes = passes;
    w.frame_hashes = at.is_none().then(Vec::new);
    w.frame_at = at;
    w.run();
    (w.frame_hashes.take().unwrap_or_default(), w.frame.take())
}

fn show_diff(pass: u64, orig: &[u8], port: &[u8]) {
    println!("pass {pass}: the screens differ");
    if orig[0] != port[0] {
        println!("  text screen up: original {}, port {}", orig[0], port[0]);
    }
    let pic = |f: &[u8]| (f[0] == 0 && f.len() > LINE * LINES).then(|| f[1..1 + LINE * LINES].to_vec());
    let text = |f: &[u8]| f[f.len() - 1000..].to_vec();
    if let (Some(a), Some(b)) = (pic(orig), pic(port)) {
        let rows: Vec<usize> =
            (0..LINES).filter(|&y| a[y * LINE..(y + 1) * LINE] != b[y * LINE..(y + 1) * LINE]).collect();
        if !rows.is_empty() {
            let cols: Vec<usize> =
                (0..LINE).filter(|&x| rows.iter().any(|&y| a[y * LINE + x] != b[y * LINE + x])).collect();
            let n =
                rows.iter().map(|&y| (0..LINE).filter(|&x| a[y * LINE + x] != b[y * LINE + x]).count()).sum::<usize>();
            println!(
                "  picture: {n} bytes differ, lines {}-{}, bytes {}-{} (two pixels each)",
                rows[0],
                rows[rows.len() - 1],
                cols[0],
                cols[cols.len() - 1]
            );
            for &y in rows.iter().take(12) {
                let (x0, x1) = (cols[0].saturating_sub(2), (cols[cols.len() - 1] + 3).min(LINE));
                let hex =
                    |p: &[u8]| p[y * LINE + x0..y * LINE + x1].iter().map(|b| format!("{b:02X}")).collect::<String>();
                println!("  y {y:3} x {x0:2}: orig {}\n               port {}", hex(&a), hex(&b));
            }
        }
    }
    let (ta, tb) = (text(orig), text(port));
    for r in 0..25 {
        let (ra, rb) = (&ta[r * 40..(r + 1) * 40], &tb[r * 40..(r + 1) * 40]);
        if ra != rb {
            let s = |x: &[u8]| {
                x.iter().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { '.' }).collect::<String>()
            };
            println!("  text row {r:2}: orig |{}|\n               port |{}|", s(ra), s(rb));
        }
    }
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = a.iter().position(|x| x == name)?;
        let v = a.get(p + 1).cloned();
        a.drain(p..(p + 2).min(a.len()));
        v
    };
    let (hashes, trace) = (opt("--hashes"), opt("--trace"));
    let from: u64 = opt("--from").map_or(1, |v| v.parse().expect("--from N"));
    let [disk, events] = a.as_slice() else {
        eprintln!("usage: kqframes-pc DISK EVENTS [--hashes OUT] [--trace OUT] [--from N]");
        std::process::exit(2);
    };
    let raw = std::fs::read(disk).unwrap_or_else(|e| panic!("{disk}: {e}"));
    let log = std::fs::read_to_string(events).unwrap_or_else(|e| panic!("{events}: {e}"));
    let passes = log.lines().filter(|l| l.starts_with("P ")).count() as u64;
    let game = Disk::from_file(raw.clone()).expect("the game disk");
    let compare = hashes.is_none();
    let port_hashes = if compare { port(&game, &log, passes, None).0 } else { Vec::new() };
    let mut r = Runner::new(&raw).unwrap_or_else(|e| panic!("{e}"));
    let mut out = String::new();
    while r.passes < passes {
        match r.follow(&log) {
            Ok(Stopped::Pass) => {}
            Ok(Stopped::Prompt) => continue,
            Err(e) => panic!("pass {}: {e}", r.passes),
        }
        let f = r.frame();
        let h = frame_hash(&f);
        if !compare {
            out += &format!("{h:016x}\n");
            continue;
        }
        let n = r.passes;
        if n < from {
            continue;
        }
        match port_hashes.get(n as usize - 1) {
            Some(&p) if p == h => {}
            Some(_) => {
                let (_, pf) = port(&game, &log, n, Some(n));
                show_diff(n, &f, &pf.expect("the port's frame"));
                return;
            }
            None => break,
        }
    }
    if compare {
        println!("{} passes compared from {from}: every screen the same", r.passes);
        return;
    }
    std::fs::write(hashes.unwrap(), out).expect("write the hashes");
    if let Some(p) = trace {
        std::fs::write(p, &r.trace).expect("write the trace");
    }
    eprintln!("{} passes followed", r.passes);
}
