//! kq [DISK] [--speed N] [--fast-loads] [--mute] [--fullscreen] [--no-joystick]
//!    [--font FILE] [--no-rom-font] [--play FILE] [--scale N] [--rate N] [--room N]
//!         [--route NAME]
//!
//! The IBM PCjr player, `kq` as shipped (`kq-pcjr` in the workspace). Plays the IBM PCjr King's Quest in a window, with the native engine, at the
//! original's speed (engine::cost: each pass lasts as long as it did on a PCjr, and the
//! timer's 18.2 ticks a second fall where they did). Room pictures appear as the
//! original draws them, outlines then fills, at its pace.
//!
//! DISK is the game's disk: an HxC `.mfm` dump of the original or a 360K `.img`. With
//! none given, kq looks next to itself, then in the current folder and its
//! `original` folder. Saves go on the Play diskette, which the game makes itself ("copy
//! disk") and which is kept as an image next to the game's disk (`KQ1 Play.img`, or
//! `--play FILE`). When the game asks for the Play or the Master diskette, it is put in
//! the drive for you: press Enter as asked.
//!
//! Keys are the PCjr's: the arrows and Home, PgUp, End, PgDn walk (the same key again
//! stops), typing enters commands, Enter submits, Tab shows the inventory, Esc pauses,
//! 0 jumps.
//!
//! A joystick or game controller is the PCjr's joystick (--no-joystick to leave it
//! out): the stick (or the d-pad) walks Graham while it is pushed, button 1 is the `-`
//! key held down and button 2 the `0` key (the jump). The original reads it whenever
//! the timer finds no key waiting, as it does with none connected.
//!
//! Front-end keys: F11/F12 slower/faster (--speed N starts at N times the original's
//! speed). F9 fast loads (--fast-loads): the disk reads and the picture drawing (a room
//! change's few seconds) take no time, and the rest keeps the original's pace; the game's
//! own time still passes as it would. F8 sound on/off (--mute). Alt+Enter full screen (--fullscreen). The window
//! can be resized (--scale N: N times 320 × 240 to start, at the display's scale); the
//! picture keeps the PCjr screen's 4:3 shape.
//!
//! The text is in the PCjr's own font when you have its ROM: --font FILE takes the PCjr
//! BIOS image (64 KB, F000:0000-FFFF, as in emulators' ROM sets: the font is at
//! F000:FA6E) or a dump of the font alone (1,024 bytes; or a 2,048-byte 256-character
//! font). Without --font, a PCjr BIOS image next to the disk or in the current folder
//! is used if there is one (`bios.rom`, or a .rom or .bin with "pcjr" in its name).
//! The port ships no ROM, and has its own font otherwise (--no-rom-font keeps it).
//!
//! --rate N plays at a fixed N passes a second (each a moment of the game: the actors
//! move a step), instead of each pass lasting as long as the original's did; the
//! game's timer keeps time with the passes, and loads and pictures take a pass.
//! --room N starts in room N (the title skipped, Graham where he enters room 1), for
//! looking round; the rooms are numbered as in the game's scripts (1-80).
//!
//! --route NAME plays a recorded route (an event log of the original, refemu-pcjr
//! `--events`: reference/NAME.events, or a file): the game runs exactly as the original
//! did, at the player's pace and speed (--speed 4 for four times), the Play diskette
//! kept in memory. Typed keys are ignored until the log ends; then the game is yours.
//!
//! --headless "SECONDS:KEYS;..." --until SECONDS --shot FILE.png runs without a window
//! or pacing: the keys are typed at those times of the game (`|` is Enter, `<U>` `<D>`
//! `<L>` `<R>` the arrows), and the screen at --until is written as a PNG, with the
//! text on it printed.

use kq1_frontend::player::{self, audio, Drives, PlayerClock, RouteClock, ScriptClock, WindowSetup};
use kq1_pcjr::disk::{Disk, IMAGE_LEN};
use kq1_pcjr::engine::clock::{Clock, Replay, CPU_HZ};
use kq1_pcjr::engine::screen::{HEIGHT, WIDTH};
use kq1_pcjr::engine::World;
use kq1_pcjr::font::Font;
use kq1_pcjr::sound::Chip;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const TITLE: &str = "King's Quest (IBM PCjr, native engine)";
/// The output buffer: the PCjr's 320 × 200, each row and column scaled to a 4:3 shape.
const OUT_W: usize = 320;
const OUT_H: usize = 200;
const AUDIO_RATE: u32 = audio::RATE;
/// The error box's caption.
const CAPTION: &str = "King's Quest (PCjr)";

/// The sound chip's bytes, and the chip itself, for the sound output.
struct Audio {
    chip: Chip,
    writes: VecDeque<u8>,
}

