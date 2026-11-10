//! The disk layout against the user's images in original/: the original as an HxC
//! `.mfm` dump and/or the cracked 360K `.img`. Skipped when there are none.

use kq1_formats::script::{Mode, Script};
use kq1_pc::disk::{Disk, Edition, ROOMS, SOUNDS, VIEWS};
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

/// What the game reads of a disk: each room's script and picture, each view, each
/// sound resource (not the whole sectors the table gives each, whose ends hold
/// leftovers: on the second edition's original, the protection's sector).
fn game(d: &Disk) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    for r in 0..ROOMS {
        let room = d.room(r).unwrap();
        v.push((format!("room {r} script"), room.script.to_vec()));
        v.push((format!("room {r} picture"), room.picture.to_vec()));
    }
    for n in 0..VIEWS {
        v.push((format!("view {n}"), d.view(n).unwrap_or_default().to_vec()));
    }
    for n in 0..SOUNDS {
        v.push((format!("sound {n}"), d.sound(n).unwrap_or_default().to_vec()));
    }
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
            let frame = d.edition.data_segment() + d.edition.ds(kq1_pc::pic::FRAME).unwrap();
            let p = kq1_pc::pic::draw_room(&cs[frame..], room.picture);
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
        assert_eq!(kq1_pc::vocab::read(&cs, d.edition).unwrap().len(), 369, "{path:?}");
        // The colour patterns are the engine's.
        let t = d.edition.data_segment() + d.edition.ds(kq1_pc::composite::PATTERN_TABLE).unwrap();
        let doubled: Vec<u8> = kq1_pc::composite::PATTERN.iter().map(|p| p << 4 | p).collect();
        assert_eq!(&cs[t..t + 16], &doubled[..], "{path:?}");
    }
}

/// Each edition's crack holds its original's game: the same resource table and
/// resources, and the engine (the first release's original has 773 bytes more at its
/// end, the protection's code, which its header starts at; the second's crack changes
/// only the header's start).
#[test]
fn the_original_and_the_crack_hold_the_same_game() {
    let images = images();
    for edition in [Edition::Cga, Edition::CgaRgb] {
        let of = |original: bool| images.iter().find(|i| i.1.edition == edition && i.1.original == original);
        let (Some(orig), Some(crack)) = (of(true), of(false)) else { continue };
        let (o, c) = (&orig.1, &crack.1);
        assert_eq!(o.entries(), c.entries());
        assert_eq!(game(o), game(c));
        assert_eq!(&o.engine[..c.engine.len()], &c.engine[..]);
        assert_eq!(o.read_file(&o.file("290K").unwrap()), c.read_file(&c.file("290K").unwrap()));
    }
}

/// The two editions hold the same game but for three bytes of script, one each in rooms
/// 18, 21 and 65 (the second release's fixes), and the second's data lies 57 sectors
/// lower on its disk.
#[test]
fn the_editions_differ_in_three_bytes_of_script() {
    let images = images();
    let (Some(a), Some(b)) =
        (images.iter().find(|i| i.1.edition == Edition::Cga), images.iter().find(|i| i.1.edition == Edition::CgaRgb))
    else {
        return;
    };
    let (a, b) = (&a.1, &b.1);
    let (ea, eb) = (a.entries(), b.entries());
    assert_eq!(ea.len(), eb.len());
    for ((ka, na, x), (kb, nb, y)) in ea.iter().zip(&eb) {
        assert_eq!((ka, na), (kb, nb));
        assert_eq!((x.offset, x.sectors), (y.offset, y.sectors), "{ka} {na}");
        assert_eq!(x.sector, y.sector + 57, "{ka} {na}");
    }
    let mut changed = Vec::new();
    for ((what, ra), (_, rb)) in game(a).iter().zip(game(b)) {
        assert_eq!(ra.len(), rb.len(), "{what}");
        let n = ra.iter().zip(&rb).filter(|(p, q)| p != q).count();
        if n > 0 {
            changed.push((what.clone(), n));
        }
    }
    let script = |r: usize| (format!("room {r} script"), 1);
    assert_eq!(changed, [script(18), script(21), script(65)]);
}

/// The protection: the original's dump has exactly one unreadable sector, track 6
/// side 0 sector 1, and its readable sectors are a whole game. In the first release
/// the sector's ID field is the unreadable part (a 256-byte sector's, with a bad CRC);
/// in the second (Copylock) its data.
#[test]
fn the_original_has_one_unreadable_sector() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for p in rd.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|e| e == "mfm")) {
        let (tracks, bad) = kq1_formats::mfm::decode_with_errors(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(bad.len(), 1, "{p:?}");
        assert_eq!((bad[0].cylinder, bad[0].side, bad[0].record), kq1_pc::disk::PROTECTION);
        assert_eq!(bad[0].size, 1);
        let d = Disk::from_tracks(&tracks).unwrap();
        assert!(d.original);
        assert_eq!(bad[0].data, d.edition == Edition::CgaRgb, "{p:?}");
    }
}
