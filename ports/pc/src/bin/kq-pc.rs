//! kq [DISK] [--speed N] [--fast-loads] [--mute] [--fullscreen] [--no-joystick]
//!    [--signal] [--bios FILE] [--play FILE] [--scale N] [--rate N] [--room N]
//!    [--route NAME]
//!
//! The IBM PC player, `kq` as shipped (`kq-pc` in the workspace). Plays the 1984 IBM PC
//! King's Quest (CGA, on a composite monitor) in a window, with the native engine, at
//! the original's speed (engine::cost: each pass's work takes as long as it did on a
//! PC, and the timer's 18.2 ticks a second fall where they did). Room pictures appear
//! as the original draws them, outlines then fills, at its pace.
//!
//! DISK is the game's disk, either edition: an HxC `.mfm` dump of the original or a 360K
//! `.img`. With
//! none given, kq looks next to itself, then in the current folder and its
//! `original` folder. Saves go on the Play diskette, which the game makes itself ("copy
//! disk") and which is kept as an image next to the game's disk (`KQ1 Play.img`, or
//! `--play FILE`). When the game asks for the Play or the Master diskette, it is put in
//! the drive for you: press Enter as asked.
//!
//! Keys are the PC's: the arrows and Home, PgUp, End, PgDn walk (the same key again
//! stops), typing enters commands, Enter submits, Tab shows the inventory, Esc pauses,
//! 0 jumps. Ctrl+Left and Ctrl+Right move the picture across the screen a character, as
//! the game lets you on a monitor whose picture sits off centre.
//!
//! A joystick or game controller is the PC's joystick (--no-joystick to leave it out):
//! the stick (or the d-pad) walks Graham while it is pushed, button 1 is the `-` key
//! held down and button 2 the `0` key (the jump). The game takes the stick's centre at
//! its first read, so leave it at rest as the game starts.
//!
//! **The screen** is the CGA's 640-dot mode as a composite monitor shows it (the
//! original's display; src/composite.rs). F7 switches between the two ways of showing
//! it: each 4-dot group in its own colour (the default, the colours the game was drawn
//! for) and the signal decoded dot by dot, with the fringes a monitor shows where colours
//! meet (--signal starts with it). The text screen (the inventory) is in the CGA's
//! colours.
//!
//! **Both editions** play: the first release (BOOT v1.1) and the second (BOOT v1.2,
//! "CGA+RGB"). The second asks at its start: Esc switches between composite and RGB
//! colour, the space bar plays. In RGB the screen is the CGA's 320-dot mode on an RGB
//! monitor (green, red and brown on blue), the game's dithered colours.
//!
//! Front-end keys: F11/F12 slower/faster (--speed N starts at N times the original's
//! speed). F9 fast loads (--fast-loads): the disk reads and the picture drawing (a room
//! change's few seconds) take no time, and the rest keeps the original's pace; the game's
//! own time still passes as it would. F8 sound on/off (--mute). Alt+Enter full screen
//! (--fullscreen). The window can be resized (--scale N: N times 320 × 240 to start, at
//! the display's scale); the picture keeps the screen's 4:3 shape.
//!
//! The text is in the IBM PC's own font when you have its BIOS: --bios FILE takes an
//! image of the IBM PC's BIOS ROM (8 KB, or 64 KB of segment F000, as in emulators' ROM
//! sets: the font is at F000:FA6E) or a dump of the font alone (1,024 or 2,048 bytes).
//! Without --bios, a BIOS image next to the disk, in the current folder or in
//! `original` is used if there is one (`bios.rom`, or a .rom or .bin with "5150" or
//! "ibm" in its name). The port ships no ROM, and has its own font otherwise.
//!
//! --rate N plays at a fixed N passes a second (each a moment of the game: the actors
//! move a step), instead of each pass lasting as long as the original's did; the
//! game's timer keeps time with the passes, and loads and pictures take a pass.
//! --room N starts in room N (the title skipped, Graham where he enters room 1), for
//! looking round; the rooms are numbered as in the game's scripts (1-80).
//!
//! --route NAME plays a recorded route (an event log of the original, refemu-pc
//! `--events`: reference/NAME.events, or a file): the game runs exactly as the original
//! did, at the player's pace and speed (--speed 4 for four times), the Play diskette
//! kept in memory. Typed keys are ignored until the log ends; then the game is yours.
//!
//! --headless "SECONDS:KEYS;..." --until SECONDS --shot FILE.png runs without a window
//! or pacing: the keys are typed at those times of the game (`|` is Enter, `<U>` `<D>`
//! `<L>` `<R>` the arrows), and the screen at --until is written as a PNG (640 × 400,
//! each line doubled), with the text on it printed.