impl audio::Render for Audio {
    fn render(&mut self, out: &mut [f32]) {
        for b in self.writes.drain(..) {
            self.chip.write(b);
        }
        for o in out.iter_mut() {
            *o = self.chip.sample(AUDIO_RATE as f64) * 0.8;
        }
    }
}

/// --headless: play the script, then write the screen at `until` seconds of the game.
fn headless(
    disk: &Disk,
    font: &Font,
    plan: &str,
    until: f64,
    shot: &str,
    rate: Option<u64>,
    room: Option<u8>,
    play: Option<PathBuf>,
) {
    let mut w = World::new(disk, Box::new(ScriptClock::parse(plan))).expect("the engine");
    let drives = std::rc::Rc::new(std::cell::RefCell::new(Drives::new(play)));
    if let Some(img) = drives.borrow_mut().load(IMAGE_LEN, w.disks[1].writes) {
        w.disks[1].image = img;
        w.drive = 1;
    }
    let d2 = drives.clone();
    w.fixed_pass_cycles = rate;
    w.start_room = room;
    let end = (until * CPU_HZ) as u64;
    let shot_path = shot.to_string();
    let f = font.clone();
    w.on_frame = Some(Box::new(move |w: &mut World| {
        look(&mut d2.borrow_mut(), w);
        if w.cycles < end {
            return;
        }
        let mut screen = vec![0u8; 320 * 200];
        render(w, &f, &mut screen);
        let rgb: Vec<u8> = screen.iter().flat_map(|&c| kq1_pcjr::color::rgb(c)).collect();
        std::fs::write(&shot_path, kq1_formats::png::encode_rgb(320, 200, &rgb)).expect("write the shot");
        let text: Vec<String> = (0..25).map(|r| w.text.row(r)).filter(|r| !r.trim().is_empty()).collect();
        println!(
            "at {:.1} s ({:.1} s of it loading: the disk and the pictures), pass {}, room {}, Graham at ({}, {}), score {}:\n{}",
            w.cycles as f64 / CPU_HZ,
            w.loading_cycles as f64 / CPU_HZ,
            w.passes,
            w.vars[1],
            w.actors[0].x,
            w.actors[0].y,
            w.vars[19],
            text.join("\n")
        );
        w.stop("the end of the script".into());
    }));
    w.run();
    drives.borrow_mut().finish(&w.disks[1].image, w.disks[1].writes);
}

/// The Play diskette's helper's look at the game, at each frame.
fn look(d: &mut Drives, w: &mut World) {
    let screen: String = (0..25).map(|r| w.text.row(r)).collect::<Vec<_>>().join(" ");
    let (image, writes) = (&w.disks[1].image, w.disks[1].writes);
    d.look(&screen, &mut w.drive, image, writes);
}

/// The screen the player sees, as 320 × 200 colour numbers: the picture (each of its
/// 160 bytes a row two pixels), then the text cells over it: in the text window (rows
/// 21-24) all of them; above it the characters written there (the engine has cleared
/// their cells); or the whole text screen when it is up.
fn render(w: &World, font: &Font, out: &mut [u8]) {
    let grid = w.text.grid();
    if w.text.text_mode {
        for r in 0..25 {
            for c in 0..40 {
                let a = grid.attrs[r][c];
                cell(out, font, r, c, grid.cells[r][c], a & 0x0F, Some((a >> 4) & 7));
            }
        }
        return;
    }
    out.fill(0);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let v = w.screen.shown[y * WIDTH + x] & 0x0F;
            out[y * 320 + x * 2] = v;
            out[y * 320 + x * 2 + 1] = v;
        }
    }
    for r in 0..25 {
        for c in 0..40 {
            let ch = grid.cells[r][c];
            let a = grid.attrs[r][c];
            if a & 0x80 != 0 {
                // The BIOS's XOR (the title's credits): the glyph's pixels XORed in the
                // colour onto the picture, the rest left as it is.
                if r >= 21 || grid.visible[r][c] {
                    xor_cell(out, font, r, c, ch, a & 0x0F);
                }
            } else if r >= 21 {
                cell(out, font, r, c, ch, a & 0x0F, Some(0));
            } else if grid.visible[r][c] && ch != b' ' {
                cell(out, font, r, c, ch, a & 0x0F, None);
            }
        }
    }
}

/// Character `ch` in cell (r, c), its ink XORed in `colour` onto what is there.
fn xor_cell(out: &mut [u8], font: &Font, r: usize, c: usize, ch: u8, colour: u8) {
    for (dy, row) in font.glyph(ch).iter().enumerate() {
        for dx in 0..8 {
            if row & (0x80 >> dx) != 0 {
                out[(r * 8 + dy) * 320 + c * 8 + dx] ^= colour;
            }
        }
    }
}

