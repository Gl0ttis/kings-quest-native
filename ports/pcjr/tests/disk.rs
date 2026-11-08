//! The disk layout against the user's image. Skipped when original/ holds no 360K image.

use kq1_pcjr::disk::{Disk, IMAGE_LEN, ROOMS, VIEWS};

fn disk() -> Option<Disk> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    let img = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| std::fs::read(e.ok()?.path()).ok())
        .find(|b| b.len() == IMAGE_LEN)?;
    Some(Disk::new(img).expect("the game disk"))
}

#[test]
fn every_room_picture_draws_cleanly() {
    let Some(d) = disk() else {
        eprintln!("skipped: no disk image");
        return;
    };
    let mut drawn = 0;
    for r in 1..ROOMS {
        let room = d.room(r).unwrap();
        // Room 64's script header is wrong in the original data (as on the Apple disks),
        // so its picture doesn't start where the header says; rooms 81-84 are empty.
        if r == 64 || room.script.len() <= 8 {
            continue;
        }
        let p = kq1_apple2::pic::draw(room.picture);
        assert!(p.warnings.is_empty(), "room {r}: {:?}", p.warnings);
        assert_eq!(room.picture[p.end - 1], 0xFF, "room {r}: no terminator");
        drawn += 1;
    }
    assert_eq!(drawn, 79);
}

#[test]
fn the_engine_starts_with_its_entry_code() {
    let Some(d) = disk() else {
        eprintln!("skipped: no disk image");
        return;
    };
    // $50D: XOR DX,DX; MOV DS,DX (set the diskette-parameter vector).
    let cs = d.code_segment();
    assert_eq!(&cs[0x50D..0x511], &[0x33, 0xD2, 0x8E, 0xDA]);
    // The ego (view 0): 4 loops of 8 cels.
    let v = d.view(0).unwrap();
    assert_eq!(v[2], 4);
    assert!((0..VIEWS).filter_map(|v| d.view(v)).count() > 90);
}
