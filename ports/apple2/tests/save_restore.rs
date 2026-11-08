//! End-to-end save/restore through the original game logic. Needs the decoded disk
//! images in extracted/ (from nibdecode); skipped when they are absent.
use kq1_apple2::engine::{f, Resources, Scripted, World};
use kq1_apple2::kq::Disks;
use std::fs;

fn world() -> Option<World> {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/");
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{base}disk{n}.do")).ok()).collect();
    let disks = Disks::new(imgs?);
    Some(World::new(Resources::load(&disks, false).ok()?))
}

fn type_line(w: &mut World, s: &str) {
    for c in s.bytes() {
        w.key(c);
    }
    w.key(0x0D);
    w.cycle();
}

#[test]
fn save_then_restore_round_trip() {
    let Some(mut w) = world() else { return };
    // Letter prompts answered with 'a' (save), then 'a' (restore).
    w.io = Some(Box::new(Scripted { keys: b"aa".iter().copied().collect(), ..Default::default() }));
    w.enter_room(6, 8);
    w.press_direction(7);
    for _ in 0..100 {
        w.cycle();
    }
    w.press_direction(7); // stop
    for _ in 0..3 {
        w.cycle();
    }
    type_line(&mut w, "get pouch");
    assert_eq!(w.vars[19], 3, "taking the pouch scores 3");
    let pos = (w.actors[0][f::X], w.actors[0][f::Y]);

    w.key(b'1'); // "save game"
    w.cycle();
    assert!(w.log.iter().all(|l| !l.contains("unimplemented")));

    // Change the state: walk away, lose the points and the pouch.
    w.press_direction(3);
    for _ in 0..40 {
        w.cycle();
    }
    w.vars[19] = 0;
    w.objects[12][2] &= 0x7F;
    assert_ne!((w.actors[0][f::X], w.actors[0][f::Y]), pos);

    w.key(b'3'); // "restore game"
    for _ in 0..4 {
        w.cycle();
    }
    assert_eq!(w.vars[19], 3, "score restored");
    assert!(w.objects[12][2] & 0x80 != 0, "pouch carried again");
    assert_eq!(w.vars[1], 6, "back in room 6");
    assert_eq!(w.vars[25], 1, "room scripts set the room up again");
    assert_eq!((w.actors[0][f::X], w.actors[0][f::Y]), pos, "ego where it was saved");
}

#[test]
fn restore_of_empty_slot_reports_it() {
    let Some(mut w) = world() else { return };
    // Restore slot 'q' (empty), then Esc out of "try another letter".
    w.io = Some(Box::new(Scripted { keys: [b'q', 0x1B].into_iter().collect(), ..Default::default() }));
    w.enter_room(1, 8);
    w.key(b'3');
    for _ in 0..3 {
        w.cycle();
    }
    let rows: Vec<String> = w.text.hires[20..24].iter().map(|r| String::from_utf8_lossy(r).into_owned()).collect();
    assert!(rows.iter().any(|r| r.contains("Press ESC to quit")), "{rows:?}");
    assert_eq!(w.vars[1], 1);
}

#[test]
fn restart_starts_a_fresh_game() {
    let Some(mut w) = world() else { return };
    w.io = Some(Box::new(Scripted { keys: b"y".iter().copied().collect(), ..Default::default() }));
    w.enter_room(6, 8);
    w.vars[19] = 42;
    w.key(b'5'); // "restart game" -> "Do you want to start over? (Y or N)" -> y
    for _ in 0..4 {
        w.cycle();
    }
    assert_eq!(w.vars[19], 0, "score reset");
    assert_eq!(w.vars[1], 1, "back at the first room");
    assert_eq!(w.vars[25], 1, "first room set up by its scripts");
}
