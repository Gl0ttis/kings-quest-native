//! nibdecode <in.nib>... --out <dir>
//! Validates every sector, writes .do/.po images, and prints a report.
use kq1_apple2::{dos33, nib};
use std::{env, fs, path::PathBuf};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut out = PathBuf::from("extracted");
    if let Some(p) = args.iter().position(|a| a == "--out") {
        out = PathBuf::from(&args[p + 1]);
        args.drain(p..p + 2);
    }
    fs::create_dir_all(&out).unwrap();
    let mut failures = 0;
    for path in &args {
        let raw = fs::read(path).expect("read nib");
        let stem = PathBuf::from(path).file_stem().unwrap().to_string_lossy().to_string();
        let tracks = match nib::decode_image(&raw) {
            Ok(t) => t,
            Err(e) => {
                println!("{path}: {e}");
                failures += 1;
                continue;
            }
        };
        let total: usize = tracks.iter().map(|t| t.sectors.len()).sum();
        let addr_ok = tracks.iter().flat_map(|t| &t.sectors).filter(|s| s.addr_checksum_ok).count();
        let data_ok = tracks.iter().flat_map(|t| &t.sectors).filter(|s| s.data_checksum_ok).count();
        let mut vols: Vec<u8> = tracks.iter().flat_map(|t| t.sectors.iter().map(|s| s.volume)).collect();
        vols.sort();
        vols.dedup();
        let nonzero = tracks.iter().flat_map(|t| &t.sectors).filter(|s| s.data.iter().any(|&b| b != 0)).count();
        println!(
            "== {stem}: sectors={total} addr_ok={addr_ok} data_ok={data_ok} volumes={vols:?} nonzero_sectors={nonzero}"
        );
        for t in &tracks {
            for p in &t.problems {
                println!("   {p}");
                failures += 1;
            }
        }
        let used: Vec<usize> = tracks
            .iter()
            .filter(|t| t.sectors.iter().any(|s| s.data.iter().any(|&b| b != 0)))
            .map(|t| t.index)
            .collect();
        println!("   tracks with data: {used:?}");
        let dsk = nib::to_ordered(&tracks, &nib::DOS_TO_PHYS);
        let po = nib::to_ordered(&tracks, &nib::PRODOS_TO_PHYS);
        fs::write(out.join(format!("{stem}.do")), &dsk).unwrap();
        fs::write(out.join(format!("{stem}.po")), &po).unwrap();
        match dos33::catalog(&dsk) {
            Some(cat) => {
                println!("   DOS 3.3 catalog ({} entries):", cat.len());
                let fdir = out.join(&stem);
                fs::create_dir_all(&fdir).unwrap();
                for e in &cat {
                    let data = dos33::read_file(&dsk, e);
                    println!(
                        "     {}{} {:03} {:<30} T{:02}S{:X} {} bytes",
                        if e.locked { '*' } else { ' ' },
                        e.kind,
                        e.sectors,
                        e.name,
                        e.ts_track,
                        e.ts_sector,
                        data.len()
                    );
                    let safe: String = e
                        .name
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
                        .collect();
                    fs::write(fdir.join(format!("{safe}.{}", e.kind)), data).unwrap();
                }
            }
            None => {
                let v = dos33::sector(&dsk, 17, 0);
                if v[0x27] == 122 && v[0x34] == 35 {
                    println!("   DOS 3.3 VTOC present (catalog T{}S{:X}, vol {}), but catalog chain is not valid: custom layout", v[1], v[2], v[6]);
                }
                let b = &po[2 * 512..3 * 512];
                if b[0x04] >> 4 == 0xF {
                    let n = (b[0x04] & 0xF) as usize;
                    println!("   ProDOS volume header: {}", String::from_utf8_lossy(&b[5..5 + n]));
                } else {
                    println!("   no well-formed DOS 3.3 catalog or ProDOS volume header");
                }
            }
        }
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
