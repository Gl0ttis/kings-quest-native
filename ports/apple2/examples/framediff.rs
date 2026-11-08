// framediff KEYS FRAMES PASSES OUTDIR: replay a key log in the port and compare its
// screen with the original's, pass by pass. FRAMES is a refemu --frames capture of the
// same key log; refemu's frame at the start of pass k+1 is the port's after pass k
// (the trace alignment of tests/reference_trace.rs).
//
// The comparison is in the Apple's terms: the port's frame is turned back into the 560
// double hi-res dots of each line (its colours are the DHGR dot patterns, and its text
// rows are drawn dot for dot), and text mode compares the 40x24 screen codes. Writes
// OUTDIR/framediff.csv (one row per pass) and, for each run of differing passes, the
// worst one as OUTDIR/diff_PASS.png: the original, the port, and the port dimmed with
// the differing dots in red. FRAMEDIFF_SWAP=1 turns on the disk-swap prompts.
use kq1_apple2::engine::{text, Resources, Scripted, World};
use std::collections::HashMap;

const ROW_BYTES: usize = 70;
const DOTS: usize = 192 * ROW_BYTES;
const RECORD: usize = 4 + 1 + DOTS + 24 * 40;
/// Double hi-res colour by dot pattern (first dot is the high bit of the index), as
/// refemu's screenshot and kq1_apple2::pic::dhgr_rgb use.
const PAL: [u32; 16] = [
    0x000000, 0xDD0033, 0x885500, 0xFF6600, 0x007722, 0x555555, 0x11DD00, 0xFFFF00, 0x000099, 0xDD22DD, 0xAAAAAA,
    0xFF9988, 0x2222FF, 0x66AAFF, 0x44FF99, 0xFFFFFF,
];

struct Frame {
    text: bool,
    dots: Vec<u8>,
    page: Vec<u8>,
}

fn dot(dots: &[u8], y: usize, x: usize) -> bool {
    dots[y * ROW_BYTES + x / 8] >> (x % 8) & 1 == 1
}

/// The port's frame as dots: a 4-pixel group of one palette colour is that colour's
/// dot pattern; any other group (text rows) is read dot by dot, white as lit.
fn port_dots(fb: &[u32]) -> Vec<u8> {
    let mut dots = vec![0u8; DOTS];
    for y in 0..192 {
        for g in 0..140 {
            let px = &fb[y * 560 + g * 4..y * 560 + g * 4 + 4];
            let bits: [bool; 4] = match PAL.iter().position(|&c| c == px[0]).filter(|_| px.iter().all(|&p| p == px[0]))
            {
                Some(i) => std::array::from_fn(|k| i >> (3 - k) & 1 == 1),
                None => std::array::from_fn(|k| px[k] == 0xFFFFFF),
            };
            for (k, &b) in bits.iter().enumerate() {
                let x = g * 4 + k;
                dots[y * ROW_BYTES + x / 8] |= (b as u8) << (x % 8);
            }
        }
    }
    dots
}

