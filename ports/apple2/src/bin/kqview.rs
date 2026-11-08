//! kqview <disk1.do> <disk2.do> <disk3.do> [--out dir]
//! Decodes every view in the $1900 directory and renders one sheet per view
//! (one row per loop) plus an overview sheet of each view's first cel.
use kq1_apple2::kq::{DiskAddr, Disks};
use kq1_apple2::{pic, png, view};
use std::{env, fs, path::PathBuf};

const SX: usize = 2; // a logical pixel is twice as wide as tall on screen
const SCALE: usize = 2;
const BG: [u8; 3] = [0x30, 0x30, 0x38];

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut out = PathBuf::from("extracted/views");
    if let Some(p) = args.iter().position(|a| a == "--out") {
        out = PathBuf::from(&args[p + 1]);
        args.drain(p..p + 2);
    }
    if args.len() != 3 {
        eprintln!("usage: kqview disk1.do disk2.do disk3.do [--out dir]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    fs::create_dir_all(&out).unwrap();
    let (mut ok, mut cels, mut failed) = (0, 0, 0);
    let mut firsts: Vec<(usize, Canvas)> = Vec::new();
    for (i, rec) in disks.directory(0x1900).unwrap().iter().enumerate() {
        if rec[0] == 0 || kq1_apple2::kq::UNUSED.contains(&("view", i)) {
            continue;
        }
        let Ok(res) = disks.read(DiskAddr::from_record(rec), 1) else { continue };
        match view::decode(res.data) {
            Ok(v) => {
                ok += 1;
                cels += v.loops.iter().map(|l| l.cels.len()).sum::<usize>();
                let sheet = render_view(&v);
                fs::write(out.join(format!("{i:03}.png")), sheet.png()).unwrap();
                if let Some(c) = v.loops.first().and_then(|l| l.cels.first()) {
                    let mut cv = Canvas::new(c.width as usize * SX * SCALE + 8, c.height as usize * SCALE + 8);
                    cv.cel(c, 4, 4);
                    firsts.push((i, cv));
                }
            }
            Err(e) => {
                failed += 1;
                println!("view {i:03}: {e}");
            }
        }
    }
    // Overview: first cel of each view, packed into rows.
    let width = 900;
    let mut rows: Vec<Vec<&Canvas>> = vec![Vec::new()];
    let mut x = 0;
    for (_, c) in &firsts {
        if x + c.w > width {
            rows.push(Vec::new());
            x = 0;
        }
        rows.last_mut().unwrap().push(c);
        x += c.w;
    }
    let height: usize = rows.iter().map(|r| r.iter().map(|c| c.h).max().unwrap_or(0)).sum();
    let mut all = Canvas::new(width, height);
    let mut y = 0;
    for r in &rows {
        let mut x = 0;
        for c in r {
            all.blit(c, x, y);
            x += c.w;
        }
        y += r.iter().map(|c| c.h).max().unwrap_or(0);
    }
    fs::write(out.join("overview.png"), all.png()).unwrap();
    println!(
        "{ok} views decoded ({cels} cels), {failed} failed; overview order: {:?}",
        firsts.iter().map(|f| f.0).collect::<Vec<_>>()
    );
}

struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        let mut rgb = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            rgb.extend_from_slice(&BG);
        }
        Canvas { w, h, rgb }
    }
    fn put(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            self.rgb[(y * self.w + x) * 3..][..3].copy_from_slice(&c);
        }
    }
    fn cel(&mut self, c: &view::Cel, ox: usize, oy: usize) {
        for y in 0..c.height as usize {
            for x in 0..c.width as usize {
                if let Some(v) = c.pixels[y * c.width as usize + x] {
                    let rgb = pic::dhgr_rgb(v);
                    for dy in 0..SCALE {
                        for dx in 0..SX * SCALE {
                            self.put(ox + x * SX * SCALE + dx, oy + y * SCALE + dy, rgb);
                        }
                    }
                }
            }
        }
    }
    fn blit(&mut self, src: &Canvas, ox: usize, oy: usize) {
        for y in 0..src.h {
            for x in 0..src.w {
                let p = &src.rgb[(y * src.w + x) * 3..][..3];
                self.put(ox + x, oy + y, [p[0], p[1], p[2]]);
            }
        }
    }
    fn png(&self) -> Vec<u8> {
        png::encode_rgb(self.w as u32, self.h as u32, &self.rgb)
    }
}

fn render_view(v: &view::View) -> Canvas {
    let pad = 4;
    let row_w = |l: &view::Loop| l.cels.iter().map(|c| c.width as usize * SX * SCALE + pad).sum::<usize>() + pad;
    let row_h = |l: &view::Loop| l.cels.iter().map(|c| c.height as usize * SCALE).max().unwrap_or(0) + pad;
    let w = v.loops.iter().map(row_w).max().unwrap_or(1);
    let h = v.loops.iter().map(row_h).sum::<usize>() + pad;
    let mut cv = Canvas::new(w.max(1), h.max(1));
    let mut y = pad;
    for l in &v.loops {
        let rh = row_h(l);
        let mut x = pad;
        for c in &l.cels {
            // Align cels on their bottom row, as the engine does.
            cv.cel(c, x, y + (rh - pad) - c.height as usize * SCALE);
            x += c.width as usize * SX * SCALE + pad;
        }
        y += rh;
    }
    cv
}
