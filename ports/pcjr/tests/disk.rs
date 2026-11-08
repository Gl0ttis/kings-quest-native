//! The disk layout against the user's images in original/: the original as an HxC
//! `.mfm` dump and/or the cracked 360K `.img`. Skipped when there are none.

use kq1_pcjr::disk::{Disk, ROOMS, VIEWS};
use std::path::PathBuf;

fn images() -> Vec<(PathBuf, Disk)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(PathBuf, Disk)> = rd
        .filter_map(|e| {
            let p = e.ok()?.path();
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            (ext == "mfm" || ext == "img").then_some(())?;
            let d = Disk::from_file(std::fs::read(&p).ok()?).expect("the game disk");
            Some((p, d))
        })
        .collect();
    v.sort_by_key(|(_, d)| !d.original);
    v
}

#[test]
fn every_room_picture_and_view_decodes() {
    for (path, d) in images() {
        let mut drawn = 0;
        for r in 1..ROOMS {
            let room = d.room(r).unwrap();
            // Room 64's script header is wrong in the original data (as on the Apple
            // disks); rooms 81-84 are empty.
            if r == 64 || room.script.len() <= 8 {
                continue;
            }
            let p = kq1_apple2::pic::draw(room.picture);
            assert!(p.warnings.is_empty(), "{path:?} room {r}: {:?}", p.warnings);
            assert_eq!(room.picture[p.end - 1], 0xFF, "{path:?} room {r}: no terminator");
            drawn += 1;
        }
        assert_eq!(drawn, 79, "{path:?}");
        let views = (0..VIEWS).filter_map(|v| d.view(v)).filter(|v| v.len() > 3); // empty views are `01 00 FF`
        let mut n = 0;
        for v in views {
            kq1_pcjr::view::decode(v).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            n += 1;
        }
        assert_eq!(n, 88, "{path:?}");
    }
}

#[test]
fn every_script_parses_with_the_pcjr_operands() {
    for (path, d) in images() {
        for r in 0..ROOMS {
            if r == 64 {
                continue;
            }
            let room = d.room(r).unwrap();
            let s = kq1_apple2::script::Script::new(room.script).unwrap();
            s.actions_with(&kq1_pcjr::script::OPERANDS).unwrap_or_else(|e| panic!("{path:?} room {r}: {e}"));
        }
    }
}

#[test]
fn every_sound_and_the_vocabulary_decode() {
    for (path, d) in images() {
        let mut sounds = 0;
        for s in 0..kq1_pcjr::disk::SOUNDS {
            let four = kq1_pcjr::sound::parse(d.sound(s).unwrap()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            sounds += four.iter().flatten().count();
        }
        assert_eq!(sounds, 40, "{path:?}");
        let words = kq1_pcjr::vocab::read(&d.code_segment()).unwrap();
        assert_eq!(words.len(), 358, "{path:?}");
        assert!(words.iter().any(|w| w.word == "dagger"));
    }
}

#[test]
fn the_original_and_the_crack_hold_the_same_game() {
    let imgs = images();
    let (Some(orig), Some(crack)) = (imgs.iter().find(|x| x.1.original), imgs.iter().find(|x| !x.1.original)) else {
        eprintln!("skipped: needs both the .mfm original and the .img crack");
        return;
    };
    let (o, c) = (&orig.1, &crack.1);
    assert_eq!(o.engine, c.engine, "the engine");
    // The boot sectors differ only in the crack's loading code, before the table.
    assert_eq!(o.code_segment()[0x100..], c.code_segment()[0x100..]);
    for (kind, n, e) in o.entries() {
        assert_eq!(o.resource(e), c.resource(e), "{kind} {n}");
    }
    // $50D: XOR DX,DX; MOV DS,DX (the engine's entry).
    assert_eq!(&o.code_segment()[0x50D..0x511], &[0x33, 0xD2, 0x8E, 0xDA]);
}
