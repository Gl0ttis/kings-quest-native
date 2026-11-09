//! scriptdump DISK OUTDIR: every room's script as text (kq1_pcjr::script::listing), for
//! reading the scripts and writing routes.

use kq1_pcjr::disk::{Disk, ROOMS};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 2 {
        eprintln!("usage: scriptdump DISK OUTDIR");
        std::process::exit(2);
    }
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    std::fs::create_dir_all(&a[1]).expect("out");
    for r in 0..ROOMS {
        if let Some(t) = kq1_pcjr::script::listing(&disk, r) {
            std::fs::write(format!("{}/{r:03}.txt", a[1]), t).expect("write");
        }
    }
}
