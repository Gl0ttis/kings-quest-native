//! drawcmp DISK EVENTS PLOTLOG OUT.csv: the room pictures' drawing in the port against the
//! original's (kqplotlog-pcjr's PLOTLOG for the same event log). For each picture, a row
//! per mark of the port's drawing: the picture, the bytes written to the displayed page
//! so far, the port's work so far (pic::Work's counts), and the original's cycles from
//! the picture's start to that byte (the clear's 26,880 bytes are counted first), with
//! the original's cycles to the colours merged on the last row. For tools/drawfit.py.
use kq1_pcjr::engine::clock::Replay;
use kq1_pcjr::engine::World;
use std::fmt::Write;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let disk = kq1_pcjr::disk::Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let plotlog = std::fs::read_to_string(&a[2]).expect("plot log");
    // The original's pictures: (start, [(bytes, cycle)], merge).
    let mut orig: Vec<(u64, Vec<(u64, u64)>, u64)> = Vec::new();
    for l in plotlog.lines() {
        let f: Vec<u64> = l[2..].split(' ').map(|x| x.parse().unwrap()).collect();
        match &l[..1] {
            "S" => orig.push((f[0], Vec::new(), 0)),
            "W" => orig.last_mut().unwrap().1.push((f[1], f[0])),
            _ => orig.last_mut().unwrap().2 = f[0],
        }
    }
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.on_frame = Some(Box::new(|_: &mut World| {}));
    w.max_passes = events.lines().filter(|l| l.starts_with("P ")).count() as u64;
    w.drawing_trace = Some(Vec::new());
    w.run();
    let port = w.drawing_trace.take().unwrap();
    println!("pictures: port {}, original {}", port.len(), orig.len());
    let mut out = String::from("pic,bytes,abs_steps,to_steps,lines,fill_bytes,fill_scans,fill_runs,fill_seeds,orig\n");
    const CLEAR: u64 = 26_880;
    for (k, (p, (start, writes, merge))) in port.iter().zip(&orig).enumerate() {
        let at = |n: u64| -> u64 {
            let i = writes.partition_point(|&(b, _)| b < n);
            writes.get(i).map_or(*merge, |&(_, c)| c) - start
        };
        for &(n, wk) in p {
            let _ = writeln!(
                out,
                "{k},{n},{},{},{},{},{},{},{},{}",
                wk.abs_steps,
                wk.to_steps,
                wk.lines,
                wk.fill_bytes,
                wk.fill_scans,
                wk.fill_runs,
                wk.fill_seeds,
                at(CLEAR + n as u64)
            );
        }
        let last = writes.last().map_or(0, |&(b, _)| b);
        let _ = writeln!(
            out,
            "{k},end,{},{},,,,,,{}",
            last.saturating_sub(CLEAR),
            p.last().map_or(0, |m| m.0),
            merge - start
        );
    }
    std::fs::write(&a[3], out).expect("write");
}
