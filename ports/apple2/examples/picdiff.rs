// picdiff PIC ORIG.bin: draw picture PIC with the port and compare it with a refemu
// --picbuf dump (aux $4000, 140x160, priority<<4 | visual).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let res = kq1_apple2::engine::Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap();
    let n: usize = a[1].parse().unwrap();
    let data = res.pictures[n].clone().unwrap();
    let p = kq1_apple2::pic::draw(&data);
    let o = std::fs::read(&a[2]).unwrap();
    let mut diffs = 0;
    for i in 0..p.buf.len() {
        if p.buf[i] != o[i] {
            diffs += 1;
            if diffs <= 20 {
                println!("x {:3} y {:3}: port {:02X} orig {:02X}", i % 140, i / 140, p.buf[i], o[i]);
            }
        }
    }
    println!("{diffs} bytes differ");
    for (x, y) in [(0usize, 51usize), (1, 51), (21, 108), (93, 31)] {
        let r = |b: &[u8]| {
            (y.saturating_sub(1)..=y + 1)
                .map(|yy| {
                    (x.saturating_sub(1)..=x + 1)
                        .map(|xx| format!("{:02X}", b[yy * 140 + xx]))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };
        println!("around ({x},{y}) port: {}\n               orig: {}", r(&p.buf), r(&o));
    }
}
