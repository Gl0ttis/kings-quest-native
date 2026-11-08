// displayshot KEYS PASS OUT_PREFIX: the frame after PASS with each display option,
// written as OUT_PREFIX-plain.png, -specks.png and -ntsc.png.
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
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    for p in 1..=pass {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
    }
    for (name, specks, ntsc) in [("plain", false, false), ("specks", true, false), ("ntsc", false, true)] {
        w.display.fill_specks = specks;
        w.display.ntsc = ntsc;
        let rgb: Vec<u8> = w.render_frame().iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        std::fs::write(
            format!("{}-{name}.png", a[3]),
            kq1_apple2::png::encode_rgb(
                kq1_apple2::engine::text::FRAME_W as u32,
                kq1_apple2::engine::text::FRAME_H as u32,
                &rgb,
            ),
        )
        .unwrap();
    }
}
