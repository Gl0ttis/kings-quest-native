//! The disk layout against the user's images in original/: the original as an HxC
//! `.mfm` dump and/or the cracked 360K `.img`. Skipped when there are none.

use kq1_formats::script::{Mode, Script};
use kq1_pc::disk::{Disk, ROOMS, SOUNDS, VIEWS};
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
fn every_resource_decodes() {
    for (path, d) in images() {
        let cs = d.code_segment();
        let (mut scripts, mut drawn, mut views, mut sounds) = (0, 0, 0, 0);
        for r in 0..ROOMS {
            let room = d.room(r).unwrap();
            // Room 64's script header is wrong in the original data (as on every
            // release's disk); rooms 81-84 are empty.
            if r == 64 || room.script.len() <= 8 {
                continue;
            }
            let s = Script::new(room.script).unwrap();
            s.actions_with(&kq1_pc::script::OPERANDS).unwrap_or_else(|e| panic!("{path:?} room {r}: {e}"));
            s.rules(Mode::Cycle).unwrap();
            s.rules(Mode::Parser).unwrap();
            scripts += 1;
            if r == 0 {
                continue;
            }
            let p = kq1_pc::pic::draw_room(&cs[kq1_pc::vocab::DATA_SEGMENT + kq1_pc::pic::FRAME..], room.picture);
            assert!(p.warnings.is_empty(), "{path:?} room {r}: {:?}", p.warnings);
            assert_eq!(room.picture[p.end - 1], 0xFF, "{path:?} room {r}: no terminator");
            drawn += 1;
        }
        for v in 0..VIEWS {
            let data = d.view(v).unwrap();
            if data.len() > 3 {
                kq1_pc::view::decode(data).unwrap_or_else(|e| panic!("{path:?} view {v}: {e}"));
                views += 1;
            }
        }
        for s in 0..SOUNDS {
            sounds += kq1_pc::sound::parse(d.sound(s).unwrap()).unwrap().iter().flatten().count();
        }
        assert_eq!((scripts, drawn, views, sounds), (80, 79, 88, 40), "{path:?}");
        assert_eq!(kq1_pc::vocab::read(&cs).unwrap().len(), 369, "{path:?}");
    }
}

/// The crack's disk holds the original's game: the same resource table and resources,
/// and the engine but for the original's last 773 bytes (the protection's code, which
/// its header starts at).
#[test]
fn the_original_and_the_crack_hold_the_same_game() {
    let images = images();
    let (Some(orig), Some(crack)) = (images.iter().find(|i| i.1.original), images.iter().find(|i| !i.1.original))
    else {
        return;
    };
    let (o, c) = (&orig.1, &crack.1);
    assert_eq!(o.entries(), c.entries());
    for (_, _, e) in o.entries() {
        assert_eq!(o.resource(e), c.resource(e));
    }
    assert_eq!(&o.engine[..c.engine.len()], &c.engine[..]);
    assert_eq!(o.read_file(&o.file("290K").unwrap()), c.read_file(&c.file("290K").unwrap()));
}

/// The protection: the original's dump has exactly one unreadable sector, track 6
/// side 0 sector 1, and its readable sectors are a whole game.
#[test]
fn the_original_has_one_unreadable_sector() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for p in rd.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|e| e == "mfm")) {
        let (tracks, bad) = kq1_formats::mfm::decode_with_errors(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(bad.len(), 1, "{p:?}");
        assert_eq!((bad[0].cylinder, bad[0].side, bad[0].record), kq1_pc::disk::PROTECTION);
        // Its ID field is the unreadable part: a 256-byte sector's, with a bad CRC.
        assert!(!bad[0].data);
        assert_eq!(bad[0].size, 1);
        assert!(Disk::from_tracks(&tracks).unwrap().original);
    }
}
