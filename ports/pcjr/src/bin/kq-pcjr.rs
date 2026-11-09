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

use kq1_frontend::{fullscreen::Fullscreen, hook};
use kq1_pcjr::disk::{Disk, IMAGE_LEN};
use kq1_pcjr::engine::clock::{Clock, Event, Joystick, Replay, CPU_HZ, CYCLES_PER_TICK};
use kq1_pcjr::engine::screen::{HEIGHT, WIDTH};
use kq1_pcjr::engine::World;
use kq1_pcjr::font::Font;
use kq1_pcjr::sound::Chip;
use minifb::{InputCallback, Key, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TITLE: &str = "King's Quest (IBM PCjr, native engine)";
/// The output buffer: the PCjr's 320 × 200, each row and column scaled to a 4:3 shape.
const OUT_W: usize = 320;
const OUT_H: usize = 200;
const AUDIO_RATE: u32 = 44_100;

/// Between the window, the engine and the sound.
struct Shared {
    /// The screen in RGB, the PCjr's 320 × 200 (OUT_W × OUT_H), and a count of the
    /// frames sent. It is scaled to the window in one step (4:3, its pixels 1.2 times
    /// as tall as wide), so that its rows and columns are even to within a pixel.
    rgb: Vec<u32>,
    seq: u64,
    /// Keys for the game (as the PCjr's BIOS gives them: scan code << 8 | character).
    keys: VecDeque<u16>,
    speed: f64,
    /// Fast loads: the loading's time (World::loading_cycles) isn't waited for.
    fast_loads: bool,
    /// A route is being replayed (--route): the disks are its log's to swap.
    replaying: bool,
    quit: bool,
    /// Why the game stopped, when it has.
    ended: Option<String>,
}

/// The sound chip's bytes, and the chip itself, for the sound output.
struct Audio {
    chip: Chip,
    writes: VecDeque<u8>,
    on: bool,
}

impl Audio {
    fn render(&mut self, out: &mut [f32]) {
        for b in self.writes.drain(..) {
            self.chip.write(b);
        }
        for o in out.iter_mut() {
            let s = self.chip.sample(AUDIO_RATE as f64);
            *o = if self.on { s * 0.8 } else { 0.0 };
        }
    }
}

/// The game's time against the wall clock: the engine waits at each checkpoint until
/// the original would have got there, then takes the keys typed and the ticks due.
struct PlayerClock {
    shared: Arc<Mutex<Shared>>,
    /// The wall-clock instant of the game's cycle 0, at the current speed.
    origin: Instant,
    speed: f64,
    ticks: u64,
    /// The loading cycles so far, and of them those skipped by fast loads (not waited
    /// for: the wall clock runs `skipped` cycles behind the game's).
    loading: u64,
    skipped: u64,
    /// Read the joystick (not --no-joystick).
    joystick: bool,
}

impl PlayerClock {
    fn new(shared: Arc<Mutex<Shared>>, speed: f64, joystick: bool) -> PlayerClock {
        PlayerClock { shared, origin: Instant::now(), speed, ticks: 0, loading: 0, skipped: 0, joystick }
    }

    /// Wait until the game's cycle `cycles` is due.
    fn wait_for(&mut self, cycles: u64) {
        let cycles = cycles.saturating_sub(self.skipped);
        let speed = self.shared.lock().unwrap().speed;
        if speed != self.speed {
            // Keep the present where it is when the speed changes.
            let now = Instant::now();
            let at = Duration::from_secs_f64(cycles as f64 / CPU_HZ / speed);
            self.origin = now.checked_sub(at).unwrap_or(now);
            self.speed = speed;
        }
        let due = self.origin + Duration::from_secs_f64(cycles as f64 / CPU_HZ / self.speed);
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        } else if now - due > Duration::from_millis(250) {
            // Far behind (the window was dragged, the machine was busy): don't rush to
            // catch up.
            self.origin += now - due;
        }
    }

    fn keys(&mut self) -> Vec<Event> {
        self.shared.lock().unwrap().keys.drain(..).map(Event::Key).collect()
    }
}

