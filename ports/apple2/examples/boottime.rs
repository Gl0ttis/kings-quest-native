// boottime: the start-up's timeline as a real-time front end would show it.
use kq1_apple2::engine::{cost::PicDraw, IoImpl, Resources, World};
use std::{cell::RefCell, rc::Rc};
struct T(Rc<RefCell<Vec<(String, f64)>>>);
impl IoImpl for T {
    fn wait_key(&mut self, _: &[u32]) -> u8 {
        0x0D
    }
    fn wait_key_timeout(&mut self, _: &[u32], s: f64) -> Option<u8> {
        self.0.borrow_mut().push(("title wait".into(), s));
        None
    }
    fn pause(&mut self, f: &[u32], s: f64, _l: bool) {
        if self.0.borrow().len() == 1 {
            // The "Please wait..." screen.
            let rgb: Vec<u8> = f.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
            if let Ok(out) = std::env::var("SHOT") {
                std::fs::write(out, kq1_apple2::png::encode_rgb(560, 192, &rgb)).unwrap();
            }
        }
        self.0.borrow_mut().push(("pause".into(), s));
    }
    fn show_drawing(&mut self, d: &PicDraw, _: &dyn Fn(u32) -> Vec<u32>) {
        let (a, b) = d.span(false);
        self.0.borrow_mut().push((format!("picture (load {a:.2} s)"), b));
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
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(T(log.clone())));
    w.boot();
    let mut t = 0.0;
    for (what, s) in log.borrow().iter() {
        t += s;
        println!("{what:28} {s:6.2} s   -> {t:6.2} s");
    }
}
