//! speedcmp-pc DISK EVENTS TRACE.csv: the speed model against the original. The event log
//! is replayed in the port, and each pass's length by the model (the cycles from its
//! start to the next pass's) is compared with the original's (refemu-pc's trace).
//! Passes are taken apart: ordinary, loading (a disk read or a picture), and those that
//! waited (most: the actors wait for a tick before they move), which the clock times.

use kq1_pc::disk::Disk;
use kq1_pc::engine::clock::Replay;
use kq1_pc::engine::World;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        eprintln!("usage: speedcmp-pc DISK EVENTS TRACE.csv");
        std::process::exit(2);
    }
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let trace = std::fs::read_to_string(&a[2]).expect("trace");
    let orig: Vec<u64> = trace.lines().skip(1).map(|l| l.split(',').nth(1).unwrap().parse().unwrap()).collect();
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = orig.len() as u64;
    w.cost_trace = Some(Vec::new());
    w.run();
    let costs = w.cost_trace.take().unwrap();
    let mut kinds: [(Vec<f64>, u64, u64); 3] = Default::default();
    for k in 1..orig.len().min(w.trace.len()) - 1 {
        // Pass k+1's row (index k) to pass k+2's: pass k+1's work, costs[k+1].
        let (Some(c), mine, theirs) =
            (costs.get(k + 1), w.trace[k + 1].cycles - w.trace[k].cycles, orig[k + 1] - orig[k])
        else {
            continue;
        };
        let kind = if c.wait_ticks > 0 {
            2
        } else if c.sectors > 0 || c.plots > 0 {
            1
        } else {
            0
        };
        let e = &mut kinds[kind];
        e.0.push((mine as f64 - theirs as f64).abs() / theirs as f64);
        e.1 += mine;
        e.2 += theirs;
    }
    for (name, (mut e, mine, theirs)) in ["ordinary", "loading", "waiting"].into_iter().zip(kinds) {
        if e.is_empty() {
            continue;
        }
        e.sort_by(f64::total_cmp);
        let n = e.len();
        println!(
            "{name}: {n} passes, median {:.2}%, p90 {:.2}%, total {:+.2}% ({:+.1} s of {:.1} s)",
            e[n / 2] * 100.0,
            e[n * 9 / 10] * 100.0,
            (mine as f64 / theirs as f64 - 1.0) * 100.0,
            (mine as f64 - theirs as f64) / 4_772_727.0,
            theirs as f64 / 4_772_727.0
        );
    }
    println!(
        "start-up (power-on to pass 1): {:.2} s by the model, {:.2} s in the original",
        w.trace[0].cycles as f64 / 4_772_727.0,
        orig[0] as f64 / 4_772_727.0
    );
    let last = w.trace.len().min(orig.len()) - 1;
    let (mine, theirs) = (w.trace[last].cycles - w.trace[1].cycles, orig[last] - orig[1]);
    println!(
        "the whole run: {:.1} s by the model, {:.1} s in the original ({:+.2}%)",
        mine as f64 / 4_772_727.0,
        theirs as f64 / 4_772_727.0,
        (mine as f64 / theirs as f64 - 1.0) * 100.0
    );
}