impl Clock for PlayerClock {
    fn at(&mut self, _n: u64, cycles: u64) -> Vec<Event> {
        self.wait_for(cycles);
        let mut out = self.keys();
        while (self.ticks + 1) * CYCLES_PER_TICK <= cycles {
            self.ticks += 1;
            out.push(Event::Tick(0xFFFF));
        }
        out
    }

    /// A busy wait: until the next tick.
    fn waiting(&mut self, _n: u64, _cycles: u64) -> Vec<Event> {
        self.wait_for((self.ticks + 1) * CYCLES_PER_TICK);
        self.ticks += 1;
        let mut out = self.keys();
        out.push(Event::Tick(0xFFFF));
        out
    }

    /// A timed delay (the title's): its time passes in real time too.
    fn poll(&mut self, cycles: u64) -> Vec<Event> {
        self.at(0, cycles)
    }

    fn joystick(&mut self) -> Option<Joystick> {
        self.joystick.then(joystick::read).flatten()
    }

    fn pace(&mut self, cycles: u64) {
        self.wait_for(cycles);
    }

    fn loading(&mut self, cycles: u64) {
        if self.shared.lock().unwrap().fast_loads {
            self.skipped += cycles - self.loading;
        }
        self.loading = cycles;
    }
}

/// For --route: a refemu-pcjr event log replayed at its checkpoints, paced by the
/// player's clock; when it runs out, the player's clock takes over.
struct RouteClock {
    replay: Replay,
    play: PlayerClock,
    handed: bool,
}

impl RouteClock {
    /// Whether the log is spent (handing over to the player at `cycles` if so).
    fn over(&mut self, cycles: u64) -> bool {
        if !self.handed && self.replay.remaining() == 0 {
            self.handed = true;
            self.play.ticks = cycles / CYCLES_PER_TICK;
            let mut s = self.play.shared.lock().unwrap();
            s.keys.clear();
            s.replaying = false;
            println!("the route is over: the game is yours");
        }
        self.handed
    }

    fn typed_keys_ignored(&mut self) {
        self.play.shared.lock().unwrap().keys.clear();
    }
}

impl Clock for RouteClock {
    fn at(&mut self, n: u64, cycles: u64) -> Vec<Event> {
        if self.over(cycles) {
            return self.play.at(n, cycles);
        }
        self.play.wait_for(cycles);
        self.typed_keys_ignored();
        self.replay.at(n, cycles)
    }
    fn waiting(&mut self, n: u64, cycles: u64) -> Vec<Event> {
        if self.over(cycles) {
            return self.play.waiting(n, cycles);
        }
        self.replay.waiting(n, cycles)
    }
    fn poll(&mut self, cycles: u64) -> Vec<Event> {
        if self.over(cycles) {
            return self.play.poll(cycles);
        }
        self.play.wait_for(cycles);
        self.replay.poll(cycles)
    }
    fn joystick(&mut self) -> Option<Joystick> {
        if self.handed {
            self.play.joystick()
        } else {
            None
        }
    }
    fn loading(&mut self, cycles: u64) {
        self.play.loading(cycles);
    }
    fn pace(&mut self, cycles: u64) {
        self.play.wait_for(cycles);
    }
}

/// For --headless: the keys typed at given times of the game, no pacing.
struct ScriptClock {
    script: VecDeque<(u64, Vec<u16>)>,
    ticks: u64,
}

