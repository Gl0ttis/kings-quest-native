//! kqplay <disk1> <disk2> <disk3> [--speed N] [--fast-loads] [--fullscreen] [--mute]
//!        [--rate N] [--scale N] [--room N] [--saves DIR] [--keys FILE] [--headless PLAN]
//!
//! The disks are the game's three disk images, as .nib (any order: each records its
//! disk number) or decoded DOS-order .do/.dsk (in order).
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
//! F8 sound on/off (--mute). F7 NTSC colour fringing (--ntsc). F6 fills the white specks
//! where the picture's fills miss a pixel (--fill-specks; display only). Alt+Enter full
//! screen (--fullscreen). The window can be resized; the picture keeps its shape.
//!
//! --savedisk FILE saves to and restores from an original save disk image (disk 4,
//! volume 10; .nib or .do), which the original on an emulator can read, and the other
//! way round. Without it, saves are files in saves/ (--saves DIR).
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
//! command and press RETURN. The final frame is written to extracted/kqplay_last.png.
use kq1_apple2::engine::text::{FRAME_H, FRAME_W};
use kq1_apple2::engine::{f, IoImpl, Resources, Scripted, World};
use kq1_apple2::kq::Disks;
use minifb::{InputCallback, Key, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};
use std::{
    cell::RefCell,
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
        if self.joystick {
            joystick::read()
        } else {
            Default::default()
        }
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
                let n = ((t - t0) / (t1 - t0) * draw.plots as f64) as u32;
                if shown != Some(n) {
                    publish(&self.link, &frame(n));
                    shown = Some(n);
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

/// The joystick through Windows (winmm: any game controller), as Apple paddle counts:
/// 0-255 across each axis, about 128 at rest; the hat (d-pad) pushes to the edges.
#[cfg(windows)]
mod joystick {
    #[repr(C)]
    #[derive(Default)]
    struct JoyInfoEx {
        size: u32,
        flags: u32,
        x: u32,
        y: u32,
        z: u32,
        r: u32,
        u: u32,
        v: u32,
        buttons: u32,
        button_number: u32,
        pov: u32,
        reserved1: u32,
        reserved2: u32,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn joyGetPosEx(id: u32, info: *mut JoyInfoEx) -> u32;
    }

    pub fn read() -> kq1_apple2::engine::Joystick {
        let mut j = JoyInfoEx { size: std::mem::size_of::<JoyInfoEx>() as u32, flags: 0xFF, ..Default::default() };
        if unsafe { joyGetPosEx(0, &mut j) } != 0 {
            return Default::default();
        }
        let paddle = |v: u32| (1 + (v.min(65535) * 254 / 65535)) as u8;
        let (mut x, mut y) = (paddle(j.x), paddle(j.y));
        if j.pov != 0xFFFF {
            let a = (j.pov as f64 / 100.0).to_radians();
            x = (128.0 + 127.0 * a.sin()).round() as u8;
            y = (128.0 - 127.0 * a.cos()).round() as u8;
        }
        kq1_apple2::engine::Joystick { paddles: Some((x.max(1), y)), buttons: [j.buttons & 1 != 0, j.buttons & 2 != 0] }
    }
}
#[cfg(not(windows))]
mod joystick {
    pub fn read() -> kq1_apple2::engine::Joystick {
        Default::default()
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
#[cfg(not(windows))]
mod sound {
    pub fn play(_double: bool) {
        print!("\x07");
    }
}

/// Borderless full screen on the window's monitor (Alt+Enter), and back.
#[derive(Default)]
struct Fullscreen {
    #[cfg(windows)]
    saved: Option<(isize, win32::Rect)>,
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    #[repr(C)]
    pub struct MonitorInfo {
        pub size: u32,
        pub monitor: Rect,
        pub work: Rect,
        pub flags: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        pub fn GetWindowLongPtrW(h: *mut c_void, index: i32) -> isize;
        pub fn SetWindowLongPtrW(h: *mut c_void, index: i32, value: isize) -> isize;
        pub fn GetWindowRect(h: *mut c_void, r: *mut Rect) -> i32;
        pub fn MonitorFromWindow(h: *mut c_void, flags: u32) -> *mut c_void;
        pub fn GetMonitorInfoW(m: *mut c_void, info: *mut MonitorInfo) -> i32;
        pub fn SetWindowPos(h: *mut c_void, after: *mut c_void, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    pub const GWL_STYLE: i32 = -16;
    pub const WS_OVERLAPPEDWINDOW: isize = 0x00CF_0000;
    pub const WS_POPUP: isize = 0x8000_0000;
    pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_FRAMECHANGED: u32 = 0x0020;
    pub const SWP_NOOWNERZORDER: u32 = 0x0200;
}

impl Fullscreen {
    #[cfg(windows)]
    fn toggle(&mut self, win: &mut Window) {
        use win32::*;
        let h = win.get_window_handle();
        unsafe {
            match self.saved.take() {
                None => {
                    let style = GetWindowLongPtrW(h, GWL_STYLE);
                    let mut r = Rect::default();
                    GetWindowRect(h, &mut r);
                    let mut mi = MonitorInfo {
                        size: std::mem::size_of::<MonitorInfo>() as u32,
                        monitor: Rect::default(),
                        work: Rect::default(),
                        flags: 0,
                    };
                    if GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
                        return;
                    }
                    SetWindowLongPtrW(h, GWL_STYLE, (style & !WS_OVERLAPPEDWINDOW) | WS_POPUP);
                    let m = mi.monitor;
                    SetWindowPos(
                        h,
                        std::ptr::null_mut(),
                        m.left,
                        m.top,
                        m.right - m.left,
                        m.bottom - m.top,
                        SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                    self.saved = Some((style, r));
                    win.set_cursor_visibility(false);
                }
                Some((style, r)) => {
                    SetWindowLongPtrW(h, GWL_STYLE, style);
                    SetWindowPos(
                        h,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                    win.set_cursor_visibility(true);
                }
            }
        }
    }
    #[cfg(not(windows))]
    fn toggle(&mut self, _win: &mut Window) {
        println!("full screen is only implemented on Windows");
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
    let saves = PathBuf::from(opt("--saves").unwrap_or_else(|| "saves".into()));
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
    let no_charrom = flag("--no-charrom");
    if args.len() != 3 {
        eprintln!("usage: kqplay disk1 disk2 disk3 (.nib or .do) [--speed N] [--fast-loads] [--fullscreen] [--mute] [--ntsc] [--fill-specks] [--charrom FILE] [--no-charrom] [--savedisk FILE] [--no-joystick] [--rate N] [--scale N] [--room N] [--keys FILE] [--headless PLAN]");
        std::process::exit(2);
    }
    // A IIe video ROM next to the disk images is used for the text screen.
    if charrom_bytes.is_none() && !no_charrom {
        charrom_bytes = find_charrom(Path::new(&args[0]).parent().unwrap_or(Path::new(".")));
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let new_world = move || World::new(Resources::load(&disks, true).expect("resources"));

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
        fs::write("extracted/kqplay_last.png", kq1_apple2::png::encode_rgb(FRAME_W as u32, FRAME_H as u32, &rgb)).ok();
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
            w.io = Some(Box::new(EngineIo { link: link.clone(), saves, disk: savedisk, joystick: use_joystick }));
            if !key_log.is_empty() {
                println!("replaying {} keys to pass {last_key}; the keyboard is ignored until then", key_log.len());
            }
            start(&mut w, start_room);
            run_engine(w, &link, &key_log, last_key, fixed_rate);
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
    hook::size_window(&win, scale);
    win.set_target_fps(60);
    let typed: Queue = Rc::new(RefCell::new(VecDeque::new()));
    win.set_input_callback(Box::new(Chars(typed.clone())));
    hook::install(&win, link.clone());
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
        if std::mem::take(&mut want_full) || hook::take_alt_enter() {
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
fn run_engine(mut w: World, link: &Link, key_log: &HashMap<usize, u8>, last_key: usize, fixed_rate: Option<f64>) {
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
                    let n = if t >= t1 { d.plots } else { ((t - t0) / (t1 - t0) * d.plots as f64) as u32 };
                    (i, n)
                })
            });
            if let Some((i, n)) = state.filter(|&s| Some(s) != *shown) {
                publish(link, &w.render_frame_with(&draws[i].partial(n), false));
                *shown = state;
            }
        }
        let wait = next.saturating_duration_since(Instant::now()).min(Duration::from_millis(15));
        std::thread::sleep(wait.max(Duration::from_millis(1)));
    }
}

/// A hook in front of minifb's window procedure (Windows): Alt+Enter, and all painting,
/// including while the window is dragged or resized, when Windows runs its own loop and
/// the main loop (and minifb) wait.
#[cfg(windows)]
mod hook {
    use super::{Link, FRAME_H, FRAME_W};
    use std::{cell::RefCell, ffi::c_void};
    type Hwnd = *mut c_void;
    type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowLongPtrW(h: Hwnd, index: i32, value: isize) -> isize;
        fn CallWindowProcW(prev: isize, h: Hwnd, msg: u32, w: usize, l: isize) -> isize;
        fn SetTimer(h: Hwnd, id: usize, ms: u32, f: *const c_void) -> usize;
        fn KillTimer(h: Hwnd, id: usize) -> i32;
        fn GetClientRect(h: Hwnd, r: *mut Rect) -> i32;
        fn GetDC(h: Hwnd) -> *mut c_void;
        fn ReleaseDC(h: Hwnd, dc: *mut c_void) -> i32;
        fn FillRect(dc: *mut c_void, r: *const Rect, brush: *mut c_void) -> i32;
        fn ValidateRect(h: Hwnd, r: *const Rect) -> i32;
        fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
        fn GetDpiForWindow(h: Hwnd) -> u32;
        fn GetWindowRect(h: Hwnd, r: *mut Rect) -> i32;
        fn SetWindowPos(h: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn StretchDIBits(
            dc: *mut c_void,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            sx: i32,
            sy: i32,
            sw: i32,
            sh: i32,
            bits: *const c_void,
            info: *const BitmapInfoHeader,
            usage: u32,
            rop: u32,
        ) -> i32;
        fn GetStockObject(i: i32) -> *mut c_void;
    }
    const GWLP_WNDPROC: i32 = -4;
    const WM_PAINT: u32 = 0x000F;
    const WM_ERASEBKGND: u32 = 0x0014;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSCHAR: u32 = 0x0106;
    const WM_TIMER: u32 = 0x0113;
    const WM_ENTERSIZEMOVE: u32 = 0x0231;
    const WM_EXITSIZEMOVE: u32 = 0x0232;
    const VK_RETURN: usize = 0x0D;
    const TIMER: usize = 0x4B51;

    struct Hook {
        prev: isize,
        link: Link,
        alt_enter: bool,
        frame: Vec<u32>,
    }
    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Per-monitor DPI awareness (before any window exists).
    pub fn dpi_aware() {
        unsafe { SetProcessDpiAwarenessContext(-4) };
    }

    /// Make the client area `scale` times the frame at the window's display scale.
    pub fn size_window(win: &minifb::Window, scale: u32) {
        let h = win.get_window_handle();
        unsafe {
            let dpi = GetDpiForWindow(h).max(96) as i32;
            let (mut wr, mut cr) = (Rect::default(), Rect::default());
            GetWindowRect(h, &mut wr);
            GetClientRect(h, &mut cr);
            let cw = FRAME_W as i32 * scale as i32 * dpi / 96;
            let ch = FRAME_H as i32 * 2 * scale as i32 * dpi / 96;
            let bw = (wr.right - wr.left) - cr.right;
            let bh = (wr.bottom - wr.top) - cr.bottom;
            SetWindowPos(h, std::ptr::null_mut(), 0, 0, cw + bw, ch + bh, 0x0002 | 0x0004);
            // NOMOVE | NOZORDER
        }
    }

    pub fn install(win: &minifb::Window, link: Link) {
        let h = win.get_window_handle();
        let prev = unsafe { SetWindowLongPtrW(h, GWLP_WNDPROC, proc as WndProc as usize as isize) };
        HOOK.with(|k| {
            *k.borrow_mut() = Some(Hook { prev, link, alt_enter: false, frame: vec![0; FRAME_W * FRAME_H * 2] })
        });
    }

    /// Alt+Enter was pressed since the last call.
    pub fn take_alt_enter() -> bool {
        HOOK.with(|k| k.borrow_mut().as_mut().is_some_and(|h| std::mem::take(&mut h.alt_enter)))
    }

    unsafe extern "system" fn proc(h: Hwnd, msg: u32, wp: usize, lp: isize) -> isize {
        // SAFETY: Win32 calls on this thread's own window handle.
        unsafe {
            // The borrow ends before minifb's procedure runs: it can re-enter this one.
            let prev = HOOK.with(|k| k.borrow().as_ref().map_or(0, |k| k.prev));
            match msg {
                WM_SYSKEYDOWN if wp == VK_RETURN => {
                    if lp & (1 << 30) == 0 {
                        HOOK.with(|k| k.borrow_mut().as_mut().map(|k| k.alt_enter = true));
                    }
                    return 0;
                }
                // The paint covers the whole window, so no erase (it would flicker).
                WM_ERASEBKGND => return 1,
                // Alt+key characters: the Apple has no Alt key, so none go to the game.
                WM_SYSCHAR => return 0,
                // Windows runs its own loop while the window is dragged or resized; a timer
                // keeps the picture coming meanwhile.
                WM_ENTERSIZEMOVE => {
                    SetTimer(h, TIMER, 15, std::ptr::null());
                }
                WM_EXITSIZEMOVE => {
                    KillTimer(h, TIMER);
                }
                WM_TIMER if wp == TIMER => {
                    paint(h);
                    return 0;
                }
                // All painting is done here (minifb's own stretch misplaces the picture
                // in a full-screen window).
                WM_PAINT => {
                    paint(h);
                    ValidateRect(h, std::ptr::null());
                    return 0;
                }
                _ => {}
            }
            CallWindowProcW(prev, h, msg, wp, lp)
        }
    }

    /// Draw the engine's latest frame, keeping its shape, with black bars.
    fn paint(h: Hwnd) {
        // SAFETY: GDI calls on this thread's own window and device context; the frame
        // buffer outlives the call.
        HOOK.with(|k| unsafe {
            let mut k = k.borrow_mut();
            let Some(k) = k.as_mut() else { return };
            {
                let s = k.link.lock().unwrap();
                k.frame.copy_from_slice(&s.frame);
            }
            let mut r = Rect::default();
            GetClientRect(h, &mut r);
            let (cw, ch) = (r.right, r.bottom);
            if cw <= 0 || ch <= 0 {
                return;
            }
            let (bw, bh) = (FRAME_W as i32, FRAME_H as i32 * 2);
            let (w, hh) = if cw * bh > ch * bw { (ch * bw / bh, ch) } else { (cw, cw * bh / bw) };
            let (x, y) = ((cw - w) / 2, (ch - hh) / 2);
            let dc = GetDC(h);
            let black = GetStockObject(4);
            for b in [
                Rect { left: 0, top: 0, right: cw, bottom: y },
                Rect { left: 0, top: y + hh, right: cw, bottom: ch },
                Rect { left: 0, top: 0, right: x, bottom: ch },
                Rect { left: x + w, top: 0, right: cw, bottom: ch },
            ] {
                if b.right > b.left && b.bottom > b.top {
                    FillRect(dc, &b, black);
                }
            }
            let info = BitmapInfoHeader {
                size: 40,
                width: bw,
                height: -bh,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                clr_used: 0,
                clr_important: 0,
            };
            StretchDIBits(dc, x, y, w, hh, 0, 0, bw, bh, k.frame.as_ptr() as *const c_void, &info, 0, 0x00CC_0020);
            ReleaseDC(h, dc);
        });
    }
}
#[cfg(not(windows))]
mod hook {
    pub fn dpi_aware() {}
    pub fn size_window(_win: &minifb::Window, _scale: u32) {}
    pub fn install(_win: &minifb::Window, _link: super::Link) {}
    pub fn take_alt_enter() -> bool {
        false
    }
}
