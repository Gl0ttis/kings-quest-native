// probe KEYS PASS [N]: replay a key log to PASS, then from that state try each direction
// for N passes (default 40) and report the ego's path and room.
use kq1_apple2::engine::{f, Resources, Scripted, World};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let keys: std::collections::HashMap<usize, u8> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let pass: usize = a[2].parse().unwrap();
    let n: usize = a.get(3).map_or(40, |v| v.parse().unwrap());
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    for p in 1..=pass {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
    }
    println!(
        "at pass {pass}: room {} ego ({}, {}) dir {} flags {:02X} fixed prio {:02X} view {:?}",
        w.vars[1],
        w.actors[0][f::X],
        w.actors[0][f::Y],
        w.actors[0][f::DIR],
        w.actors[0][f::FLAGS],
        w.actors[0][0x1D],
        w.actor_view[0]
    );
    for (name, d) in [
        ("up", 1u8),
        ("up-right", 2),
        ("right", 3),
        ("down-right", 4),
        ("down", 5),
        ("down-left", 6),
        ("left", 7),
        ("up-left", 8),
    ] {
        let mut c = w.clone();
        c.io = Some(Box::new(Scripted::default()));
        c.press_direction(d);
        let mut path = Vec::new();
        for _ in 0..n {
            c.cycle();
            path.push((c.actors[0][f::X], c.actors[0][f::Y]));
        }
        let last = path.last().unwrap();
        println!("{name:10} -> {:?} room {} v4 {} v32 {}", last, c.vars[1], c.vars[4], c.vars[32]);
    }
}
