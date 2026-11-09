//! primap DISK ROOM [X1 X2 Y1 Y2]: a room picture's priorities (hex), one row per line,
//! for reading where the ego can walk.
use kq1_pcjr::{disk::Disk, pic, vocab};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let disk = Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let r: usize = a[1].parse().expect("room");
    let n = |i: usize, d: usize| a.get(i).map_or(d, |v| v.parse().expect("number"));
    let (x1, x2, y1, y2) = (n(2, 0), n(3, 159), n(4, 0), n(5, 167));
    let cs = disk.code_segment();
    let p = pic::draw_room(&cs[vocab::DATA_SEGMENT + pic::FRAME..], disk.room(r).expect("room").picture);
    for y in y1..=y2 {
        let row: String = (x1..=x2).map(|x| format!("{:X}", p.buf[y * 160 + x] >> 4)).collect();
        println!("{y:3} {row}");
    }
}