fn render(dots: &[u8]) -> Vec<u32> {
    let mut fb = vec![0u32; 560 * 192];
    for y in 0..192 {
        for g in 0..140 {
            let i = (0..4).fold(0, |i, k| i << 1 | dot(dots, y, g * 4 + k) as usize);
            fb[y * 560 + g * 4..y * 560 + g * 4 + 4].fill(PAL[i]);
        }
    }
    fb
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 5 {
        eprintln!("usage: framediff KEYS FRAMES PASSES OUTDIR");
        std::process::exit(2);
    }
    let keys: HashMap<usize, u8> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let raw = std::fs::read(&a[2]).unwrap();
    assert_eq!(raw.len() % RECORD, 0, "not a refemu --frames file");
    let orig: Vec<(usize, Frame)> = raw
        .chunks(RECORD)
        .map(|r| {
            let pass = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
            (pass, Frame { text: r[4] != 0, dots: r[5..5 + DOTS].to_vec(), page: r[5 + DOTS..].to_vec() })
        })
        .collect();
    let n: usize = a[3].parse().unwrap();
    let out = std::path::Path::new(&a[4]);
    std::fs::create_dir_all(out).unwrap();

    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    // FRAMEDIFF_SWAP=1: show the original's disk-swap prompts (kq --swap-prompts); the
    // RETURN they wait for is answered at once, as refemu does.
    w.swap_prompts = std::env::var_os("FRAMEDIFF_SWAP").is_some();
    w.boot();

    let mut csv = String::from("pass,room,orig_mode,port_mode,picture_dots,status_dots,text_cells\n");
    // Runs of differing passes: (first, last, worst pass, worst count, room).
    let mut runs: Vec<(usize, usize, usize, usize, u8)> = Vec::new();
    let mut worst_frames: HashMap<usize, (Vec<u8>, Vec<u8>)> = HashMap::new();
    // FRAMEDIFF_DUMP=p,p,...: also write those passes' images, differing or not;
    // FRAMEDIFF_CROP=x0,y0,x1,y1: write only that region (dots), scaled up 4x.
    let dump: Vec<usize> = std::env::var("FRAMEDIFF_DUMP")
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    let crop: Option<Vec<usize>> =
        std::env::var("FRAMEDIFF_CROP").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let mut oi = 0;
    let (mut same, mut differ) = (0usize, 0usize);
    // Port state after pass p (p = 0: after boot) against refemu's frame at pass p+1.
    for p in 0..n {
        if p > 0 {
            if let Some(&k) = keys.get(&p) {
                w.key(k);
            }
            w.cycle();
        }
        let rp = p + 1;
        while oi + 1 < orig.len() && orig[oi + 1].0 <= rp {
            oi += 1;
        }
        if orig[oi].0 > rp {
            continue;
        }
        let o = &orig[oi].1;
        let port_text = w.text.text_mode;
        let (mut pic, mut status, mut cells) = (0usize, 0usize, 0usize);
        let mut pd = Vec::new();
        if o.text || port_text {
            if o.text && port_text {
                for r in 0..text::ROWS {
                    for c in 0..text::COLS {
                        cells += (w.text.page[r][c] != o.page[r * 40 + c]) as usize;
                    }
                }
            } else {
                cells = 960;
            }
        } else {
            pd = port_dots(&w.render_frame());
            for y in 0..192 {
                for x in 0..560 {
                    if dot(&pd, y, x) != dot(&o.dots, y, x) {
                        if y < 160 {
                            pic += 1;
                        } else {
                            status += 1;
                        }
                    }
                }
            }
        }
        let room = w.vars[1];
        if dump.contains(&p) {
            // The port's actor table (31 bytes each), to compare with REFEMU_DUMP.
            let act: Vec<String> = w.actors.iter().map(|a| format!("{:?}", &a[..31])).collect();
            eprintln!("port actors after pass {p}: {}", act.join(" "));
        }
        if dump.contains(&p) && !pd.is_empty() {
            worst_frames.insert(p, (o.dots.clone(), pd.clone()));
        }
        csv += &format!("{p},{room},{},{},{pic},{status},{cells}\n", o.text as u8, port_text as u8);
        let total = pic + status + cells;
        if total == 0 {
            same += 1;
            continue;
        }
        differ += 1;
        match runs.last_mut() {
            Some(r) if r.1 + 1 == p => {
                r.1 = p;
                if total > r.3 {
                    worst_frames.remove(&r.2);
                    (r.2, r.3) = (p, total);
                    if !pd.is_empty() {
                        worst_frames.insert(p, (o.dots.clone(), pd));
                    }
                }
            }
            _ => {
                runs.push((p, p, p, total, room));
                if !pd.is_empty() {
                    worst_frames.insert(p, (o.dots.clone(), pd));
                }
            }
        }
    }
    std::fs::write(out.join("framediff.csv"), csv).unwrap();

    for (&p, (od, pd)) in &worst_frames {
        let (of, pf) = (render(od), render(pd));
        let mut img = Vec::with_capacity(560 * 192 * 3 * 3);
        let rgb = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        for fb in [&of, &pf] {
            img.extend(fb.iter().flat_map(|&c| rgb(c)));
        }
        for y in 0..192 {
            for x in 0..560 {
                if dot(od, y, x) != dot(pd, y, x) {
                    img.extend([255, 0, 0]);
                } else {
                    let [r, g, b] = rgb(pf[y * 560 + x]);
                    img.extend([r / 4, g / 4, b / 4]);
                }
            }
        }
        if let Some(c) = &crop {
            let (x0, y0, x1, y1) = (c[0], c[1], c[2], c[3]);
            let (cw, ch) = (x1 - x0, y1 - y0);
            let mut z = Vec::new();
            for panel in 0..3 {
                for y in 0..ch * 4 {
                    for x in 0..cw * 2 {
                        let i = ((panel * 192 + y0 + y / 4) * 560 + x0 + x / 2) * 3;
                        z.extend_from_slice(&img[i..i + 3]);
                    }
                }
            }
            std::fs::write(
                out.join(format!("crop_{p:05}.png")),
                kq1_apple2::png::encode_rgb((cw * 2) as u32, (ch * 4 * 3) as u32, &z),
            )
            .unwrap();
            continue;
        }
        std::fs::write(out.join(format!("diff_{p:05}.png")), kq1_apple2::png::encode_rgb(560, 192 * 3, &img)).unwrap();
    }

    println!("passes compared: {}, identical: {same}, differing: {differ}", same + differ);
    println!("runs of differing passes: {}", runs.len());
    println!("first..last  passes  room  worst(pass, count)");
    for r in &runs {
        println!("{:5}..{:5} {:6} {:5}  {} at {}", r.0, r.1, r.1 - r.0 + 1, r.4, r.3, r.2);
    }
    println!("text mode in the original: {} records", orig.iter().filter(|o| o.1.text).count());
}