use kq1_frontend::player::{self, audio, Drives, PlayerClock, RouteClock, ScriptClock, WindowSetup};
use kq1_pc::composite::{self, Mode};
use kq1_pc::disk::{Disk, IMAGE_LEN};
use kq1_pc::engine::clock::{Clock, Replay, CPU_HZ};
use kq1_pc::engine::{Speaker, World};
use kq1_pc::font::Font;
use minifb::Key;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const TITLE: &str = "King's Quest (IBM PC, native engine)";
/// The error box's caption.
const CAPTION: &str = "King's Quest (IBM PC)";
/// The output buffer: the CGA's 640 dots × 200 lines, scaled to a 4:3 shape.
const OUT_W: usize = 640;
const OUT_H: usize = 200;
const AUDIO_RATE: u32 = audio::RATE;

/// The speaker: the engine's events (the timer's channel 2 divisor, the gate), as a
/// square wave of 1,193,182 Hz over the divisor while the gate is on.
struct Audio {
    events: VecDeque<Speaker>,
    divisor: u16,
    gate: bool,
    /// Where in its cycle the wave is (0-1).
    phase: f64,
}

/// The integral of a square wave of period 1 (+1 for the first half, -1 for the
/// second) from the start of its cycle to `t` (0-1).
fn square_area(t: f64) -> f64 {
    if t < 0.5 {
        t
    } else {
        1.0 - t
    }
}

impl audio::Render for Audio {
    fn render(&mut self, out: &mut [f32]) {
        for e in self.events.drain(..) {
            match e {
                Speaker::Divisor(d) => self.divisor = d,
                Speaker::Gate(g) => self.gate = g,
            }
        }
        let hz = 1_193_182.0 / if self.divisor == 0 { 65_536.0 } else { self.divisor as f64 };
        // The game stops a note with a divisor of 2 (600 kHz): no sound a speaker makes.
        let audible = self.gate && (20.0..15_000.0).contains(&hz);
        let step = hz / AUDIO_RATE as f64;
        for o in out.iter_mut() {
            if !audible {
                *o = 0.0;
                continue;
            }
            // Each sample the wave's average over its time (no aliasing of the edges).
            let a = square_area(self.phase);
            let end = self.phase + step;
            self.phase = end - end.floor();
            let b = square_area(self.phase);
            *o = ((b - a) / step) as f32 * 0.25;
        }
    }
}

