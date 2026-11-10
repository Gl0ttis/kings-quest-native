// primap-apple2 PIC [Y0 Y1 STEP]: print a picture's priority map as hex digits, one row per line.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let res = kq1_apple2::engine::Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap();
    let p = kq1_apple2::pic::draw(&res.pictures[a[1].parse::<usize>().unwrap()].clone().unwrap());
    let (y0, y1, st): (usize, usize, usize) = (
        a.get(2).map_or(0, |v| v.parse().unwrap()),
        a.get(3).map_or(160, |v| v.parse().unwrap()),
        a.get(4).map_or(1, |v| v.parse().unwrap()),
    );
    for y in (y0..y1).step_by(st) {
        let row: String = (0..140).map(|x| format!("{:X}", p.priority(x, y))).collect();
        println!("y{:3} {row}", y + 3);
    }
}
