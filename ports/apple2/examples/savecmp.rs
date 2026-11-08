// savecmp KEYS PASS DISK.nib: compare the save the original wrote (slot A) with the
// port's save image after replaying the key log to PASS.
use kq1_apple2::engine::{Resources, Scripted, World};
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
    let disk = kq1_apple2::savedisk::SaveDisk::open(std::fs::read(&a[3]).unwrap()).unwrap();
    for s in 0..9 {
        let d = disk.read_sector(1, s).unwrap();
        println!("sector {s}: {}", d[..48].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "));
    }
    let orig = disk.load_game(1).expect("no save in slot A");
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    for p in 1..=pass {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
    }
    let port = w.save_image();
    let names = [
        ("objects", 0x1A80usize, 0x500usize),
        ("actors", 0x0100, 0x7C),
        ("zero page", 0x0060, 0x90),
        ("variables", 0x6E06, 0xDF),
    ];
    let mut p = 8;
    for (n, base, len) in names {
        let d: Vec<usize> = (0..len).filter(|&i| orig[p + i] != port[p + i]).collect();
        println!("{n}: {} bytes differ", d.len());
        for &i in d.iter().take(40) {
            println!("   ${:04X}: orig {:02X} port {:02X}", base + i, orig[p + i], port[p + i]);
        }
        p += len;
    }
}
