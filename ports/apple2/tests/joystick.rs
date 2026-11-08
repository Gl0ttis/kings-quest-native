//! The joystick through the original game logic ($4817 calibration, $4846 each pass).
//! Needs the decoded disk images in extracted/; skipped when they are absent.
use kq1_apple2::engine::{f, IoImpl, Joystick, Resources, World};
use std::{cell::Cell, fs, rc::Rc};

struct Stick(Rc<Cell<Joystick>>);
impl IoImpl for Stick {
    fn wait_key(&mut self, _f: &[u32]) -> u8 {
        0x0D
    }
    fn bell(&mut self) {}
    fn save(&mut self, _: u8, _: &[u8]) -> Result<(), String> {
        Err("no saves".into())
    }
    fn load(&mut self, _: u8) -> Option<Vec<u8>> {
        None
    }
    fn joystick(&mut self) -> Joystick {
        self.0.get()
    }
}

#[test]
fn stick_walks_graham_and_buttons_reach_the_scripts() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/");
    let Some(imgs) = (1..=3).map(|n| fs::read(format!("{base}disk{n}.do")).ok()).collect::<Option<Vec<_>>>() else {
        return;
    };
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap());
    let stick = Rc::new(Cell::new(Joystick { paddles: Some((128, 128)), buttons: [false; 2] }));
    w.io = Some(Box::new(Stick(stick.clone())));
    w.boot(); // calibrates at rest: thresholds 64 and 192
    assert_eq!(w.g.joy_x, (64, 192));
    for _ in 0..20 {
        w.cycle();
    }
    let x0 = w.actors[0][f::X];
    // Hard right: direction 3 while held; the ego walks right.
    stick.set(Joystick { paddles: Some((250, 128)), buttons: [false; 2] });
    for _ in 0..10 {
        w.cycle();
    }
    assert_eq!(w.g.joy_dir, 3);
    assert!(w.actors[0][f::X] > x0 + 5, "ego moved right: {} -> {}", x0, w.actors[0][f::X]);
    // Back to rest: no direction, the ego stops.
    stick.set(Joystick { paddles: Some((128, 128)), buttons: [false; 2] });
    w.cycle();
    w.cycle();
    let x1 = w.actors[0][f::X];
    w.cycle();
    assert_eq!(w.g.joy_dir, 0);
    assert_eq!(w.actors[0][f::X], x1);
    // Button 0 (Open-Apple): v16, which the global script turns into a swim (v27).
    stick.set(Joystick { paddles: Some((128, 128)), buttons: [true, false] });
    let seen = w.log.len();
    w.cycle();
    stick.set(Joystick { paddles: Some((128, 128)), buttons: [false; 2] });
    w.cycle();
    assert!(w.log[seen..].iter().any(|l| l.contains("swim")), "{:?}", &w.log[seen..]);
}

#[test]
fn no_joystick_reads_zero_and_stays_off() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/");
    let Some(imgs) = (1..=3).map(|n| fs::read(format!("{base}disk{n}.do")).ok()).collect::<Option<Vec<_>>>() else {
        return;
    };
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap());
    let stick = Rc::new(Cell::new(Joystick::default()));
    w.io = Some(Box::new(Stick(stick.clone())));
    w.boot();
    assert_eq!(w.g.joy_x.1, 0);
    // Plugged in later: still off until the game is started again, as on the Apple.
    stick.set(Joystick { paddles: Some((250, 128)), buttons: [false; 2] });
    w.cycle();
    assert_eq!(w.g.joy_dir, 0);
}
