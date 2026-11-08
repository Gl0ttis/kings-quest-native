// savedump DISK SLOT: print the key fields of a save on a save-disk image.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kq1_apple2::savedisk::SaveDisk::open(std::fs::read(&a[1]).unwrap()).unwrap();
    let img = d.load_game(a[2].parse().unwrap()).expect("no save");
    let zp = &img[8 + 0x500 + 0x7C..][..0x90];
    let vars = &img[8 + 0x500 + 0x7C + 0x90..];
    println!("v0-v31 (zp $D0): {:?}", &zp[0x70..0x90]);
    println!("v32..: {:?}", &vars[0x20..0x40]);
    println!("actor0: {:?}", &img[8 + 0x500..8 + 0x500 + 31]);
}
