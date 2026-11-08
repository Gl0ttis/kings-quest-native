fn main() {
    let a: Vec<String> = std::env::args().collect();
    let disks = kq1_apple2::kq::Disks::new(a[1..4].iter().map(|p| std::fs::read(p).unwrap()).collect());
    let res = kq1_apple2::engine::Resources::load(&disks, true).expect("res");
    let v: usize = a[4].parse().unwrap();
    let view = res.views[v].as_ref().expect("view");
    for (i, l) in view.loops.iter().enumerate() {
        let w: Vec<String> = l.cels.iter().map(|c| format!("{}x{}", c.width, c.height)).collect();
        println!("loop {i}: {}", w.join(" "));
    }
}
