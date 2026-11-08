// tracekeys KEYS PASSES OUT: replay a key log for PASSES passes and write the trace
// (same columns and hash as kq --trace).
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
    let n: usize = a[2].parse().unwrap();
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let mut t = String::from("pass,room,ego_x,ego_y,ego_dir,score,model_cycles,vars_hash\n");
    for p in 1..=n {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
        let h = w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
        t += &format!(
            "{p},{},{},{},{},{},0,{h:016x}\n",
            w.vars[1],
            w.actors[0][f::X],
            w.actors[0][f::Y],
            w.actors[0][f::DIR],
            w.vars[19]
        );
    }
    std::fs::write(&a[3], t).unwrap();
}