impl ScriptClock {
    /// "SECONDS:KEYS;..." (KEYS typed as text; `|` is Enter, `<U>` `<D>` `<L>` `<R>`
    /// the arrows, `<S>` the same arrow again).
    fn parse(plan: &str) -> ScriptClock {
        let mut script = VecDeque::new();
        for item in plan.split(';').filter(|i| !i.trim().is_empty()) {
            let (t, keys) = item.split_once(':').expect("SECONDS:KEYS");
            let t: f64 = t.trim().parse().expect("SECONDS");
            let mut codes = Vec::new();
            let mut rest = keys;
            while let Some(c) = rest.chars().next() {
                let named = [("<U>", 0x4800), ("<D>", 0x5000), ("<L>", 0x4B00), ("<R>", 0x4D00)];
                if let Some((n, k)) = named.iter().find(|(n, _)| rest.starts_with(n)) {
                    codes.push(*k);
                    rest = &rest[n.len()..];
                    continue;
                }
                codes.push(match c {
                    '|' => 0x1C0D,
                    ' ' => 0x3920,
                    c => c as u16,
                });
                rest = &rest[c.len_utf8()..];
            }
            script.push_back(((t * CPU_HZ) as u64, codes));
        }
        ScriptClock { script, ticks: 0 }
    }

    fn due(&mut self, cycles: u64) -> Vec<Event> {
        let mut out = Vec::new();
        while self.script.front().is_some_and(|(t, _)| *t <= cycles) {
            out.extend(self.script.pop_front().unwrap().1.into_iter().map(Event::Key));
        }
        out
    }
}

impl Clock for ScriptClock {
    fn at(&mut self, _n: u64, cycles: u64) -> Vec<Event> {
        let mut out = self.due(cycles);
        while (self.ticks + 1) * CYCLES_PER_TICK <= cycles {
            self.ticks += 1;
            out.push(Event::Tick(0xFFFF));
        }
        out
    }
    fn waiting(&mut self, _n: u64, _cycles: u64) -> Vec<Event> {
        self.ticks += 1;
        let mut out = self.due(self.ticks * CYCLES_PER_TICK);
        out.push(Event::Tick(0xFFFF));
        out
    }
    fn poll(&mut self, cycles: u64) -> Vec<Event> {
        self.at(0, cycles)
    }
}

/// --headless: play the script, then write the screen at `until` seconds of the game.
fn headless(disk: &Disk, font: &Font, plan: &str, until: f64, shot: &str, rate: Option<u64>, room: Option<u8>) {
    let mut w = World::new(disk, Box::new(ScriptClock::parse(plan))).expect("the engine");
    w.fixed_pass_cycles = rate;
    w.start_room = room;
    let end = (until * CPU_HZ) as u64;
    let shot_path = shot.to_string();
    let f = font.clone();
    w.on_frame = Some(Box::new(move |w: &mut World| {
        if w.cycles < end {
            return;
        }
        let mut screen = vec![0u8; 320 * 200];
        render(w, &f, &mut screen);
        let rgb: Vec<u8> = screen.iter().flat_map(|&c| kq1_pcjr::color::rgb(c)).collect();
        std::fs::write(&shot_path, kq1_formats::png::encode_rgb(320, 200, &rgb)).expect("write the shot");
        let text: Vec<String> = (0..25).map(|r| w.text.row(r)).filter(|r| !r.trim().is_empty()).collect();
        println!(
            "at {:.1} s ({:.1} s of it loading: the disk and the pictures), pass {}, room {}:\n{}",
            w.cycles as f64 / CPU_HZ,
            w.loading_cycles as f64 / CPU_HZ,
            w.passes,
            w.vars[1],
            text.join("\n")
        );
        w.stop("the end of the script".into());
    }));
    w.run();
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

/// Where the screen goes in a `cw` × `ch` window: the largest 4:3 rectangle, centred
/// (x, y, width, height).
fn screen_rect(cw: usize, ch: usize) -> (usize, usize, usize, usize) {
    let (w, h) = if cw * 3 > ch * 4 { (ch * 4 / 3, ch) } else { (cw, cw * 3 / 4) };
    ((cw - w) / 2, (ch - h) / 2, w, h)
}

/// The 320 × 200 screen into a `cw` × `ch` buffer in one step (nearest pixel), in its
/// 4:3 rectangle, the rest black.
#[cfg_attr(windows, allow(dead_code))]
fn fit(src: &[u32], cw: usize, ch: usize, out: &mut Vec<u32>) {
    out.clear();
    out.resize(cw * ch, 0);
    let (x0, y0, w, h) = screen_rect(cw, ch);
    if w == 0 || h == 0 {
        return;
    }
    let cols: Vec<usize> = (0..w).map(|x| x * OUT_W / w).collect();
    for y in 0..h {
        let row = &src[(y * OUT_H / h) * OUT_W..][..OUT_W];
        let line = &mut out[(y0 + y) * cw + x0..][..w];
        for (o, &sx) in line.iter_mut().zip(&cols) {
            *o = row[sx];
        }
    }
}

/// Typed characters, from the window's callback.
struct Chars(Arc<Mutex<Shared>>);
impl InputCallback for Chars {
    fn add_char(&mut self, c: u32) {
        let k: u16 = match c {
            0x0D => 0x1C0D,
            0x08 => 0x0E08,
            0x1B => 0x011B,
            0x09 => 0x0F09,
            0x20 => 0x3920,
            0x01..=0x1A | 0x21..=0x7E => c as u16,
            _ => return,
        };
        self.0.lock().unwrap().keys.push_back(k);
    }
}

/// The keys that type no character: the walking keys. (The numeric keypad gives these
/// with NumLock off, and digits with it on.)
fn special(k: Key) -> Option<u16> {
    Some(match k {
        Key::Up => 0x4800,
        Key::Down => 0x5000,
        Key::Left => 0x4B00,
        Key::Right => 0x4D00,
        Key::Home => 0x4700,
        Key::PageUp => 0x4900,
        Key::End => 0x4F00,
        Key::PageDown => 0x5100,
        _ => return None,
    })
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
    if let Some(p) = given {
        return Ok(PathBuf::from(p));
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(d);
    }
    dirs.push(PathBuf::from("."));
    dirs.push(PathBuf::from("original"));
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut found: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                (ext == "mfm" || ext == "img") && !name.contains("Play")
            })
            .collect();
        // The original's dump first, then any image of the right size.
        found.sort_by_key(|p| (p.extension().and_then(|e| e.to_str()) != Some("mfm"), p.clone()));
        if let Some(p) = found.into_iter().find(|p| Disk::from_file(std::fs::read(p).unwrap_or_default()).is_ok()) {
            return Ok(p);
        }
    }
    Err("No disk image of King's Quest (IBM PCjr) was found.\n\nPut the game's disk image (an HxC .mfm dump or a \
         360K .img) next to kq, or give its path: kq DISK"
        .into())
}