/// Character `ch` in cell (r, c): its ink in `fg`, the rest in `bg` (or left alone).
fn cell(out: &mut [u8], font: &Font, r: usize, c: usize, ch: u8, fg: u8, bg: Option<u8>) {
    let g = font.glyph(ch);
    for (dy, row) in g.iter().enumerate() {
        for dx in 0..8 {
            let i = (r * 8 + dy) * 320 + c * 8 + dx;
            if row & (0x80 >> dx) != 0 {
                out[i] = fg;
            } else if let Some(b) = bg {
                out[i] = b;
            }
        }
    }
}

/// The PCjr's 16 colours, as 0RGB.
fn palette() -> [u32; 16] {
    std::array::from_fn(|i| {
        let [r, g, b] = kq1_pcjr::color::rgb(i as u8);
        (r as u32) << 16 | (g as u32) << 8 | b as u32
    })
}

/// The 320 × 200 screen into the window's 640 × 480 buffer, in RGB.
fn scale(screen: &[u8], palette: &[u32; 16], buf: &mut [u32]) {
    for (o, &c) in buf.iter_mut().zip(screen) {
        *o = palette[c as usize & 15];
    }
}

/// The game's disk: given, or found next to the program, in the current folder or
/// its `original` folder.
/// A PCjr BIOS image next to the disk, in the current folder or in `original`, and
/// its font: a file named `bios.rom`, or a .rom or .bin with "pcjr" in its name, that
/// holds one.
fn find_bios_font(disk: &Path) -> Option<(PathBuf, Font)> {
    let dirs = [disk.parent().unwrap_or(Path::new(".")).to_path_buf(), PathBuf::from("."), PathBuf::from("original")];
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut names: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        names.sort();
        for p in names {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            let rom =
                name == "bios.rom" || (name.contains("pcjr") && (name.ends_with(".rom") || name.ends_with(".bin")));
            if !rom || std::fs::metadata(&p).map_or(true, |m| m.len() != 0x10000) {
                continue;
            }
            if let Some(f) = std::fs::read(&p).ok().and_then(|b| Font::from_dump(&b).ok()) {
                return Some((p, f));
            }
        }
    }
    None
}

