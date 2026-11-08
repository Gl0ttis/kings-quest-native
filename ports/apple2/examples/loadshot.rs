// loadshot KEYS PASS CYCLES OUT.png: replay a key log to PASS and write the frame the
// window shows CYCLES into that pass (a picture part drawn, during a room change).
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
    let (pass, at): (usize, f64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let mut prev = w.render_frame();
    for p in 1..=pass {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        if p == pass {
            break;
        }
        w.cycle();
        w.take_cost();
        prev = w.render_frame();
    }
    w.cycle();
    let t = at / kq1_apple2::engine::cost::CPU_HZ;
    let mut frame = prev;
    for d in &w.pic_draws {
        let (t0, t1) = d.span(false);
        println!("draw: {:.2}s to {:.2}s, {} plots", t0, t1, d.plots);
        if t >= t0 {
            let n = if t >= t1 { d.plots } else { ((t - t0) / (t1 - t0) * d.plots as f64) as u32 };
            frame = w.render_frame_with(&d.partial(n), false);
        }
    }
    println!("pass total {:.2}s", w.take_cost().seconds());
    let rgb: Vec<u8> = frame.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    std::fs::write(
        &a[4],
        kq1_apple2::png::encode_rgb(
            kq1_apple2::engine::text::FRAME_W as u32,
            kq1_apple2::engine::text::FRAME_H as u32,
            &rgb,
        ),
    )
    .unwrap();
}