#[cfg(windows)]
mod audio_out {
    //! Sound through winmm's waveOut: four buffers of 1/30 s kept queued.
    use super::{Audio, AUDIO_RATE};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};

    #[repr(C)]
    struct WaveFormatEx {
        format_tag: u16,
        channels: u16,
        samples_per_sec: u32,
        avg_bytes_per_sec: u32,
        block_align: u16,
        bits_per_sample: u16,
        size: u16,
    }
    #[repr(C)]
    struct WaveHdr {
        data: *mut i16,
        buffer_length: u32,
        bytes_recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut WaveHdr,
        reserved: usize,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn waveOutOpen(
            h: *mut *mut c_void,
            dev: u32,
            fmt: *const WaveFormatEx,
            cb: usize,
            inst: usize,
            flags: u32,
        ) -> u32;
        fn waveOutPrepareHeader(h: *mut c_void, hdr: *mut WaveHdr, size: u32) -> u32;
        fn waveOutWrite(h: *mut c_void, hdr: *mut WaveHdr, size: u32) -> u32;
    }
    const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
    const WHDR_DONE: u32 = 1;
    const BUFFERS: usize = 4;
    const SAMPLES: usize = AUDIO_RATE as usize / 30;

    pub fn start(audio: Arc<Mutex<Audio>>) {
        std::thread::spawn(move || unsafe {
            let fmt = WaveFormatEx {
                format_tag: 1,
                channels: 1,
                samples_per_sec: AUDIO_RATE,
                avg_bytes_per_sec: AUDIO_RATE * 2,
                block_align: 2,
                bits_per_sample: 16,
                size: 0,
            };
            let mut h: *mut c_void = std::ptr::null_mut();
            if waveOutOpen(&mut h, WAVE_MAPPER, &fmt, 0, 0, 0) != 0 {
                eprintln!("sound: no audio output could be opened; playing silently");
                return;
            }
            let mut data: Vec<Vec<i16>> = (0..BUFFERS).map(|_| vec![0i16; SAMPLES]).collect();
            let mut hdrs: Vec<WaveHdr> = data
                .iter_mut()
                .map(|d| WaveHdr {
                    data: d.as_mut_ptr(),
                    buffer_length: (SAMPLES * 2) as u32,
                    bytes_recorded: 0,
                    user: 0,
                    flags: WHDR_DONE,
                    loops: 0,
                    next: std::ptr::null_mut(),
                    reserved: 0,
                })
                .collect();
            let size = std::mem::size_of::<WaveHdr>() as u32;
            for hdr in hdrs.iter_mut() {
                waveOutPrepareHeader(h, hdr, size);
                hdr.flags |= WHDR_DONE;
            }
            let mut f = vec![0f32; SAMPLES];
            loop {
                for (i, hdr) in hdrs.iter_mut().enumerate() {
                    if std::ptr::read_volatile(&hdr.flags) & WHDR_DONE != 0 {
                        audio.lock().unwrap().render(&mut f);
                        for (d, s) in data[i].iter_mut().zip(&f) {
                            *d = (s * 32767.0) as i16;
                        }
                        hdr.flags &= !WHDR_DONE;
                        waveOutWrite(h, hdr, size);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
    }
}

#[cfg(target_os = "linux")]
mod audio_out {
    //! Sound through cpal (ALSA, or PulseAudio/PipeWire through it), on a thread of
    //! its own that keeps the stream.
    use super::Audio;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    pub fn start(audio: Arc<Mutex<Audio>>) {
        std::thread::spawn(move || {
            let run = || -> Option<()> {
                let device = cpal::default_host().default_output_device()?;
                let config = device.default_output_config().ok()?;
                let channels = config.channels() as usize;
                let rate = config.sample_rate().0;
                if rate != super::AUDIO_RATE {
                    eprintln!("sound: the device runs at {rate} Hz; the music plays at {} Hz", super::AUDIO_RATE);
                }
                let mut mono = Vec::new();
                let stream = device
                    .build_output_stream(
                        &config.into(),
                        move |data: &mut [f32], _| {
                            mono.resize(data.len() / channels, 0.0);
                            audio.lock().unwrap().render(&mut mono);
                            for (frame, s) in data.chunks_mut(channels).zip(&mono) {
                                frame.fill(*s);
                            }
                        },
                        |e| eprintln!("sound: {e}"),
                        None,
                    )
                    .ok()?;
                stream.play().ok()?;
                loop {
                    std::thread::park();
                }
            };
            if run().is_none() {
                eprintln!("sound: no audio output could be opened; playing silently");
            }
        });
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod audio_out {
    pub fn start(_audio: std::sync::Arc<std::sync::Mutex<super::Audio>>) {}
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
        headless(&disk, &font, &plan, until, &shot, rate, room);
        return;
    }
    let play_path = play_file
        .map(PathBuf::from)
        .unwrap_or_else(|| disk_path.parent().unwrap_or(Path::new(".")).join("KQ1 Play.img"));

    let shared = Arc::new(Mutex::new(Shared {
        rgb: vec![0; OUT_W * OUT_H],
        seq: 0,
        keys: VecDeque::new(),
        speed,
        fast_loads,
        replaying: route.is_some(),
        quit: false,
        ended: None,
    }));
    let route_log = route.map(|r| {
        let file = if Path::new(&r).is_file() {
            PathBuf::from(&r)
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("reference").join(format!("{r}.events"))
        };
        std::fs::read_to_string(&file).unwrap_or_else(|e| fatal(&format!("{}: {e}", file.display())))
    });
    let audio = Arc::new(Mutex::new(Audio { chip: Chip::default(), writes: VecDeque::new(), on: !mute }));
    audio_out::start(audio.clone());

    // The engine, on a thread of its own (its state isn't shared: the screen goes out
    // as frames).
    {
        let shared = shared.clone();
        let audio = audio.clone();
        std::thread::spawn(move || {
            let play = PlayerClock::new(shared.clone(), speed, use_joystick);
            let replaying = route_log.is_some();
            let clock: Box<dyn Clock> = match route_log {
                Some(log) => Box::new(RouteClock { replay: Replay::parse(&log), play, handed: false }),
                None => Box::new(play),
            };
            let mut w = World::new(&disk, clock).expect("the engine");
            w.fixed_pass_cycles = rate;
            w.start_room = room;
            // A route starts with the Play diskette the original had (a blank one), and
            // keeps it in memory.
            if !replaying {
                if let Ok(img) = std::fs::read(&play_path) {
                    if img.len() == IMAGE_LEN {
                        w.disks[1].image = img;
                    }
                }
            }
            let mut screen = vec![0u8; 320 * 200];
            let mut rgb = vec![0u32; OUT_W * OUT_H];
            let colours = palette();
            let mut saved_writes = w.disks[1].writes;
            let mut pending_save = false;
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
                // A disk asked for goes in the drive (the player presses Enter).
                let text: String = (21..25).map(|r| w.text.row(r)).collect::<Vec<_>>().join(" ");
                if text.contains("insert your") && text.contains("Play") {
                    w.drive = 1;
                } else if text.contains("insert your") && text.contains("Master") {
                    w.drive = 0;
                }
                // The Play diskette, written back once the game has stopped writing.
                if w.disks[1].writes != saved_writes {
                    saved_writes = w.disks[1].writes;
                    pending_save = true;
                } else if pending_save {
                    pending_save = false;
                    if let Err(e) = std::fs::write(&play_path, &w.disks[1].image) {
                        eprintln!("{}: {e}", play_path.display());
                    }
                }
            }));
            let why = w.run();
            shared.lock().unwrap().ended = Some(why);
        });
    }

    hook::dpi_aware();
    // --scale N times 320 × 240 (the screen at its 4:3 shape); on Windows the hook sizes
    // it again at the display's scale.
    let scale = scale_n.unwrap_or(3).max(1);
    let mut window = Window::new(
        TITLE,
        320 * scale,
        240 * scale,
        WindowOptions {
            resize: true,
            scale: Scale::X1,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .unwrap_or_else(|e| fatal(&format!("no window: {e}")));
    hook::size_window(&window, 320 * scale as u32, 240 * scale as u32);
    let l = shared.clone();
    hook::install(
        &window,
        hook::Picture { width: OUT_W, height: OUT_H, aspect: (4, 3) },
        Box::new(move |buf| buf.copy_from_slice(&l.lock().unwrap().rgb)),
    );
    window.set_input_callback(Box::new(Chars(shared.clone())));
    let mut full = Fullscreen::default();
    let mut want_full = start_fullscreen;
    println!("F11/F12: slower/faster, F9: fast loads, F8: sound, Alt+Enter: full screen");
    let fast_title = |on: bool| format!("{TITLE}{}", if on { " - fast loads" } else { "" });
    if fast_loads {
        window.set_title(&fast_title(true));
    }
    let mut buf = vec![0u32; OUT_W * OUT_H];
    #[cfg(not(windows))]
    let mut fitted: Vec<u32> = Vec::new();
    let mut last_seq = u64::MAX;
    while window.is_open() {
        let (seq, ended) = {
            let s = shared.lock().unwrap();
            if s.seq != last_seq {
                buf.copy_from_slice(&s.rgb);
            }
            (s.seq, s.ended.clone())
        };
        last_seq = seq;
        if let Some(why) = ended {
            println!("the game stopped: {why}");
            break;
        }
        // Windows: the hook paints, stretching the screen to the window itself.
        #[cfg(windows)]
        window.update_with_buffer(&buf, OUT_W, OUT_H).ok();
        // Elsewhere: scaled here to the window's size, so minifb doesn't scale again.
        #[cfg(not(windows))]
        {
            let (cw, ch) = window.get_size();
            let (cw, ch) = (cw.max(1), ch.max(1));
            fit(&buf, cw, ch, &mut fitted);
            window.update_with_buffer(&fitted, cw, ch).ok();
        }
        // After an update, so that minifb sees the size change.
        #[cfg(not(windows))]
        let alt_enter = (window.is_key_down(Key::LeftAlt) || window.is_key_down(Key::RightAlt))
            && window.is_key_pressed(Key::Enter, KeyRepeat::No);
        #[cfg(windows)]
        let alt_enter = false;
        if std::mem::take(&mut want_full) || hook::take_alt_enter() || alt_enter {
            full.toggle(&mut window);
        }
        for k in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(code) = special(k) {
                shared.lock().unwrap().keys.push_back(code);
            }
            match k {
                Key::F8 => {
                    let mut a = audio.lock().unwrap();
                    a.on = !a.on;
                    window.set_title(&format!("{TITLE}{}", if a.on { "" } else { " - muted" }));
                }
                Key::F9 => {
                    let mut s = shared.lock().unwrap();
                    s.fast_loads = !s.fast_loads;
                    window.set_title(&fast_title(s.fast_loads));
                }
                Key::F11 | Key::F12 => {
                    let mut s = shared.lock().unwrap();
                    s.speed = if k == Key::F12 { (s.speed * 2.0).min(16.0) } else { (s.speed / 2.0).max(0.25) };
                    window.set_title(&format!("{TITLE} - speed x{}", s.speed));
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    shared.lock().unwrap().quit = true;
}

/// Report an error that stops kq before its window opens, and exit (in a message box
/// too on Windows).
fn fatal(msg: &str) -> ! {
    kq1_frontend::fatal("King's Quest (PCjr)", msg)
}

/// The joystick (kq1_frontend::joystick: winmm on Windows, /dev/input/js0 on Linux) as
/// positions 0.0-1.0 across each axis; the hat (d-pad) pushes to the edges.
mod joystick {
    use kq1_pcjr::engine::clock::Joystick;

    pub fn read() -> Option<Joystick> {
        let s = kq1_frontend::joystick::read()?;
        let (mut x, mut y) = (s.axes.0 as f64 / 65535.0, s.axes.1 as f64 / 65535.0);
        if let Some(degrees) = s.hat {
            let a = degrees.to_radians();
            (x, y) = (0.5 + 0.5 * a.sin(), 0.5 - 0.5 * a.cos());
        }
        Some(Joystick::from_position(x, y, s.buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scaled in one step, the screen's rows are all within a pixel of each other in
    /// height, and its columns in width, whatever the window's size; and the picture
    /// keeps its 4:3 shape.
    #[test]
    fn rows_and_columns_scale_evenly() {
        // Each source pixel its own colour number, so that the output says where each is.
        let src: Vec<u32> = (0..(OUT_W * OUT_H) as u32).collect();
        let mut out = Vec::new();
        for (cw, ch) in [(640, 480), (960, 720), (1440, 1080), (1000, 700), (1920, 1080), (801, 599)] {
            fit(&src, cw, ch, &mut out);
            let (x0, y0, w, h) = screen_rect(cw, ch);
            assert!((w as f64 / h as f64 - 4.0 / 3.0).abs() < 0.01, "{cw}x{ch}: {w}x{h}");
            let mut heights = vec![0usize; OUT_H];
            for y in 0..h {
                heights[out[(y0 + y) * cw + x0] as usize / OUT_W] += 1;
            }
            let mut widths = vec![0usize; OUT_W];
            for x in 0..w {
                widths[out[y0 * cw + x0 + x] as usize % OUT_W] += 1;
            }
            for (name, v) in [("rows", &heights), ("columns", &widths)] {
                let (lo, hi) = (v.iter().min().unwrap(), v.iter().max().unwrap());
                assert!(hi - lo <= 1 && *lo > 0, "{cw}x{ch}: {name} from {lo} to {hi} pixels");
            }
        }
    }
}
