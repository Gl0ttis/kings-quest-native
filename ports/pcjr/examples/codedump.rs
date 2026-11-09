//! codedump DISK OUT: the engine's code segment (segment 0060h, as loaded) to a file,
//! for searching and disassembling.

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let disk = kq1_pcjr::disk::Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    std::fs::write(&a[1], disk.code_segment()).expect("write");
}
