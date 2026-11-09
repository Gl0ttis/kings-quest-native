//! kqprofile-pcjr DISK EVENTS PASS... [--checkpoints]: where the original's cycles go
//! in the given passes (0: from power-on to the first pass). The event log is followed
//! (`Runner::follow`; with `--checkpoints`, its keys pressed at their checkpoints) and
//! each pass's cycles are added up by 16-byte block of the engine's code. With one
//! pass, its blocks, the most costly first; with two or more, the blocks where they
//! differ most from the first. For the speed model: what a pass's time depends on.

use kq1_pcjr_refemu::runner::{Runner, Stopped};

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let at_checkpoints = a.iter().position(|x| x == "--checkpoints").map(|p| a.remove(p)).is_some();
    if a.len() < 3 {
        eprintln!("usage: kqprofile-pcjr DISK EVENTS PASS... [--checkpoints]");
        std::process::exit(2);
    }
    let raw = std::fs::read(&a[0]).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let passes: Vec<u64> = a[2..].iter().map(|p| p.parse().expect("a pass")).collect();
    let last = *passes.iter().max().unwrap();
    let mut r = Runner::new(&raw).expect("boot");
    if at_checkpoints {
        r.replay(&events);
    }
    let mut profiles: Vec<(u64, Vec<u64>)> = Vec::new();
    // Pass 0: power-on to the first pass.
    if passes.contains(&0) {
        r.profile = Some(vec![0; 0x1001]);
    }
    while r.passes <= last {
        let step = if at_checkpoints { r.run(u64::MAX, 4_000_000_000) } else { r.follow(&events) };
        match step {
            Ok(Stopped::Pass) => {}
            Ok(Stopped::Prompt) => continue,
            Err(e) => {
                eprintln!("stopped: {e}");
                break;
            }
        }
        // The pass that just ended.
        if let Some(p) = r.profile.take() {
            profiles.push((r.passes - 1, p));
        }
        if passes.contains(&r.passes) {
            r.profile = Some(vec![0; 0x1001]);
        }
    }
    let name = |b: usize| if b == 0x1000 { "other".to_string() } else { format!("{:04X}", b << 4) };
    let total = |p: &[u64]| p.iter().sum::<u64>();
    for (n, p) in &profiles {
        println!("pass {n}: {} cycles", total(p));
    }
    if profiles.len() == 1 {
        let p = &profiles[0].1;
        let mut b: Vec<usize> = (0..p.len()).filter(|&i| p[i] > 0).collect();
        b.sort_by_key(|&i| std::cmp::Reverse(p[i]));
        for i in b.into_iter().take(40) {
            println!("  {} {}", name(i), p[i]);
        }
    } else if let Some((first, base)) = profiles.first() {
        for (n, p) in &profiles[1..] {
            println!("pass {n} against pass {first}:");
            let mut b: Vec<usize> = (0..p.len()).filter(|&i| p[i] != base[i]).collect();
            b.sort_by_key(|&i| std::cmp::Reverse((p[i] as i64 - base[i] as i64).abs()));
            for i in b.into_iter().take(15) {
                println!("  {} {:+}", name(i), p[i] as i64 - base[i] as i64);
            }
        }
    }
}
