//! kqroom <disk1.do> <disk2.do> <disk3.do> [--out dir] [--rooms a-b]
//! M1 room viewer: enters each room through the real scripts (global + room cycle rules)
//! and renders the resulting screen: picture, objects added to it, and visible actors.
use kq1_apple2::engine::{Resources, World};
use kq1_apple2::kq::Disks;
use kq1_apple2::pic::{HEIGHT, WIDTH};
use kq1_apple2::png;
use std::{env, fs, path::PathBuf};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args[p + 1].clone();
        args.drain(p..p + 2);
        Some(v)
    };
    let out = PathBuf::from(opt("--out").unwrap_or_else(|| "extracted/rooms".into()));
    let (first, last) = match opt("--rooms") {
        Some(r) => {
            let (a, b) = r.split_once('-').unwrap_or((&r, &r));
            (a.parse().unwrap(), b.parse().unwrap())
        }
        None => (1u8, 80u8),
    };
    if args.len() != 3 {
        eprintln!("usage: kqroom disk1.do disk2.do disk3.do [--out dir] [--rooms a-b]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    fs::create_dir_all(&out).unwrap();
    let mut sheet: Vec<Vec<u8>> = Vec::new();
    let mut report = String::new();
    for r in first..=last {
        let res = Resources::load(&disks, false).expect("resources");
        let mut w = World::new(res);
        let cycles = w.enter_room(r, 8);
        let screen = w.compose();
        let rgb = World::to_rgb(&screen);
        fs::write(out.join(format!("{r:03}.png")), png::encode_rgb((WIDTH * 2) as u32, HEIGHT as u32, &rgb)).unwrap();
        let line = format!("room {r:3}: {cycles} cycles, v25={} {}\n", w.vars[25], w.actor_summary());
        report += &line;
        for l in &w.log {
            report += &format!("    {l}\n");
        }
        sheet.push(rgb);
    }
    // Contact sheet, 8 per row.
    let (cw, ch, gap, cols) = (WIDTH * 2, HEIGHT, 4, 8);
    let rows = sheet.len().div_ceil(cols);
    let (sw, sh) = (cols * (cw + gap), rows * (ch + gap));
    let mut s = vec![0x40u8; sw * sh * 3];
    for (n, img) in sheet.iter().enumerate() {
        let (ox, oy) = ((n % cols) * (cw + gap), (n / cols) * (ch + gap));
        for y in 0..ch {
            let d = ((oy + y) * sw + ox) * 3;
            s[d..d + cw * 3].copy_from_slice(&img[y * cw * 3..(y + 1) * cw * 3]);
        }
    }
    fs::write(out.join("sheet.png"), png::encode_rgb(sw as u32, sh as u32, &s)).unwrap();
    fs::write(out.join("report.txt"), &report).unwrap();
    println!("{} rooms rendered; report in {}", sheet.len(), out.join("report.txt").display());
}
