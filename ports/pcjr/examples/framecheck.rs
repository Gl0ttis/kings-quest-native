//! framecheck DISK EVENTS PASS: the port's frame at PASS (`World::frame`), summarised:
//! the picture's byte values and the text rows. A look at what kqframes-pcjr compares.

use kq1_pcjr::disk::Disk;
use kq1_pcjr::engine::clock::Replay;
use kq1_pcjr::engine::World;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        eprintln!("usage: framecheck DISK EVENTS PASS");
        std::process::exit(2);
    }
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let n: u64 = a[2].parse().expect("PASS");
    let mut w = World::new(&disk, Box::new(Replay::parse(&events))).expect("engine");
    w.max_passes = n;
    w.frame_at = Some(n);
    w.run();
    let f = w.frame.expect("the frame");
    let mut seen = [0usize; 256];
    for &b in &f[1..f.len() - 1000] {
        seen[b as usize] += 1;
    }
    let values: Vec<String> = (0..256).filter(|&b| seen[b] > 0).map(|b| format!("{b:02X}:{}", seen[b])).collect();
    println!("text screen {}, picture bytes {}, values {}", f[0], f.len() - 1001, values.join(" "));
    for r in f[f.len() - 1000..].chunks(40) {
        let s: String = r.iter().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { '.' }).collect();
        if !s.trim().is_empty() {
            println!("|{s}|");
        }
    }
}
