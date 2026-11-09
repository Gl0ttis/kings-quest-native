//! kq [FOLDER | disk1 disk2 disk3] [--speed N] [--fast-loads] [--fullscreen] [--mute]
//!        [--rate N] [--scale N] [--room N] [--saves DIR] [--keys FILE] [--headless PLAN]
//!
//! The disks are the game's three disk images, as .nib (any order: each records its
//! disk number) or decoded DOS-order .do/.dsk (in order, or found by the disk number
//! in their names). With none given, kq looks next to itself, then in the current
//! folder (Disks::find). Saves go in a saves folder next to the disks.
//!
//! Plays the game in a window using the native engine, at the original's speed (each
//! pass of the main loop lasts as long as it did on an Apple IIe; engine::cost). Room
//! pictures appear line by line as the original draws them.
//! Keys are as on the Apple II: arrows and ' / . ; move (press again to stop), typing
//! enters commands, RETURN submits, Tab shows the inventory, Esc pauses, Ctrl-E recalls the
//! last line, 1/3/5 are the save/restore/restart shortcuts.
//!
//! Front-end keys: F11/F12 slower/faster (--speed N starts at N times the original's
//! speed; everything speeds up, walking included). F9 fast loads (--fast-loads): room
//! changes skip the disk and the picture drawing, about 10 s each, and the rest keeps the
//! original's pace; the game plays the same, since its state depends only on passes.
//! --swap-prompts shows the original's "Please insert side B and press RETURN." prompts
//! when the game reads another disk (the port has every disk, so they're off by default).
//! F8 sound on/off (--mute). F7 NTSC colour fringing (--ntsc). F6 fills the white specks
//! where the picture's fills miss a pixel (--fill-specks; display only). Alt+Enter full
//! screen (--fullscreen). The window can be resized; the picture keeps its shape.
//!
//! --savedisk FILE saves to and restores from an original save disk image (disk 4,
//! volume 10; .nib or .do), which the original on an emulator can read, and the other
//! way round. Without it, saves are files in a saves folder next to the disks (--saves DIR).
//! --charrom FILE uses an Apple IIe video ROM dump for the text screen (the inventory);
//! the port ships no ROM. A file named like one (342-0265 or 342-0133) next to the disk
//! images is used without asking; --no-charrom keeps the game's own font. A joystick or game controller is used if one is connected
//! when the game starts (--no-joystick to ignore it): the stick moves Graham, and
//! buttons 1 and 2 are the Apple's two buttons.
//!
//! --keys FILE replays a key log (lines "PASS KEY-HEX", as written by kqroute: the key
//! is pressed just before that pass), for example routes/max_score.keys. While it plays,
//! typed keys are ignored, waits for a key are answered with RETURN after about 3 seconds
//! (as refemu does with the original), and the title screens run their full time. When
//! the log ends, the game is yours.
//!
//! --headless runs without a window. PLAN is a comma-separated list: a direction (U, UR,
//! R, DR, D, DL, L, UL, or S to stop) followed by a number of cycles, or ">text" to type a
//! command and press RETURN. The final frame is written to extracted/kq_last.png.
use kq1_apple2::engine::text::{FRAME_H, FRAME_W};
use kq1_apple2::engine::{f, IoImpl, Resources, Scripted, World};
use kq1_apple2::kq::Disks;
use kq1_frontend::{fullscreen::Fullscreen, hook};
use minifb::{InputCallback, Key, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const TITLE: &str = "King's Quest (native engine)";

type Queue = Rc<RefCell<VecDeque<u8>>>;

/// Typed characters, from the window's callback.
struct Chars(Queue);
impl InputCallback for Chars {
    fn add_char(&mut self, c: u32) {
        let k = match c {
            0x08 => 0x7F, // Backspace = Apple Delete
            0x0D | 0x09 | 0x1B | 0x03 | 0x05 | 0x0E | 0x20..=0x7E => c as u8,
            _ => return,
        };
        self.0.borrow_mut().push_back(k);
    }
}

/// Between the window thread and the engine thread.
struct Shared {
    /// The latest frame, lines doubled.
    frame: Vec<u32>,
    frame_seq: u64,
    /// Keys for the engine, in Apple codes.
    keys: VecDeque<u8>,
    speed: f64,
    fast_loads: bool,
    /// A key log is playing: typed keys are dropped and waits answer themselves.
    replaying: bool,
    mute: bool,
    ntsc: bool,
    specks: bool,
    quit: bool,
}
type Link = Arc<Mutex<Shared>>;

fn publish(link: &Link, frame: &[u32]) {
    let d = doubled(frame);
    let mut s = link.lock().unwrap();
    s.frame = d;
    s.frame_seq += 1;
}

/// The engine's front end, on the engine thread.
struct EngineIo {
    link: Link,
    saves: PathBuf,
    /// --savedisk: an original save-disk image (and where to write it back).
    disk: Option<(kq1_apple2::savedisk::SaveDisk, PathBuf)>,
    joystick: bool,
    /// Buttons pressed since the engine last read the joystick (see run_engine).
    latch: Rc<Cell<[bool; 2]>>,
}

/// saves/kq-a.sav .. kq-z.sav
fn slot_path(dir: &Path, slot: u8) -> PathBuf {
    dir.join(format!("kq-{}.sav", (b'a' + slot - 1) as char))
}

impl EngineIo {
    /// Show `frame` and wait for a key, for at most `secs` seconds if given. While a key
    /// log replays, the wait lasts `secs` (or 3 s, then RETURN) at the current speed.
    fn wait(&mut self, frame: &[u32], secs: Option<f64>) -> Option<u8> {
        publish(&self.link, frame);
        let start = Instant::now();
        loop {
            {
                let mut s = self.link.lock().unwrap();
                if s.quit {
                    std::process::exit(0);
                }
                let t = start.elapsed().as_secs_f64();
                if s.replaying {
                    s.keys.clear();
                    if t * s.speed >= secs.unwrap_or(3.0) {
                        return if secs.is_some() { None } else { Some(0x0D) };
                    }
                } else if let Some(k) = s.keys.pop_front() {
                    return Some(k);
                } else if secs.is_some_and(|l| t >= l) {
                    return None;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Play a beep and take as long as the original did.
    fn beep(&mut self, double: bool) {
        let (speed, mute) = {
            let s = self.link.lock().unwrap();
            (s.speed, s.mute)
        };
        if !mute {
            sound::play(double);
        }
        std::thread::sleep(Duration::from_secs_f64(kq1_apple2::sound::seconds(double) / speed));
    }
}

impl IoImpl for EngineIo {
    fn wait_key(&mut self, frame: &[u32]) -> u8 {
        self.wait(frame, None).unwrap_or(0x0D)
    }
    fn wait_key_timeout(&mut self, frame: &[u32], secs: f64) -> Option<u8> {
        self.wait(frame, Some(secs))
    }
    fn bell(&mut self) {
        self.beep(false);
    }
    fn double_bell(&mut self) {
        self.beep(true);
    }
    fn save(&mut self, slot: u8, data: &[u8]) -> Result<(), String> {
        if let Some((disk, path)) = self.disk.as_mut() {
            disk.save_game(slot, data)?;
            // Write a copy first, so a failed write cannot spoil the disk image.
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, disk.bytes()).map_err(|e| e.to_string())?;
            return fs::rename(&tmp, path).map_err(|e| e.to_string());
        }
        fs::create_dir_all(&self.saves).map_err(|e| e.to_string())?;
        fs::write(slot_path(&self.saves, slot), data).map_err(|e| e.to_string())
    }
    fn load(&mut self, slot: u8) -> Option<Vec<u8>> {
        if let Some((disk, _)) = self.disk.as_ref() {
            return disk.load_game(slot);
        }
        fs::read(slot_path(&self.saves, slot)).ok()
    }
    fn pause(&mut self, frame: &[u32], secs: f64, loading: bool) {
        publish(&self.link, frame);
        let start = Instant::now();
        loop {
            let (speed, fast) = {
                let s = self.link.lock().unwrap();
                if s.quit {
                    std::process::exit(0);
                }
                (s.speed, s.fast_loads)
            };
            if (loading && fast) || start.elapsed().as_secs_f64() * speed >= secs {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn joystick(&mut self) -> kq1_apple2::engine::Joystick {
        if !self.joystick {
            return Default::default();
        }
        // A press seen between passes counts too: the original also reads the buttons
        // during its redraw ($5D33-$5D45), so it sees a press within the pass.
        let mut j = joystick::read();
        let l = self.latch.replace([false; 2]);
        j.buttons = [j.buttons[0] || l[0], j.buttons[1] || l[1]];
        j
    }
    fn show_drawing(&mut self, draw: &kq1_apple2::engine::cost::PicDraw, frame: &dyn Fn(u32) -> Vec<u32>) {
        // As a pass's drawing in run_engine: the disk read, then the plots.
        let start = Instant::now();
        let mut shown = None;
        loop {
            let (speed, fast) = {
                let s = self.link.lock().unwrap();
                if s.quit {
                    std::process::exit(0);
                }
                (s.speed, s.fast_loads)
            };
            let (t0, t1) = draw.span(fast);
            let t = start.elapsed().as_secs_f64() * speed;
            if t >= t1 {
                return;
            }
            if t >= t0 {
                let n = draw.plots_at((t - t0) / (t1 - t0));
                if shown != Some(n) {
                    publish(&self.link, &frame(n));
                    shown = Some(n);
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

/// The joystick (kq1_frontend::joystick: winmm on Windows, /dev/input/js0 on Linux) as
/// Apple paddle counts: 1-255 across each axis, about 128 at rest; the hat (d-pad)
/// pushes to the edges.
mod joystick {
    pub fn read() -> kq1_apple2::engine::Joystick {
        let Some(s) = kq1_frontend::joystick::read() else { return Default::default() };
        let paddle = |v: u32| (1 + (v.min(65535) * 254 / 65535)) as u8;
        let (mut x, mut y) = (paddle(s.axes.0), paddle(s.axes.1));
        if let Some(degrees) = s.hat {
            let a = degrees.to_radians();
            x = (128.0 + 127.0 * a.sin()).round() as u8;
            y = (128.0 - 127.0 * a.cos()).round() as u8;
        }
        kq1_apple2::engine::Joystick { paddles: Some((x.max(1), y)), buttons: s.buttons }
    }
}

/// A file in `dir` named like an Apple IIe video ROM (342-0265 enhanced, 342-0133
/// unenhanced) that reads as one.
fn find_charrom(dir: &Path) -> Option<Vec<u8>> {
    let mut names: Vec<PathBuf> = fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    names.sort();
    for p in names {
        let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
        if !(name.contains("342-0265") || name.contains("342-0133")) {
            continue;
        }
        let Ok(rom) = fs::read(&p) else { continue };
        if kq1_apple2::engine::display::CharRom::from_rom(&rom).is_ok() {
            println!("text screen: the Apple IIe characters from {}", p.display());
            return Some(rom);
        }
    }
    None
}

/// Power-on (title screens and initialisation), or, as a developer option, a jump
/// straight into a room as kqroom does. Runs once the front end is attached.
fn start(w: &mut World, room: Option<u8>) {
    match room {
        Some(r) => {
            w.enter_room(r, 8);
            w.screen = w.compose();
        }
        None => w.boot(),
    }
}

/// Prints the engine's log lines as they appear.
struct Log(usize);
impl Log {
    fn flush(&mut self, w: &World) {
        for l in &w.log[self.0..] {
            println!("{l}");
        }
        self.0 = w.log.len();
    }
}

/// The bell through the Windows sound system (kq1_apple2::sound builds it).
#[cfg(windows)]
mod sound {
    use std::{ffi::c_void, sync::OnceLock};
    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> i32;
    }
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;
    static WAVS: OnceLock<[Vec<u8>; 2]> = OnceLock::new();

    pub fn play(double: bool) {
        let w = WAVS.get_or_init(|| [kq1_apple2::sound::wav(false, 44_100), kq1_apple2::sound::wav(true, 44_100)]);
        let wav = &w[double as usize];
        // The buffer is static, so it outlives the asynchronous playback.
        unsafe { PlaySoundW(wav.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
    }
}
/// The bell through cpal (ALSA, or PulseAudio/PipeWire through it), on a thread of its
/// own that keeps the output stream; beeps are queued as samples at the device's rate.
#[cfg(target_os = "linux")]
mod sound {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex, OnceLock},
    };

    struct Out {
        queue: Arc<Mutex<VecDeque<f32>>>,
        rate: u32,
    }
    static OUT: OnceLock<Option<Out>> = OnceLock::new();

    fn start() -> Option<Out> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let run = || -> Option<()> {
                let device = cpal::default_host().default_output_device()?;
                let config = device.default_output_config().ok()?;
                let rate = config.sample_rate().0;
                let channels = config.channels() as usize;
                let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
                let q = queue.clone();
                let stream = device
                    .build_output_stream(
                        &config.into(),
                        move |data: &mut [f32], _| {
                            let mut q = q.lock().unwrap();
                            for frame in data.chunks_mut(channels) {
                                let v = q.pop_front().unwrap_or(0.0);
                                frame.fill(v);
                            }
                        },
                        |e| eprintln!("sound: {e}"),
                        None,
                    )
                    .ok()?;
                stream.play().ok()?;
                println!("sound: {} at {rate} Hz", device.name().unwrap_or_default());
                tx.send(Some(Out { queue, rate })).ok()?;
                // Keep the stream alive for the life of the program.
                loop {
                    std::thread::park();
                }
            };
            if run().is_none() {
                eprintln!("sound: no audio output could be opened; the bell is silent");
                let _ = tx.send(None);
            }
        });
        rx.recv().ok().flatten()
    }

    pub fn play(double: bool) {
        let Some(out) = OUT.get_or_init(start) else { return };
        let samples = kq1_apple2::sound::samples(double, out.rate);
        out.queue.lock().unwrap().extend(samples.iter().map(|&s| s as f32 / 32768.0));
    }
}
#[cfg(not(any(windows, target_os = "linux")))]
mod sound {
    pub fn play(_double: bool) {
        print!("\x07");
    }
}

/// Double each line so the 560x192 frame keeps roughly the original shape.
fn doubled(frame: &[u32]) -> Vec<u32> {
    let mut v = Vec::with_capacity(frame.len() * 2);
    for row in frame.chunks(FRAME_W) {
        v.extend_from_slice(row);
        v.extend_from_slice(row);
    }
    v
}

/// The three game disk images: as given, or found in the folder given, or (none given)
/// next to the executable and then in the current folder. Exits with a message when
/// they can't be found.
fn disk_paths(args: &[String]) -> Vec<PathBuf> {
    if args.len() == 3 {
        return args.iter().map(PathBuf::from).collect();
    }
    let dirs: Vec<PathBuf> = match args.first() {
        Some(d) => vec![PathBuf::from(d)],
        None => {
            let exe_dir = env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
            let mut dirs: Vec<PathBuf> = exe_dir.into_iter().chain(env::current_dir().ok()).collect();
            dirs.dedup_by(|a, b| fs::canonicalize(&*a).ok() == fs::canonicalize(&*b).ok());
            dirs
        }
    };
    let mut why = Vec::new();
    for d in &dirs {
        match Disks::find(d) {
            Ok(p) => return p,
            Err(e) => why.push(e),
        }
    }
    fatal(&format!(
        "{}\n\nPut the game's three disk images (.nib, or .do/.dsk named with their disk number) next to kq, \
         or give their folder or the three files: kq disk1 disk2 disk3",
        why.join("\n")
    ))
}

/// Report an error that stops kq before its window opens, and exit. On Windows it is
/// shown in a message box too, since kq is often started from Explorer, whose console
/// window closes at once.
fn fatal(msg: &str) -> ! {
    kq1_frontend::fatal("King's Quest", msg)
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args[p + 1].clone();
        args.drain(p..p + 2);
        Some(v)
    };
    let fixed_rate: Option<f64> = opt("--rate").map(|r| r.parse().expect("--rate"));
    let scale: u32 = opt("--scale").map_or(2, |v| v.parse().expect("--scale"));
    let headless = opt("--headless");
    let saves = opt("--saves").map(PathBuf::from);
    let savedisk = opt("--savedisk").map(|p| {
        let disk = kq1_apple2::savedisk::SaveDisk::open(fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}")))
            .unwrap_or_else(|e| panic!("{p}: {e}"));
        println!("saving to the save disk {p}");
        (disk, PathBuf::from(p))
    });
    // The character ROM is checked here and decoded on the engine thread.
    let mut charrom_bytes = opt("--charrom").map(|p| {
        let rom = fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
        if let Err(e) = kq1_apple2::engine::display::CharRom::from_rom(&rom) {
            panic!("{p}: {e}");
        }
        rom
    });
    let trace_path = opt("--trace");
    let picbuf_path = opt("--picbuf");
    let start_room: Option<u8> = opt("--room").map(|r| r.parse().expect("--room"));
    let key_log: HashMap<usize, u8> = opt("--keys")
        .map(|p| {
            fs::read_to_string(&p)
                .expect("read key log")
                .lines()
                .filter_map(|l| {
                    let (n, k) = l.split_once(' ')?;
                    Some((n.trim().parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let last_key = key_log.keys().copied().max().unwrap_or(0);
    let speed: f64 = opt("--speed").map_or(1.0, |v| v.parse().expect("--speed"));
    let mut flag = |name: &str| match args.iter().position(|a| a == name) {
        Some(p) => {
            args.remove(p);
            true
        }
        None => false,
    };
    let fast_loads = flag("--fast-loads");
    let start_fullscreen = flag("--fullscreen");
    let mute = flag("--mute");
    let ntsc = flag("--ntsc");
    let specks = flag("--fill-specks");
    let use_joystick = !flag("--no-joystick");
    let swap_prompts = flag("--swap-prompts");
    let no_charrom = flag("--no-charrom");
    if args.iter().any(|a| a == "--help" || a == "-h") || !(args.len() <= 1 || args.len() == 3) {
        eprintln!("usage: kq [FOLDER | disk1 disk2 disk3] [--speed N] [--fast-loads] [--fullscreen] [--mute] [--ntsc] [--fill-specks] [--charrom FILE] [--no-charrom] [--swap-prompts] [--saves DIR] [--savedisk FILE] [--no-joystick] [--rate N] [--scale N] [--room N] [--keys FILE] [--headless PLAN]");
        eprintln!("With no disks given, kq looks for them next to itself, then in the current folder.");
        std::process::exit(2);
    }
    let disk_paths = disk_paths(&args);
    let disk_dir = disk_paths[0].parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    // A IIe video ROM next to the disk images is used for the text screen.
    if charrom_bytes.is_none() && !no_charrom {
        charrom_bytes = find_charrom(&disk_dir);
    }
    let saves = saves.unwrap_or_else(|| disk_dir.join("saves"));
    let disks = Disks::from_files(&disk_paths).unwrap_or_else(|e| fatal(&e));
    let new_world = move || World::new(Resources::load(&disks, false).expect("resources"));

    if let Some(plan) = headless {
        let mut w = new_world();
        let mut log = Log(0);
        w.io = Some(Box::new(Scripted::default()));
        start(&mut w, start_room);
        let mut cycles = 0;
        let mut trace = String::from(
            "pass,room,ego_x,ego_y,ego_dir,score,model_cycles,vars_hash
",
        );
        // A key log replays first (the same pass numbering as kqroute and refemu).
        if !key_log.is_empty() {
            for pass in 1..=last_key + 200 {
                if let Some(&k) = key_log.get(&pass) {
                    w.key(k);
                }
                w.cycle();
                cycles += 1;
                log.flush(&w);
                let model = w.take_cost().cycles();
                let hash = w.vars[..0xDF]
                    .iter()
                    .fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
                trace += &format!(
                    "{cycles},{},{},{},{},{},{model:.0},{hash:016x}
",
                    w.vars[1],
                    w.actors[0][f::X],
                    w.actors[0][f::Y],
                    w.actors[0][f::DIR],
                    w.vars[19]
                );
            }
        }
        for item in plan.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(cmd) = item.strip_prefix('>') {
                for c in cmd.bytes() {
                    w.key(c);
                }
                w.key(0x0D);
                w.cycle();
                cycles += 1;
                log.flush(&w);
                continue;
            }
            let split = item.find(|c: char| c.is_ascii_digit()).unwrap_or(item.len());
            let (d, n) = item.split_at(split);
            let n: usize = n.parse().unwrap_or(1);
            let dir = match d.to_ascii_uppercase().as_str() {
                "U" => 1,
                "UR" => 2,
                "R" => 3,
                "DR" => 4,
                "D" => 5,
                "DL" => 6,
                "L" => 7,
                "UL" => 8,
                _ => 0,
            };
            if w.g.key_dir != dir {
                let d = if dir == 0 { w.g.key_dir } else { dir };
                w.press_direction(d);
            }
            for _ in 0..n {
                let room = w.vars[1];
                w.cycle();
                cycles += 1;
                log.flush(&w);
                let model = w.take_cost().cycles();
                if std::env::var("KQ_DUMP").is_ok_and(|v| v.split(',').any(|x| x.parse::<usize>() == Ok(cycles))) {
                    let vals: Vec<String> = w.vars[..0xDF].iter().map(|v| v.to_string()).collect();
                    eprintln!("vars at end of pass {cycles}: {}", vals.join(","));
                }
                // Same hash as refemu: FNV-1a over v0..v222.
                let hash = w.vars[..0xDF]
                    .iter()
                    .fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
                trace += &format!(
                    "{cycles},{},{},{},{},{},{model:.0},{hash:016x}
",
                    w.vars[1],
                    w.actors[0][f::X],
                    w.actors[0][f::Y],
                    w.actors[0][f::DIR],
                    w.vars[19]
                );
                let e = &w.actors[0];
                if w.vars[1] != room || cycles % 10 == 0 {
                    println!(
                        "cycle {cycles:4}: room {:2} ego ({:3},{:3}) dir {} loop {} cel {} v3={}",
                        w.vars[1],
                        e[f::X],
                        e[f::Y],
                        e[f::DIR],
                        e[f::LOOP],
                        e[f::CEL],
                        w.vars[3]
                    );
                }
            }
        }
        if let Some(p) = &trace_path {
            fs::write(p, &trace).expect("write trace");
        }
        if let Some(p) = &picbuf_path {
            // The composed buffer: picture plus sprites, as in the engine's own buffer.
            fs::write(p, &w.screen).expect("write picture buffer");
        }
        println!("actors: {}", w.actor_summary());
        let carried: Vec<String> = (0..0x6F)
            .filter(|&o| w.objects[o][2] & 0x80 != 0)
            .map(|o| String::from_utf8_lossy(&w.object_name(o)).into_owned())
            .collect();
        println!("score {}, carrying {:?}", w.vars[19], carried);
        let rgb: Vec<u8> = w.render_frame().iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        fs::write("extracted/kq_last.png", kq1_apple2::png::encode_rgb(FRAME_W as u32, FRAME_H as u32, &rgb)).ok();
        return;
    }

    // The engine runs on its own thread at the original's pace; this thread only shows
    // its frames and collects keys.
    let link: Link = Arc::new(Mutex::new(Shared {
        frame: vec![0; FRAME_W * FRAME_H * 2],
        frame_seq: 0,
        keys: VecDeque::new(),
        speed,
        fast_loads,
        replaying: !key_log.is_empty(),
        mute,
        ntsc,
        specks,
        quit: false,
    }));
    let engine = {
        let link = link.clone();
        std::thread::spawn(move || {
            let mut w = new_world();
            w.display.charrom = charrom_bytes
                .map(|r| std::rc::Rc::new(kq1_apple2::engine::display::CharRom::from_rom(&r).expect("character ROM")));
            w.swap_prompts = swap_prompts;
            let latch = Rc::new(Cell::new([false; 2]));
            w.io = Some(Box::new(EngineIo {
                link: link.clone(),
                saves,
                disk: savedisk,
                joystick: use_joystick,
                latch: latch.clone(),
            }));
            if !key_log.is_empty() {
                println!("replaying {} keys to pass {last_key}; the keyboard is ignored until then", key_log.len());
            }
            start(&mut w, start_room);
            run_engine(w, &link, &key_log, last_key, fixed_rate, use_joystick.then_some(latch));
        })
    };

    // Sizes in real pixels (Windows display scaling would otherwise blur the picture and
    // misplace the full-screen window); the window starts at `scale` times the frame,
    // times the display scale.
    hook::dpi_aware();
    let opts = WindowOptions {
        scale: Scale::X1,
        resize: true,
        scale_mode: ScaleMode::AspectRatioStretch,
        ..WindowOptions::default()
    };
    let mut win = Window::new(TITLE, FRAME_W, FRAME_H * 2, opts).expect("window");
    hook::size_window(&win, FRAME_W as u32 * scale, FRAME_H as u32 * 2 * scale);
    win.set_target_fps(60);
    let typed: Queue = Rc::new(RefCell::new(VecDeque::new()));
    win.set_input_callback(Box::new(Chars(typed.clone())));
    let l = link.clone();
    hook::install(
        &win,
        hook::Picture { width: FRAME_W, height: FRAME_H * 2, aspect: (FRAME_W as u32, FRAME_H as u32 * 2) },
        Box::new(move |buf| buf.copy_from_slice(&l.lock().unwrap().frame)),
    );
    let mut full = Fullscreen::default();
    let mut want_full = start_fullscreen;
    match fixed_rate {
        Some(r) => println!("fixed rate {r} passes per second"),
        None => println!("original speed (modelled)"),
    }
    println!(
        "F11/F12: slower/faster, F9: fast loads, F8: sound, F7: NTSC colour, F6: fill specks, Alt+Enter: full screen"
    );
    let mut frame = vec![0u32; FRAME_W * FRAME_H * 2];
    let mut seq = 0;
    let mut title = String::new();
    while win.is_open() {
        {
            let s = link.lock().unwrap();
            if s.frame_seq != seq {
                seq = s.frame_seq;
                frame.copy_from_slice(&s.frame);
            }
        }
        win.update_with_buffer(&frame, FRAME_W, FRAME_H * 2).expect("update");
        // After an update, so that minifb sees the size change.
        #[cfg(not(windows))]
        let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
        #[cfg(not(windows))]
        let alt_enter = alt && win.is_key_pressed(Key::Enter, KeyRepeat::No);
        #[cfg(windows)]
        let alt_enter = false;
        if std::mem::take(&mut want_full) || hook::take_alt_enter() || alt_enter {
            full.toggle(&mut win);
        }
        let mut s = link.lock().unwrap();
        for k in win.get_keys_pressed(KeyRepeat::No) {
            let c = match k {
                Key::Up => 0x0B,
                Key::Down => 0x0A,
                Key::Left => 0x08,
                Key::Right => 0x15,
                Key::F12 => {
                    s.speed = (s.speed * 1.25).min(256.0);
                    continue;
                }
                Key::F11 => {
                    s.speed = (s.speed / 1.25).max(0.1);
                    continue;
                }
                Key::F9 => {
                    s.fast_loads = !s.fast_loads;
                    continue;
                }
                Key::F8 => {
                    s.mute = !s.mute;
                    continue;
                }
                Key::F7 => {
                    s.ntsc = !s.ntsc;
                    continue;
                }
                Key::F6 => {
                    s.specks = !s.specks;
                    continue;
                }
                _ => continue,
            };
            s.keys.push_back(c);
        }
        let chars: Vec<u8> = typed.borrow_mut().drain(..).collect();
        // Alt+key isn't typing (the Apple has no Alt); on Windows the hook drops it.
        #[cfg(not(windows))]
        if !alt {
            s.keys.extend(chars);
        }
        #[cfg(windows)]
        s.keys.extend(chars);
        let t = format!(
            "{TITLE} - speed x{:.2}{}{}{}{}{}",
            s.speed,
            if s.fast_loads { ", fast loads" } else { "" },
            if s.mute { ", muted" } else { "" },
            if s.ntsc { ", NTSC" } else { "" },
            if s.specks { ", specks filled" } else { "" },
            if s.replaying { ", replaying" } else { "" }
        );
        drop(s);
        if t != title {
            win.set_title(&t);
            title = t;
        }
    }
    link.lock().unwrap().quit = true;
    engine.join().ok();
}

/// The engine thread's loop: each pass lasts as long as the original's (engine::cost),
/// and its frame is shown when that time is up, as the original's screen changes at the
/// end of a pass. A pass that draws a picture (a room change) shows it being drawn: the
/// old room while the disk turns, then the picture plotted line by line on white, as the
/// original plots straight to the screen; the sprites come with the pass's end.
fn run_engine(
    mut w: World,
    link: &Link,
    key_log: &HashMap<usize, u8>,
    last_key: usize,
    fixed_rate: Option<f64>,
    latch: Option<Rc<Cell<[bool; 2]>>>,
) {
    let mut log = Log(0);
    let mut pass: usize = 0;
    let mut next = Instant::now();
    let mut pending: Option<Vec<u32>> = None;
    // The picture drawing of the current pass: its start, speed, fast loads, the draws,
    // and the last state shown (draw index, plots).
    let mut anim: Option<(Instant, f64, bool, Vec<kq1_apple2::engine::cost::PicDraw>, Option<(usize, u32)>)> = None;
    loop {
        let (speed, fast, replaying, keys) = {
            let mut s = link.lock().unwrap();
            if s.quit {
                return;
            }
            w.display.ntsc = s.ntsc;
            w.display.fill_specks = s.specks;
            let keys: Vec<u8> = s.keys.drain(..).collect();
            (s.speed, s.fast_loads, s.replaying, keys)
        };
        // Outside the lock: a key can open a screen that waits for another key (Tab, the
        // inventory), and that wait takes the lock.
        if !replaying {
            for k in keys {
                w.key(k);
            }
        }
        let now = Instant::now();
        if now >= next {
            if let Some(f) = pending.take() {
                publish(link, &f);
            }
            anim = None;
            // Run every pass that is due; at high speeds that is several per round (for
            // at most about 30 ms, so frames keep coming).
            while Instant::now() >= next && now.elapsed() < Duration::from_millis(30) {
                pass += 1;
                if let Some(&k) = key_log.get(&pass) {
                    w.key(k);
                }
                w.cycle();
                log.flush(&w);
                let c = w.take_cost();
                let secs = match fixed_rate {
                    Some(r) => 1.0 / r,
                    None if fast => c.seconds_fast_loads(),
                    None => c.seconds(),
                };
                let start = next;
                next += Duration::from_secs_f64(secs / speed);
                pending = Some(w.render_frame());
                let ending = replaying && pass >= last_key;
                if ending {
                    link.lock().unwrap().replaying = false;
                    println!("pass {pass}: key log finished (score {}); the keyboard is yours", w.vars[19]);
                }
                if fixed_rate.is_none() && !w.pic_draws.is_empty() && next > Instant::now() {
                    anim = Some((start, speed, fast, std::mem::take(&mut w.pic_draws), None));
                    break;
                }
                if ending {
                    break;
                }
            }
            if next < now {
                next = now;
            }
        }
        if let Some((start, speed, fast, draws, shown)) = anim.as_mut() {
            let t = Instant::now().saturating_duration_since(*start).as_secs_f64() * *speed;
            // The last drawing begun by now, and how far it has got.
            let state = draws.iter().enumerate().rev().find_map(|(i, d)| {
                let (t0, t1) = d.span(*fast);
                (t >= t0).then(|| {
                    let n = if t >= t1 { d.plots } else { d.plots_at((t - t0) / (t1 - t0)) };
                    (i, n)
                })
            });
            if let Some((i, n)) = state.filter(|&s| Some(s) != *shown) {
                publish(link, &w.render_frame_with(&draws[i].partial(n), false));
                *shown = state;
            }
        }
        // Between passes, keep an eye on the joystick buttons (EngineIo::joystick).
        if let Some(l) = &latch {
            let b = joystick::read().buttons;
            let old = l.get();
            l.set([old[0] || b[0], old[1] || b[1]]);
        }
        let wait = next.saturating_duration_since(Instant::now()).min(Duration::from_millis(15));
        std::thread::sleep(wait.max(Duration::from_millis(1)));
    }
}
