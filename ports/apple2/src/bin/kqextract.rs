//! kqextract <disk1.do> <disk2.do> <disk3.do> --out <dir>
//! Extracts the engine image, fixed startup resources, and every directory entry
//! using the engine's own addressing rules, then checks that no two resources overlap.
use kq1_apple2::kq::{self, DiskAddr, Disks};
use std::{env, fs, path::PathBuf};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut out = PathBuf::from("extracted/res");
    if let Some(p) = args.iter().position(|a| a == "--out") {
        out = PathBuf::from(&args[p + 1]);
        args.drain(p..p + 2);
    }
    if args.len() != 3 {
        eprintln!("usage: kqextract disk1.do disk2.do disk3.do [--out dir]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("engine_4000.bin"), disks.engine()).unwrap();

    let mut manifest = String::new();
    // (disk, span, label) for overlap checking; shared (disk 0) resources are checked on disk 1.
    let mut spans: Vec<(u8, std::ops::Range<usize>, String)> = Vec::new();
    let mut errors = 0;

    let fdir = out.join("fixed");
    fs::create_dir_all(&fdir).unwrap();
    for (rec, name) in kq::FIXED {
        let r = disks.fixed(rec).expect("fixed resource");
        fs::write(fdir.join(format!("{name}.bin")), r.data).unwrap();
        manifest += &format!("fixed {name:<20} rec=${rec:04X} {} len={}\n", fmt(r.addr), r.data.len());
        spans.push((r.disk, r.span.clone(), format!("fixed/{name}")));
    }

    for (page, kind) in kq::DIRS {
        let d = out.join(kind);
        fs::create_dir_all(&d).unwrap();
        let (mut ok, mut empty, mut bad) = (0, 0, 0);
        for (i, rec) in disks.directory(page).unwrap().iter().enumerate() {
            if kq::UNUSED.contains(&(kind, i)) {
                manifest += &format!("{kind:5} {i:3} -- unused slot ({})\n", hex(rec));
                continue;
            }
            if rec[0] == 0 {
                // Engine $4264 treats track 0 as "not present".
                empty += 1;
                manifest += &format!("{kind:5} {i:3} -- empty ({})\n", hex(rec));
                continue;
            }
            let a = DiskAddr::from_record(rec);
            match disks.read(a, 1) {
                Ok(r) => {
                    ok += 1;
                    fs::write(d.join(format!("{i:03}.bin")), r.data).unwrap();
                    manifest += &format!("{kind:5} {i:3} {} len={}\n", fmt(a), r.data.len());
                    spans.push((r.disk, r.span.clone(), format!("{kind}/{i:03}")));
                }
                Err(e) => {
                    bad += 1;
                    manifest += &format!("{kind:5} {i:3} !! invalid ({}): {e}\n", hex(rec));
                }
            }
        }
        println!("{kind:5} (${page:04X}): {ok} valid, {empty} empty, {bad} invalid");
    }

    // Overlap check: a correct reading of the format should yield disjoint spans.
    spans.sort_by_key(|(d, s, _)| (*d, s.start));
    for w in spans.windows(2) {
        let ((d1, s1, l1), (d2, s2, l2)) = (&w[0], &w[1]);
        if d1 == d2 && s2.start < s1.end {
            println!("OVERLAP disk{d1}: {l1} {s1:?} vs {l2} {s2:?}");
            errors += 1;
        }
    }
    fs::write(out.join("manifest.txt"), manifest).unwrap();
    println!("{} resources extracted, {errors} overlaps", spans.len());
    std::process::exit(if errors == 0 { 0 } else { 1 });
}

fn fmt(a: DiskAddr) -> String {
    format!("d{} T{:02} S{:X} +{:02X}", a.disk, a.track, a.sector, a.offset)
}

fn hex(r: &[u8; 3]) -> String {
    r.iter().map(|b| format!("{b:02x}")).collect()
}
