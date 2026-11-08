// nibcheck A B C: load the disks with Disks::from_files and compare them with extracted/disk1-3.do.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let d = kq1_apple2::kq::Disks::from_files(&a).unwrap();
    for n in 0..3 {
        let want = std::fs::read(format!("extracted/disk{}.do", n + 1)).unwrap();
        println!("disk {}: {}", n + 1, if d.images[n] == want { "identical to extracted .do" } else { "DIFFERENT" });
    }
}
