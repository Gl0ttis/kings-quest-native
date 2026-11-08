// textshot KEYS PASS DUMP OUT.png: replay to PASS, keep the frame shown at the last key
// wait (the text screen), compare the text page with a refemu REFEMU_TEXTDUMP file.
use kq1_apple2::engine::{IoImpl, Resources, World};
use std::{cell::RefCell, rc::Rc};
struct Grab(Rc<RefCell<Vec<u32>>>);
impl IoImpl for Grab {
    fn wait_key(&mut self, f: &[u32]) -> u8 {
        *self.0.borrow_mut() = f.to_vec();
        0x0D
    }
    fn bell(&mut self) {}
    fn save(&mut self, _: u8, _: &[u8]) -> Result<(), String> {
        Err(String::new())
    }
    fn load(&mut self, _: u8) -> Option<Vec<u8>> {
        None
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let keys: std::collections::HashMap<usize, u8> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let pass: usize = a[2].parse().unwrap();
    let grab = Rc::new(RefCell::new(Vec::new()));
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Grab(grab.clone())));
    if let Ok(r) = std::env::var("CHARROM") {
        w.display.charrom =
            Some(std::rc::Rc::new(kq1_apple2::engine::display::CharRom::from_rom(&std::fs::read(r).unwrap()).unwrap()));
    }
    w.boot();
    for p in 1..=pass {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
    }
    let dump: Vec<Vec<u8>> = std::fs::read_to_string(&a[3])
        .unwrap()
        .lines()
        .map(|l| l.split(' ').map(|h| u8::from_str_radix(h, 16).unwrap()).collect())
        .collect();
    let mut diff = 0;
    for r in 0..23 {
        for c in 0..40 {
            if w.text.page[r][c] != dump[r][c] && dump[r][c] != 0 {
                diff += 1;
                if diff < 10 {
                    println!("row {r} col {c}: port {:02X} orig {:02X}", w.text.page[r][c], dump[r][c]);
                }
            }
        }
    }
    println!("{diff} cells differ in rows 0-22 (ignoring refemu's 00 cells)");
    let f = grab.borrow();
    let rgb: Vec<u8> = f.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    std::fs::write(&a[4], kq1_apple2::png::encode_rgb(560, 192, &rgb)).unwrap();
}
