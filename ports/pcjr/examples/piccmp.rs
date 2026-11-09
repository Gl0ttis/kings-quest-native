//! piccmp DISK ENGINEPICS: draw every room picture with the PCjr painter and compare it
//! byte for byte with the buffers the original engine drew (refemu-pcjr --pictures).
use kq1_pcjr::{disk::Disk, pic, vocab};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let disk = Disk::from_file(std::fs::read(&a[1]).unwrap()).unwrap();
    let cs = disk.code_segment();
    let frame = &cs[vocab::DATA_SEGMENT + pic::FRAME..];
    let (mut same, mut total) = (0, 0);
    for r in 1..kq1_pcjr::disk::ROOMS {
        let Ok(want) = std::fs::read(format!("{}/{r:03}.bin", a[2])) else { continue };
        let room = disk.room(r).unwrap();
        let p = pic::draw_room(frame, room.picture);
        total += 1;
        let diff: Vec<usize> = (0..want.len()).filter(|&i| want[i] != p.buf[i]).collect();
        if diff.is_empty() {
            same += 1;
        } else {
            let i = diff[0];
            println!(
                "room {r:2}: {} bytes differ; first at x {} y {}: engine {:02X} painter {:02X}{}",
                diff.len(),
                i % 160,
                i / 160,
                want[i],
                p.buf[i],
                if p.warnings.is_empty() { String::new() } else { format!(" {:?}", p.warnings) }
            );
        }
    }
    println!("{same} of {total} pictures identical");
}
