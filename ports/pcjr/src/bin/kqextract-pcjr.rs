//! kqextract-pcjr DISK.img [OUT]: extract every resource from the PCjr game disk into OUT
//! (default `extracted/`, which git ignores: it is game content), and check each one
//! with the Apple port's decoders, which read the same formats:
//!
//! - rooms: the script (actions, cycle rules, parser rules, messages) and the picture
//!   (drawn with the PCjr painter, as the engine draws it, and rendered to
//!   OUT/pics/NNN.png in the IBM colours);
//! - views: loops and cels;
//! - sounds: parsed, and each rendered to OUT/sounds/NN.wav (the sound chip
//!   synthesised, the sound played once);
//! - the engine is written as it is.
//!
//! Prints one line per resource and a summary; exits non-zero if anything fails to
//! decode.

use kq1_apple2::{png, script};
use kq1_pcjr::color;
use kq1_pcjr::disk::{Disk, ROOMS, SOUNDS, VIEWS};
use std::{fs, path::PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: kqextract-pcjr DISK.img [OUT]");
        std::process::exit(2);
    };
    let out = PathBuf::from(args.get(1).map_or("extracted", |s| s.as_str()));
    let disk = Disk::from_file(fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1)
    });
    for d in ["res/room", "res/view", "res/sound", "pics", "sounds"] {
        fs::create_dir_all(out.join(d)).unwrap();
    }
    fs::write(out.join("res/engine.bin"), disk.code_segment()).unwrap();

    let mut failures = 0;
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
                let actions = s.actions_with(&kq1_pcjr::script::OPERANDS);
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
                        if r != 64 {
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
            let cs = disk.code_segment();
            let p = kq1_pcjr::pic::draw_room(&cs[kq1_pcjr::vocab::DATA_SEGMENT + kq1_pcjr::pic::FRAME..], room.picture);
            let data = &room.picture[..p.end.min(room.picture.len())];
            fs::write(out.join(format!("res/room/{r:03}.pic.bin")), data).unwrap();
            let clean = p.warnings.is_empty() && room.picture.get(p.end.wrapping_sub(1)) == Some(&0xFF);
            if clean {
                pictures += 1;
            } else if r != 64 {
                failures += 1;
            }
            line += &format!(
                "; picture {} bytes, {} plots{}",
                p.end,
                p.plots,
                if clean { String::new() } else { format!(" PICTURE WARNINGS: {:?}", p.warnings) }
            );
            // The whole 160 x 168 picture, each pixel 2 x 1 as mode 9 shows it, in the IBM
            // colours.
            use kq1_pcjr::pic::{HEIGHT, WIDTH};
            let rgb: Vec<u8> = (0..HEIGHT)
                .flat_map(|y| (0..WIDTH * 2).map(move |x| (x / 2, y)))
                .flat_map(|(x, y)| color::rgb(p.visual(x, y)))
                .collect();
            fs::write(out.join(format!("pics/{r:03}.png")), png::encode_rgb((WIDTH * 2) as u32, HEIGHT as u32, &rgb))
                .unwrap();
        }
        println!("{line}");
    }
    println!(
        "room 64: the original's data defect (its script header is off by one, so its parser rules don't parse, \
         and the engine starts its picture a byte early and draws none); not counted as a failure"
    );

    // Views.
    let mut views = 0;
    for v in 0..VIEWS {
        let Some(data) = disk.view(v) else { continue };
        fs::write(out.join(format!("res/view/{v:03}.bin")), data).unwrap();
        if data.len() <= 3 {
            println!("view {v:3}: empty ({} bytes)", data.len());
            continue;
        }
        match kq1_pcjr::view::decode(data) {
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

    let mut sounds = 0;
    for s in 0..SOUNDS {
        let Some(data) = disk.sound(s) else { continue };
        fs::write(out.join(format!("res/sound/{s:03}.bin")), data).unwrap();
        match kq1_pcjr::sound::parse(data) {
            Ok(four) => {
                for (k, snd) in four.iter().enumerate() {
                    let n = 4 * s + k + 1;
                    match snd {
                        Some(x) => {
                            sounds += 1;
                            let pcm = kq1_pcjr::sound::render(x, 22050);
                            fs::write(out.join(format!("sounds/{n:02}.wav")), kq1_pcjr::sound::wav(&pcm, 22050))
                                .unwrap();
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
    match kq1_pcjr::vocab::read(&disk.code_segment()) {
        Ok(words) => {
            let text: String = words
                .iter()
                .map(|e| {
                    format!(
                        "{:<16} position {} type {} id {}
",
                        e.word, e.position, e.kind, e.id
                    )
                })
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
