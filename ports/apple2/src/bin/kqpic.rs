//! kqpic <disk1.do> <disk2.do> <disk3.do> [--out dir]
//! Renders every room picture to PNG (visual and priority), plus a contact sheet.
use kq1_apple2::kq::{self, DiskAddr, Disks};
use kq1_apple2::pic::{self, HEIGHT, WIDTH};
use kq1_apple2::png;
use std::{env, fs, path::PathBuf};

/// Each logical pixel is 4 DHGR dots wide and one scan line tall; 2x1 keeps the shape.
const SX: usize = 2;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut out = PathBuf::from("extracted/pics");
    if let Some(p) = args.iter().position(|a| a == "--out") {
        out = PathBuf::from(&args[p + 1]);
        args.drain(p..p + 2);
    }
    if args.len() != 3 {
        eprintln!("usage: kqpic disk1.do disk2.do disk3.do [--out dir]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    fs::create_dir_all(&out).unwrap();

    let mut sheet: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut warned = 0;
    for (i, rec) in disks.directory(0x1700).unwrap().iter().enumerate() {
        if kq::UNUSED.contains(&("pic", i)) || rec[0] == 0 {
            continue;
        }
        let res = match disks.read(DiskAddr::from_record(rec), 1) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let p = pic::draw(res.data);
        for w in &p.warnings {
            println!("pic {i:03}: {w}");
            warned += 1;
        }
        let vis = image(|x, y| pic::dhgr_rgb(p.visual(x, y)));
        let pri = image(|x, y| [p.priority(x, y) * 17; 3]);
        fs::write(out.join(format!("{i:03}.png")), png::encode_rgb((WIDTH * SX) as u32, HEIGHT as u32, &vis)).unwrap();
        fs::write(out.join(format!("{i:03}_pri.png")), png::encode_rgb((WIDTH * SX) as u32, HEIGHT as u32, &pri))
            .unwrap();
        sheet.push((i, vis));
    }

    // Contact sheet: 8 columns with a 4-pixel gap.
    let (cw, ch, gap, cols) = (WIDTH * SX, HEIGHT, 4, 8);
    let rows = sheet.len().div_ceil(cols);
    let (sw, sh) = (cols * (cw + gap), rows * (ch + gap));
    let mut s = vec![0x40u8; sw * sh * 3];
    for (n, (_, img)) in sheet.iter().enumerate() {
        let (ox, oy) = ((n % cols) * (cw + gap), (n / cols) * (ch + gap));
        for y in 0..ch {
            let d = ((oy + y) * sw + ox) * 3;
            s[d..d + cw * 3].copy_from_slice(&img[y * cw * 3..(y + 1) * cw * 3]);
        }
    }
    fs::write(out.join("sheet.png"), png::encode_rgb(sw as u32, sh as u32, &s)).unwrap();
    println!(
        "{} pictures rendered, {warned} warnings; sheet order: {:?}",
        sheet.len(),
        sheet.iter().map(|s| s.0).collect::<Vec<_>>()
    );
}

fn image(f: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let mut v = Vec::with_capacity(WIDTH * SX * HEIGHT * 3);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let c = f(x, y);
            for _ in 0..SX {
                v.extend_from_slice(&c);
            }
        }
    }
    v
}