fn find_disk(given: Option<&str>) -> Result<PathBuf, String> {
    player::find_disk(
        given,
        |raw| Disk::from_file(raw).is_ok(),
        "No disk image of King's Quest (IBM PCjr) was found.\n\nPut the game's disk image (an HxC .mfm dump or a \
         360K .img) next to kq, or give its path: kq DISK",
    )
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let speed: f64 = opt("--speed").map_or(1.0, |s| s.parse().expect("--speed N"));
    let font_file = opt("--font");
    let play_file = opt("--play");
    let scale_n: Option<usize> = opt("--scale").map(|s| s.parse().expect("--scale N"));
    let route = opt("--route");
    // --rate N: passes a second, as cycles a pass of the game's time.
    let rate: Option<u64> = opt("--rate").map(|r| {
        let n: f64 = r.parse().ok().filter(|n: &f64| *n > 0.0).unwrap_or_else(|| fatal("--rate N: passes a second"));
        (CPU_HZ / n) as u64
    });
    let room: Option<u8> = opt("--room")
        .map(|r| r.parse().ok().filter(|n| (1..=80).contains(n)).unwrap_or_else(|| fatal("--room N: 1-80")));
    if route.is_some() && (rate.is_some() || room.is_some()) {
        fatal("--route plays the original's own run: not with --rate or --room");
    }
    let plan = opt("--headless");
    let until: f64 = opt("--until").map_or(30.0, |s| s.parse().expect("--until SECONDS"));
    let shot = opt("--shot").unwrap_or_else(|| "kq-pcjr.png".into());
    let start_fullscreen = if let Some(p) = args.iter().position(|a| a == "--fullscreen") {
        args.remove(p);
        true
    } else {
        false
    };
    let mut flag = |name: &str| -> bool {
        let p = args.iter().position(|a| a == name);
        if let Some(p) = p {
            args.remove(p);
        }
        p.is_some()
    };
    let mute = flag("--mute");
    let fast_loads = flag("--fast-loads");
    let use_joystick = !flag("--no-joystick");
    let no_rom_font = flag("--no-rom-font");
    if args.iter().any(|a| a.starts_with("--")) || args.len() > 1 {
        eprintln!("usage: kq [DISK] [--speed N] [--fast-loads] [--mute] [--fullscreen] [--no-joystick] [--font FILE] [--no-rom-font] [--play FILE] [--scale N] [--rate N] [--room N] [--route NAME]");
        std::process::exit(2);
    }
    let disk_path = find_disk(args.first().map(String::as_str)).unwrap_or_else(|e| fatal(&e));
    let raw = std::fs::read(&disk_path).unwrap_or_else(|e| fatal(&format!("{}: {e}", disk_path.display())));
    let disk = Disk::from_file(raw).unwrap_or_else(|e| fatal(&format!("{}: {e}", disk_path.display())));
    let font = match font_file {
        Some(f) => {
            let bytes = std::fs::read(&f).unwrap_or_else(|e| fatal(&format!("{f}: {e}")));
            Font::from_dump(&bytes).unwrap_or_else(|e| fatal(&format!("{f}: {e}")))
        }
        None if no_rom_font => Font::default(),
        None => match find_bios_font(&disk_path) {
            Some((p, f)) => {
                println!("the font from {}", p.display());
                f
            }
            None => Font::default(),
        },
    };
    if let Some(plan) = plan {
        headless(&disk, &font, &plan, until, &shot, rate, room, play_file.map(PathBuf::from));
        return;
    }
    let play_path = play_file
        .map(PathBuf::from)
        .unwrap_or_else(|| disk_path.parent().unwrap_or(Path::new(".")).join("KQ1 Play.img"));

    let shared = player::Shared::new(OUT_W * OUT_H, speed, fast_loads, route.is_some());
    let route_log = route.map(|r| {
        let file = if Path::new(&r).is_file() {
            PathBuf::from(&r)
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("reference").join(format!("{r}.events"))
        };
        std::fs::read_to_string(&file).unwrap_or_else(|e| fatal(&format!("{}: {e}", file.display())))
    });
    let audio = Arc::new(Mutex::new(Audio { chip: Chip::default(), writes: VecDeque::new() }));
    let sound_on = Arc::new(AtomicBool::new(!mute));
    audio::start(audio.clone(), sound_on.clone());

    // The engine, on a thread of its own (its state isn't shared: the screen goes out
    // as frames).
    {
        let shared = shared.clone();
        let audio = audio.clone();
        std::thread::spawn(move || {
            let play = PlayerClock::new(shared.clone(), speed, use_joystick);
            let replaying = route_log.is_some();
            let clock: Box<dyn Clock> = match route_log {
                Some(log) => Box::new(RouteClock::new(Replay::parse(&log), play)),
                None => Box::new(play),
            };
            let mut w = World::new(&disk, clock).expect("the engine");
            w.fixed_pass_cycles = rate;
            w.start_room = room;
            // A route starts with the Play diskette the original had (a blank one), and
            // keeps it in memory.
            let drives = std::rc::Rc::new(std::cell::RefCell::new(Drives::new((!replaying).then_some(play_path))));
            if let Some(img) = drives.borrow_mut().load(IMAGE_LEN, w.disks[1].writes) {
                w.disks[1].image = img;
                w.drive = 1;
            }
            let d2 = drives.clone();
            let mut screen = vec![0u8; 320 * 200];
            let mut rgb = vec![0u32; OUT_W * OUT_H];
            let colours = palette();
            let s2 = shared.clone();
            let mut loading = 0;
            let mut was_replaying = false;
            w.on_frame = Some(Box::new(move |w: &mut World| {
                // While a fast load goes by, its frames needn't be drawn.
                let fast = s2.lock().unwrap().fast_loads;
                let skip = fast && w.loading_cycles != std::mem::replace(&mut loading, w.loading_cycles);
                if !skip {
                    render(w, &font, &mut screen);
                    scale(&screen, &colours, &mut rgb);
                }
                {
                    let mut s = s2.lock().unwrap();
                    if !skip {
                        s.rgb.copy_from_slice(&rgb);
                        s.seq += 1;
                    }
                    if s.quit {
                        drop(s);
                        w.stop("the window was closed".into());
                    }
                }
                audio.lock().unwrap().writes.extend(w.sound_out.drain(..));
                if s2.lock().unwrap().replaying {
                    was_replaying = true;
                    return;
                }
                if std::mem::take(&mut was_replaying) {
                    println!("at pass {}: room {}, score {}", w.passes, w.vars[1], w.vars[19]);
                }
                look(&mut d2.borrow_mut(), w);
            }));
            let why = w.run();
            drives.borrow_mut().finish(&w.disks[1].image, w.disks[1].writes);
            shared.lock().unwrap().ended = Some(why);
        });
    }

    let setup = WindowSetup {
        title: TITLE,
        width: OUT_W,
        height: OUT_H,
        scale: scale_n.unwrap_or(3),
        fullscreen: start_fullscreen,
        help: "F11/F12: slower/faster, F9: fast loads, F8: sound, Alt+Enter: full screen",
        caption: CAPTION,
        ctrl_arrows: false,
    };
    player::run_window(setup, shared, sound_on, |_, _| {});
}

/// Report an error that stops kq before its window opens, and exit (in a message box
/// too on Windows).
fn fatal(msg: &str) -> ! {
    kq1_frontend::fatal(CAPTION, msg)
}
