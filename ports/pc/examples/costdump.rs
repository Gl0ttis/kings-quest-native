//! costdump-pc DISK EVENTS OUT.csv: the work of every pass of a route (`engine::cost`), as
//! the port counts it, for fitting the speed model against the original's cycles
//! (tools/costfit.py). Row k is pass k's work, from its start to pass k+1's: what
//! refemu-pc's trace gives as the cycles of row k+1, less kqframes-pc's `--waits`.

use kq1_pc::disk::Disk;
use kq1_pc::engine::clock::Replay;
use kq1_pc::engine::cost::Cost;
use kq1_pc::engine::World;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        eprintln!("usage: costdump-pc DISK EVENTS OUT.csv");
        std::process::exit(2);
    }
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = events.lines().filter(|l| l.starts_with("P ")).count() as u64;
    w.cost_trace = Some(Vec::new());
    let why = w.run();
    let rows = w.cost_trace.take().unwrap_or_default();
    let mut out = format!("pass,{}\n", Cost::header());
    // Row 0 is the start-up, before pass 1.
    for (k, c) in rows.iter().enumerate().skip(1) {
        out += &format!("{k},{}\n", c.row());
    }
    std::fs::write(&a[2], out).expect("write");
    eprintln!("{} passes ({why})", rows.len().saturating_sub(1));
}