/// --headless: play the script, then write the screen at `until` seconds of the game.
fn headless(
    disk: &Disk,
    font: Vec<u8>,
    mode: Mode,
    plan: &str,
    until: f64,
    shot: &str,
    rate: Option<u64>,
    room: Option<u8>,
    play: Option<PathBuf>,
) {
    let mut w = World::new(disk, Box::new(ScriptClock::parse(plan))).expect("the engine");
    w.font = Some(std::rc::Rc::new(Font(font)));
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
    w.on_frame = Some(Box::new(move |w: &mut World| {
        look(&mut d2.borrow_mut(), w);
        if w.cycles < end {
            return;
        }
        let mut screen = vec![0u32; OUT_W * OUT_H];
        render(w, mode, &mut screen);
        let rgb: Vec<u8> = screen
            .chunks(OUT_W)
            .flat_map(|line| {
                let row: Vec<u8> = line.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]).collect();
                [row.clone(), row]
            })
            .flatten()
            .collect();
        std::fs::write(&shot_path, kq1_formats::png::encode_rgb(640, 400, &rgb)).expect("write the shot");
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

/// The CGA's 16 colours (RGBI), as 0RGB: the text screen's.
const CGA: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555, 0x5555FF, 0x55FF55,
    0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// The screen position the BIOS sets (the CRTC's register 2, 2Dh), where the picture
/// is centred; each step from it moves the picture a character, 16 dots (Ctrl+Left
/// and Ctrl+Right: a higher value moves it left).
const CENTRE: i32 = 0x2D;

/// Line `src` moved `shift` dots right into `out`, the edges black.
fn shift_line<T: Copy + Default>(src: &[T], shift: i32, out: &mut [T]) {
    for (x, o) in out.iter_mut().enumerate() {
        let s = x as i32 - shift;
        *o = if (0..src.len() as i32).contains(&s) { src[s as usize] } else { T::default() };
    }
}

/// The screen the player sees, in RGB (OUT_W × OUT_H): in graphics, the CGA's dots
/// (the engine's frame: the picture's dot patterns and the text the BIOS draws over
/// them) through the composite model; the text screen in the CGA's colours. Moved
/// across by the screen position, the edges black.
///
/// The text is always shown as the monitor shows the signal, mostly as brightness:
/// its strokes aren't colour patterns, and taken a 4-dot group at a time (the plain
/// mode) they would be blocks of colour.
fn render(w: &World, mode: Mode, out: &mut [u32]) {
    let shift = (CENTRE - w.g.hpos as i32) * 16;
    if !w.text.text_mode && w.screen.rgb.is_some() {
        // The second edition's RGB colours: the CGA's 320-dot mode on an RGB monitor,
        // each byte four pixels of 2 bits, palette 0 on blue (3D9h = 01h, `$19E8`), each
        // pixel two of the output's dots wide.
        const RGB: [u32; 4] = [0x0000AA, 0x00AA00, 0xAA0000, 0xAA5500];
        let f = w.frame();
        let mut line = vec![0u32; OUT_W];
        for y in 0..OUT_H {
            for (x, &b) in f[1 + y * 80..1 + (y + 1) * 80].iter().enumerate() {
                for k in 0..4 {
                    let c = RGB[(b >> (6 - 2 * k) & 3) as usize];
                    line[x * 8 + 2 * k] = c;
                    line[x * 8 + 2 * k + 1] = c;
                }
            }
            shift_line(&line, shift, &mut out[y * OUT_W..(y + 1) * OUT_W]);
        }
        return;
    }
    if !w.text.text_mode {
        let f = w.frame();
        let g = &w.text.graphics;
        let (mut dots, mut line) = (vec![0u8; OUT_W], vec![0u8; OUT_W]);
        for y in 0..OUT_H {
            for (x, &b) in f[1 + y * 80..1 + (y + 1) * 80].iter().enumerate() {
                for k in 0..8 {
                    dots[x * 8 + k] = b >> (7 - k) & 1;
                }
            }
            shift_line(&dots, shift, &mut line);
            let mut colours = composite::line(&line, mode);
            // The cells with text in them (the text window's rows; above it, the
            // characters still shown), through the signal's decoding.
            let r = y / 8;
            let text: Vec<usize> = (0..40).filter(|&c| r >= 21 || (g.visible[r][c] && g.cells[r][c] != b' ')).collect();
            if mode == Mode::Groups && !text.is_empty() {
                let signal = composite::line(&line, Mode::Signal);
                for c in text {
                    for x in c as i32 * 16 + shift..(c as i32 + 1) * 16 + shift {
                        if (0..OUT_W as i32).contains(&x) {
                            colours[x as usize] = signal[x as usize];
                        }
                    }
                }
            }
            for (o, c) in out[y * OUT_W..(y + 1) * OUT_W].iter_mut().zip(colours) {
                *o = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
            }
        }
        return;
    }
    // The text screen: 40 columns, each character's pixels two dots wide.
    let grid = w.text.grid();
    let font = w.font.as_deref();
    let mut row = vec![0u32; OUT_W];
    for y in 0..OUT_H {
        let (r, gl) = (y / 8, y % 8);
        for c in 0..40 {
            let (ch, a) = (grid.cells[r][c], grid.attrs[r][c]);
            let bits = font.map_or(0, |f| f.row(ch, gl));
            for k in 0..8 {
                let colour = if bits >> (7 - k) & 1 != 0 { a & 0x0F } else { (a >> 4) & 7 };
                row[c * 16 + k * 2] = CGA[colour as usize];
                row[c * 16 + k * 2 + 1] = CGA[colour as usize];
            }
        }
        shift_line(&row, shift, &mut out[y * OUT_W..(y + 1) * OUT_W]);
    }
}

/// An IBM PC BIOS image next to the disk, in the current folder or in `original`, and
/// its font: a file named `bios.rom`, or a .rom or .bin with "5150" or "ibm" in its
/// name, of 8 or 64 KB, whose font is there.
fn find_bios_font(disk: &Path) -> Option<(PathBuf, Vec<u8>)> {
    let dirs = [disk.parent().unwrap_or(Path::new(".")).to_path_buf(), PathBuf::from("."), PathBuf::from("original")];
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut names: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        names.sort();
        for p in names {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            let rom = name == "bios.rom"
                || ((name.contains("5150") || name.contains("ibm"))
                    && (name.ends_with(".rom") || name.ends_with(".bin")));
            if !rom || std::fs::metadata(&p).map_or(true, |m| m.len() != 0x2000 && m.len() != 0x10000) {
                continue;
            }
            if let Some(f) = std::fs::read(&p).ok().and_then(|b| Font::from_bios(&b).ok()).filter(Font::plausible) {
                return Some((p, f.0));
            }
        }
    }
    None
}

/// The game's disk: given, or found next to the program, in the current folder or
/// its `original` folder.
fn find_disk(given: Option<&str>) -> Result<PathBuf, String> {
    player::find_disk(
        given,
        |raw| Disk::from_file(raw).is_ok(),
        "No disk image of King's Quest (IBM PC) was found.\n\nPut the game's disk image (an HxC .mfm dump or a \
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
    let font_file = opt("--bios");
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
    let shot = opt("--shot").unwrap_or_else(|| "kq-pc.png".into());
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
    let mode = if flag("--signal") { Mode::Signal } else { Mode::Groups };
    if args.iter().any(|a| a.starts_with("--")) || args.len() > 1 {
        eprintln!("usage: kq [DISK] [--speed N] [--fast-loads] [--mute] [--fullscreen] [--no-joystick] [--signal] [--bios FILE] [--play FILE] [--scale N] [--rate N] [--room N] [--route NAME]");
        std::process::exit(2);
    }
    let disk_path = find_disk(args.first().map(String::as_str)).unwrap_or_else(|e| fatal(&e));
    let raw = std::fs::read(&disk_path).unwrap_or_else(|e| fatal(&format!("{}: {e}", disk_path.display())));
    let disk = Disk::from_file(raw).unwrap_or_else(|e| fatal(&format!("{}: {e}", disk_path.display())));
    // The font's bytes (the engine's World, on its own thread, makes its Font of them).
    let font: Vec<u8> = match font_file {
        Some(f) => {
            let bytes = std::fs::read(&f).unwrap_or_else(|e| fatal(&format!("{f}: {e}")));
            Font::from_bios(&bytes).unwrap_or_else(|e| fatal(&format!("{f}: {e}"))).0
        }
        None => match find_bios_font(&disk_path) {
            Some((p, f)) => {
                println!("the font from {}", p.display());
                f
            }
            None => Font::built_in().0,
        },
    };
    if let Some(plan) = plan {
        headless(&disk, font, mode, &plan, until, &shot, rate, room, play_file.map(PathBuf::from));
        return;
    }
    let play_path = play_file
        .map(PathBuf::from)
        .unwrap_or_else(|| disk_path.parent().unwrap_or(Path::new(".")).join("KQ1 Play.img"));

    let shared = player::Shared::new(OUT_W * OUT_H, speed, fast_loads, route.is_some());
    // How the composite screen is shown (F7).
    let mode = Arc::new(Mutex::new(mode));
    let route_log = route.map(|r| {
        let file = if Path::new(&r).is_file() {
            PathBuf::from(&r)
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("reference").join(format!("{r}.events"))
        };
        std::fs::read_to_string(&file).unwrap_or_else(|e| fatal(&format!("{}: {e}", file.display())))
    });
    let audio = Arc::new(Mutex::new(Audio { events: VecDeque::new(), divisor: 2, gate: false, phase: 0.0 }));
    let sound_on = Arc::new(AtomicBool::new(!mute));
    audio::start(audio.clone(), sound_on.clone());

    // The engine, on a thread of its own (its state isn't shared: the screen goes out
    // as frames).
    {
        let shared = shared.clone();
        let audio = audio.clone();
        let mode = mode.clone();
        std::thread::spawn(move || {
            let play = PlayerClock::new(shared.clone(), speed, use_joystick);
            let replaying = route_log.is_some();
            let clock: Box<dyn Clock> = match route_log {
                Some(log) => Box::new(RouteClock::new(Replay::parse(&log), play)),
                None => Box::new(play),
            };
            let mut w = World::new(&disk, clock).expect("the engine");
            w.font = Some(std::rc::Rc::new(Font(font)));
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
            let mut rgb = vec![0u32; OUT_W * OUT_H];
            let s2 = shared.clone();
            let mut loading = 0;
            let mut was_replaying = false;
            w.on_frame = Some(Box::new(move |w: &mut World| {
                // While a fast load goes by, its frames needn't be drawn.
                let (fast, mode) = (s2.lock().unwrap().fast_loads, *mode.lock().unwrap());
                let skip = fast && w.loading_cycles != std::mem::replace(&mut loading, w.loading_cycles);
                if !skip {
                    render(w, mode, &mut rgb);
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
                audio.lock().unwrap().events.extend(w.speaker.drain(..));
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
        help: "F11/F12: slower/faster, F9: fast loads, F8: sound, F7: the colours' fringes, Alt+Enter: full screen",
        caption: CAPTION,
        ctrl_arrows: true,
    };
    player::run_window(setup, shared, sound_on, |k, window| {
        // F7: the composite screen's two modes.
        if k == Key::F7 {
            let mut m = mode.lock().unwrap();
            *m = if *m == Mode::Groups { Mode::Signal } else { Mode::Groups };
            let what = if *m == Mode::Signal { " - the signal, with its fringes" } else { "" };
            window.set_title(&format!("{TITLE}{what}"));
        }
    });
}

/// Report an error that stops kq before its window opens, and exit (in a message box
/// too on Windows).
fn fatal(msg: &str) -> ! {
    kq1_frontend::fatal(CAPTION, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The speaker: a note sounds at its pitch while the gate is on; the game's stop (a
    /// divisor of 2) and the gate off are silence.
    #[test]
    fn the_speaker_plays_the_notes() {
        use audio::Render;
        let mut a = Audio { events: VecDeque::new(), divisor: 2, gate: false, phase: 0.0 };
        let mut out = vec![0f32; AUDIO_RATE as usize];
        a.events.extend([Speaker::Divisor(1193), Speaker::Gate(true)]);
        a.render(&mut out);
        // 1,000 Hz: 2,000 sign changes a second, and no offset.
        let changes = out.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        assert!((1990..=2010).contains(&changes), "{changes} sign changes");
        assert!((out.iter().sum::<f32>() / out.len() as f32).abs() < 0.01);
        a.events.push_back(Speaker::Divisor(2));
        a.render(&mut out);
        assert!(out.iter().all(|&v| v == 0.0), "a divisor of 2 is silence");
        a.events.extend([Speaker::Divisor(1193), Speaker::Gate(false)]);
        a.render(&mut out);
        assert!(out.iter().all(|&v| v == 0.0), "the gate off is silence");
    }
}
