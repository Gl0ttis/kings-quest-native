//! replay DISK EVENTS TRACE.csv [PASSES]: run the engine on refemu-pcjr's event log
//! (`--events`) and compare its pass trace with refemu-pcjr's (`--trace`), stopping at
//! the first pass that differs.

use kq1_pcjr::disk::Disk;
use kq1_pcjr::engine::clock::Replay;
use kq1_pcjr::engine::World;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: replay DISK EVENTS TRACE.csv [PASSES]");
        std::process::exit(2);
    }
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let reference: Vec<Vec<String>> = std::fs::read_to_string(&a[2])
        .expect("trace")
        .lines()
        .skip(1)
        .map(|l| l.split(',').map(String::from).collect())
        .collect();
    let passes: u64 = a.get(3).map_or(reference.len() as u64, |p| p.parse().expect("passes"));
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = passes;
    w.keep_vars = true;
    // With refemu's checkpoint log (REFEMU_CHECKPOINTS), compare the sequences.
    let theirs: Vec<(u64, u16)> = events
        .lines()
        .filter_map(|l| {
            let mut f = l.strip_prefix("C ")?.split_whitespace();
            Some((f.next()?.parse().ok()?, u16::from_str_radix(f.next()?, 16).ok()?))
        })
        .collect();
    if !theirs.is_empty() {
        w.checkpoint_trace = Some(Vec::new());
    }
    let why = w.run();
    println!("stopped: {why}");
    if let Some(mine) = &w.checkpoint_trace {
        let first = mine.iter().zip(&theirs).position(|(m, t)| *m != t.1);
        match first {
            Some(i) => {
                println!("checkpoints part at number {}: port {:04X}, original {:04X}", i + 1, mine[i], theirs[i].1);
                let from = i.saturating_sub(8);
                let show = |v: &[u16]| v.iter().map(|a| format!("{a:04X}")).collect::<Vec<_>>().join(" ");
                println!("  port:     {}", show(&mine[from..(i + 6).min(mine.len())]));
                let t: Vec<u16> = theirs[from..(i + 6).min(theirs.len())].iter().map(|t| t.1).collect();
                println!("  original: {}", show(&t));
            }
            None => println!("checkpoints: the same for all {} the port passed", mine.len()),
        }
    }
    for l in w.log.iter().take(20) {
        println!("log: {l}");
    }
    let mut same = 0;
    for (row, r) in w.trace.iter().zip(&reference) {
        let mine = [
            row.pass.to_string(),
            row.room.to_string(),
            row.ego_x.to_string(),
            row.ego_y.to_string(),
            row.ego_dir.to_string(),
            row.score.to_string(),
            format!("{:016x}", row.vars_hash),
        ];
        let theirs = [&r[0], &r[3], &r[4], &r[5], &r[6], &r[7], &r[8]];
        if mine.iter().zip(theirs).any(|(m, t)| m != t) {
            println!("first difference at pass {}:", row.pass);
            println!(
                "  port:   room {} x {} y {} dir {} score {} vars {}",
                mine[1], mine[2], mine[3], mine[4], mine[5], mine[6]
            );
            println!("  refemu: room {} x {} y {} dir {} score {} vars {}", r[3], r[4], r[5], r[6], r[7], r[8]);
            // With refemu's snapshot of that pass (--snap, SNAP_DIR), every variable.
            if let Ok(dir) = std::env::var("SNAP_DIR") {
                if let Ok(ds) = std::fs::read(format!("{dir}/{:05}.ds", row.pass)) {
                    let vars = &w.vars_trace[row.pass as usize - 1];
                    let diff: Vec<String> = (0..256)
                        .filter(|&v| ds[0x1003 + v] != vars[v])
                        .map(|v| format!("v{v} port {} refemu {}", vars[v], ds[0x1003 + v]))
                        .collect();
                    println!("  differing: {}", diff.join(", "));
                }
            }
            println!("  vars: {:?}", &w.vars[..48]);
            println!("{same} passes the same before it");
            return;
        }
        same += 1;
    }
    println!("{same} passes compared, all the same (port ran {}, refemu {})", w.trace.len(), reference.len());
}
