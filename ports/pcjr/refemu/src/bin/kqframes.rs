//! kqframes-pcjr DISK EVENTS [--from N] [--all] [--checkpoints] [--hashes OUT]: the screen, pass by pass, in the
//! original and in the port. The event log (kqroute-pcjr `--events`) is replayed in the
//! port, which hashes its screen at the start of each pass (`World::frame`), and
//! followed in the original (`Runner::follow`), whose screen is hashed at the same
//! instant (`Runner::frame`). At the first pass that differs (from pass N), both frames
//! are taken whole and the difference shown: the picture's rows and columns that
//! differ, and the text rows side by side. `--all` counts every pass that differs
//! instead of stopping at the first.

use kq1_pcjr::disk::Disk;
use kq1_pcjr::engine::clock::Replay;
use kq1_pcjr::engine::{frame_hash, World};
use kq1_pcjr_refemu::runner::{Runner, Stopped};

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
    let pic = |f: &[u8]| (f[0] == 0).then(|| f[1..1 + 160 * 168].to_vec());
    let text = |f: &[u8]| f[f.len() - 1000..].to_vec();
    if let (Some(a), Some(b)) = (pic(orig), pic(port)) {
        let rows: Vec<usize> = (0..168).filter(|&y| a[y * 160..(y + 1) * 160] != b[y * 160..(y + 1) * 160]).collect();
        if !rows.is_empty() {
            let cols: Vec<usize> = (0..160).filter(|&x| rows.iter().any(|&y| a[y * 160 + x] != b[y * 160 + x])).collect();
            let n = rows.iter().map(|&y| (0..160).filter(|&x| a[y * 160 + x] != b[y * 160 + x]).count()).sum::<usize>();
            println!(
                "  picture: {n} bytes differ, rows {}-{}, columns {}-{} (bytes, two pixels each)",
                rows[0],
                rows[rows.len() - 1],
                cols[0],
                cols[cols.len() - 1]
            );
            for &y in rows.iter().take(12) {
                let (x0, x1) = (cols[0].saturating_sub(2), (cols[cols.len() - 1] + 3).min(160));
                let hex = |p: &[u8]| p[y * 160 + x0..y * 160 + x1].iter().map(|b| format!("{b:02X}")).collect::<String>();
                println!("  y {y:3} x {x0:3}: orig {}\n                port {}", hex(&a), hex(&b));
            }
        }
    }
    let (ta, tb) = (text(orig), text(port));
    for r in 0..25 {
        let (ra, rb) = (&ta[r * 40..(r + 1) * 40], &tb[r * 40..(r + 1) * 40]);
        if ra != rb {
            let s = |x: &[u8]| x.iter().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { '.' }).collect::<String>();
            println!("  text row {r:2}: orig |{}|\n               port |{}|", s(ra), s(rb));
        }
    }
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let all = a.iter().position(|x| x == "--all").map(|p| a.remove(p)).is_some();
    // --checkpoints: press the log's keys at their checkpoints (Runner::replay), for a
    // log made that way (gold_key's, from kqroute-pcjr's first --resume).
    let at_checkpoints = a.iter().position(|x| x == "--checkpoints").map(|p| a.remove(p)).is_some();
    // --hashes OUT: the original's frame hash for every pass, one per line (hex), for
    // the port's tests (reference/*.frames).
    let hashes_out = a.iter().position(|x| x == "--hashes").map(|p| {
        let v = a.remove(p + 1);
        a.remove(p);
        v
    });
    let mut orig_hashes = String::new();
    let from: u64 = a.iter().position(|x| x == "--from").map_or(1, |p| {
        let v = a.remove(p + 1).parse().expect("--from N");
        a.remove(p);
        v
    });
    if a.len() != 2 {
        eprintln!("usage: kqframes-pcjr DISK EVENTS [--from N] [--all] [--checkpoints] [--hashes OUT]");
        std::process::exit(2);
    }
    let raw = std::fs::read(&a[0]).expect("disk");
    let disk = Disk::from_file(raw.clone()).expect("the game disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let passes = events.lines().filter(|l| l.starts_with("P ")).count() as u64;
    let (hashes, _) = port(&disk, &events, passes, None);
    let mut r = Runner::new(&raw).expect("boot");
    if at_checkpoints {
        r.replay(&events);
    }
    let mut differ = 0u64;
    let mut first = None;
    while r.passes < passes {
        let step = if at_checkpoints { r.run(u64::MAX, 4_000_000_000) } else { r.follow(&events) };
        match step {
            Ok(Stopped::Pass) => {}
            Ok(Stopped::Prompt) => continue,
            Err(e) => {
                eprintln!("stopped: {e}");
                break;
            }
        }
        let n = r.passes;
        if n < from {
            continue;
        }
        let Some(&h) = hashes.get(n as usize - 1) else { break };
        let f = r.frame();
        if hashes_out.is_some() {
            orig_hashes += &format!("{:016x}\n", frame_hash(&f));
        }
        if frame_hash(&f) != h {
            differ += 1;
            if first.is_none() {
                first = Some(n);
                let (_, pf) = port(&disk, &events, n, Some(n));
                show_diff(n, &f, &pf.expect("the port's frame"));
                if !all {
                    break;
                }
            }
        }
    }
    if let Some(p) = hashes_out {
        std::fs::write(p, &orig_hashes).expect("write the hashes");
    }
    match first {
        None => println!("{} passes compared from {from}: every screen the same", r.passes.min(passes)),
        Some(n) if all => println!("{differ} passes differ, the first {n} (of {} compared)", r.passes),
        Some(_) => {}
    }
}
