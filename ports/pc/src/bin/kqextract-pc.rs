//! kqextract-pc DISK [OUT]: extract every resource from the PC game disk (the original's
//! `.mfm` dump or the cracked `.img`) into OUT (default `extracted/`, which git
//! ignores: it is game content), and check each one with the shared decoders:
//!
//! - the disk's files (`files/`), the engine as it runs (`res/engine.bin`, the resource
//!   table read over it) and the table (`res/manifest.txt`);
//! - rooms: the script (actions, cycle rules, parser rules, messages) and the picture
//!   (drawn with the PCjr port's painter, and rendered to OUT/pics/NNN.png in the IBM
//!   colours);
//! - views: loops and cels;
//! - sounds: parsed (in the PCjr's format);
//! - the vocabulary (`res/vocab.txt`).
//!
//! Prints one line per resource and a summary; exits non-zero if anything fails to
//! decode.

use kq1_formats::{png, script};
use kq1_pc::disk::{Disk, ROOMS, SOUNDS, VIEWS};
use kq1_pc::{color, pic, vocab};
use std::{fs, path::PathBuf};

/// Room 64's script header is off by one on every release's disk (the PCjr's
/// docs/disk-layout.md): its parser rules don't parse and its picture starts a byte
/// early. Not counted as a failure.
const ROOM_64: usize = 64;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: kqextract-pc DISK [OUT]");
        std::process::exit(2);
    };
    let out = PathBuf::from(args.get(1).map_or("extracted", |s| s.as_str()));
    let disk = Disk::from_file(fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1)
    });
    for d in ["files", "res/room", "res/view", "res/sound", "pics"] {
        fs::create_dir_all(out.join(d)).unwrap();
    }
    println!("{} disk", if disk.original { "the original" } else { "the crack's" });
    let mut failures = 0;
    for f in disk.files() {
        match disk.read_file(&f) {
            Ok(data) => {
                println!("file {:<12} {:6} bytes from sector {}", f.name, f.size, f.sector());
                fs::write(out.join("files").join(&f.name), data).unwrap();
            }
            Err(e) => {
                failures += 1;
                println!("file {:<12} FILE ERROR: {e}", f.name);
            }
        }
    }
    let cs = disk.code_segment();
    fs::write(out.join("res/engine.bin"), &cs).unwrap();

    let mut manifest = String::new();
    for (kind, n, e) in disk.entries() {
        manifest += &format!("{kind} {n:3}: sector {:3} offset {:3} sectors {:2}\n", e.sector, e.offset, e.sectors);
    }

    // Rooms: script + picture.
    let (mut scripts, mut pictures) = (0, 0);
    for r in 0..ROOMS {
        let Some(room) = disk.room(r) else { continue };
        fs::write(out.join(format!("res/room/{r:03}.script.bin")), room.script).unwrap();
        let mut line = format!("room {r:3}: script {:5} bytes", room.script.len());
        match script::Script::new(room.script) {
            Ok(s) => {
                let actions = s.actions_with(&kq1_pc::script::OPERANDS);
                let cycle = s.rules(script::Mode::Cycle);
                let parser = s.rules(script::Mode::Parser);
                match (actions, cycle, parser) {
                    (Ok(a), Ok(c), Ok(p)) => {
                        scripts += 1;
                        line += &format!(
                            " ({} actions, {} cycle rules, {} parser rules, {} messages)",
                            a.len(),
                            c.len(),
                            p.len(),
                            s.messages().len()
                        );
                    }
                    (a, c, p) => {
                        if r != ROOM_64 {
                            failures += 1;
                        }
                        let errs: Vec<String> =
                            [a.err(), c.err().map(|e| e.to_string()), p.err().map(|e| e.to_string())]
                                .into_iter()
                                .flatten()
                                .collect();
                        line += &format!(" SCRIPT ERROR: {}", errs.join("; "));
                    }
                }
            }
            Err(e) => {
                failures += 1;
                line += &format!(" SCRIPT ERROR: {e}");
            }
        }
        if r > 0 && !room.picture.is_empty() {
            let p = pic::draw_room(&cs[vocab::DATA_SEGMENT + pic::FRAME..], room.picture);
            let data = &room.picture[..p.end.min(room.picture.len())];
            fs::write(out.join(format!("res/room/{r:03}.pic.bin")), data).unwrap();
            let clean = p.warnings.is_empty() && room.picture.get(p.end.wrapping_sub(1)) == Some(&0xFF);
            if clean {
                pictures += 1;
            } else if r != ROOM_64 {
                failures += 1;
            }
            line += &format!(
                "; picture {} bytes, {} plots{}",
                p.end,
                p.plots,
                if clean { String::new() } else { format!(" PICTURE WARNINGS: {:?}", p.warnings) }
            );
            // The whole 160 x 168 picture, each pixel 2 x 1, in the IBM colours.
            use pic::{HEIGHT, WIDTH};
            let rgb: Vec<u8> = (0..HEIGHT)
                .flat_map(|y| (0..WIDTH * 2).map(move |x| (x / 2, y)))
                .flat_map(|(x, y)| color::rgb(p.visual(x, y)))
                .collect();
            fs::write(out.join(format!("pics/{r:03}.png")), png::encode_rgb((WIDTH * 2) as u32, HEIGHT as u32, &rgb))
                .unwrap();
        }
        println!("{line}");
    }
    println!("room 64: the data defect every release's disk has; not counted as a failure");

    // Views.
    let mut views = 0;
    for v in 0..VIEWS {
        let Some(data) = disk.view(v) else { continue };
        fs::write(out.join(format!("res/view/{v:03}.bin")), data).unwrap();
        if data.len() <= 3 {
            println!("view {v:3}: empty ({} bytes)", data.len());
            continue;
        }
        match kq1_pc::view::decode(data) {
            Ok(vw) => {
                views += 1;
                let cels: usize = vw.loops.iter().map(|l| l.cels.len()).sum();
                println!("view {v:3}: {} bytes, {} loops, {cels} cels", data.len(), vw.loops.len());
            }
            Err(e) => {
                failures += 1;
                println!("view {v:3}: {} bytes VIEW ERROR: {e}", data.len());
            }
        }
    }

    // Sounds.
    let mut sounds = 0;
    for s in 0..SOUNDS {
        let Some(data) = disk.sound(s) else { continue };
        fs::write(out.join(format!("res/sound/{s:03}.bin")), data).unwrap();
        match kq1_pc::sound::parse(data) {
            Ok(four) => {
                for (k, snd) in four.iter().enumerate() {
                    let n = 4 * s + k + 1;
                    match snd {
                        Some(x) => {
                            sounds += 1;
                            println!(
                                "sound {n:2}: {} records, {} chip writes, {:.1} s, {:?}",
                                x.records.len(),
                                x.records.iter().map(|r| r.writes.len()).sum::<usize>(),
                                x.ticks() as f64 / 18.2065,
                                x.end
                            );
                        }
                        None => println!("sound {n:2}: none"),
                    }
                }
            }
            Err(e) => {
                failures += 1;
                println!("sound resource {s}: SOUND ERROR: {e}");
            }
        }
    }
    fs::write(out.join("res/manifest.txt"), manifest).unwrap();
    match vocab::read(&cs) {
        Ok(words) => {
            let text: String = words
                .iter()
                .map(|e| format!("{:<16} position {} type {} id {}\n", e.word, e.position, e.kind, e.id))
                .collect();
            fs::write(out.join("res/vocab.txt"), text).unwrap();
            println!("vocabulary: {} words", words.len());
        }
        Err(e) => {
            failures += 1;
            println!("VOCABULARY ERROR: {e}");
        }
    }
    println!(
        "{scripts} scripts and {pictures} pictures decode cleanly, {views} views, {sounds} sounds; {failures} failures"
    );
    if failures > 0 {
        std::process::exit(1);
    }
}
