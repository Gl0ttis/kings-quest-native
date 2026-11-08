// costdump KEYS FROM TO: replay a key log and print the cost counters per pass.
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
    let (from, to): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let mut seen = w.log.len();
    let mut csv = std::env::var("CSV").ok().map(|p| {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(p).unwrap());
        writeln!(f, "pass,rule_bytes,cel_nibbles,screen_pixels,chars,sectors,plots,room_changes").unwrap();
        f
    });
    for pass in 1..=to {
        if let Some(&k) = keys.get(&pass) {
            w.key(k);
        }
        w.cycle();
        let c = w.take_cost();
        if let Some(f) = csv.as_mut() {
            use std::io::Write;
            writeln!(
                f,
                "{pass},{},{},{},{},{},{},{}",
                c.rule_bytes, c.cel_nibbles, c.screen_pixels, c.chars, c.sectors, c.plots, c.room_changes
            )
            .unwrap();
        }
        if pass >= from {
            println!("{pass}: {:?} = {:.0} cv={} ch={}", c, c.cycles(), w.text.cv, w.text.ch);
            for l in &w.log[seen..] {
                println!("    {l}");
            }
        }
        seen = w.log.len();
    }
    if let Ok(out) = std::env::var("SHOT") {
        let rgb: Vec<u8> = w.render_frame().iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        std::fs::write(
            out,
            kq1_apple2::png::encode_rgb(
                kq1_apple2::engine::text::FRAME_W as u32,
                kq1_apple2::engine::text::FRAME_H as u32,
                &rgb,
            ),
        )
        .unwrap();
    }
}
