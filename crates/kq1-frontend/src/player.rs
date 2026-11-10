//! The half the IBM PC and PCjr players share: each keeps only its own screen, sound,
//! font and engine glue.
//! - [`Shared`]: what the window, the engine's thread and the sound share;
//! - the engine's clocks: [`PlayerClock`] (the original's pace by the wall clock, the
//!   keys and the joystick), [`RouteClock`] (a recorded run of the original), and
//!   [`ScriptClock`] (--headless: keys at given times, no pacing);
//! - [`Drives`]: the disk the game asks for, and the Play diskette's file;
//! - [`audio`]: the sound output, the port's [`audio::Render`] asked for samples;
//! - [`run_window`]: the window, its keys and the frames;
//! - [`fit`], [`find_disk`], [`joystick`].

use kq1_ibm::clock::{Clock, Event, Joystick, Replay, CPU_HZ, CYCLES_PER_TICK};
use minifb::{InputCallback, Key, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Between the window, the engine and the sound.
pub struct Shared {
    /// The screen in RGB, the port's frame (its size given to [`run_window`]), and a count
    /// of the frames sent. It is scaled to the window in one step, at 4:3, so that its
    /// rows and columns are even to within a pixel.
    pub rgb: Vec<u32>,
    pub seq: u64,
    /// Keys for the game (as the BIOS gives them: scan code << 8 | character).
    pub keys: VecDeque<u16>,
    pub speed: f64,
    /// Fast loads: the loading's time (World::loading_cycles) isn't waited for.
    pub fast_loads: bool,
    /// A route is being replayed (--route): the disks are its log's to swap.
    pub replaying: bool,
    pub quit: bool,
    /// Why the game stopped, when it has.
    pub ended: Option<String>,
}

impl Shared {
    pub fn new(pixels: usize, speed: f64, fast_loads: bool, replaying: bool) -> Arc<Mutex<Shared>> {
        Arc::new(Mutex::new(Shared {
            rgb: vec![0; pixels],
            seq: 0,
            keys: VecDeque::new(),
            speed,
            fast_loads,
            replaying,
            quit: false,
            ended: None,
        }))
    }
}

/// The game's time against the wall clock: the engine waits at each checkpoint until
/// the original would have got there, then takes the keys typed and the ticks due.
pub struct PlayerClock {
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
    pub fn new(shared: Arc<Mutex<Shared>>, speed: f64, joystick: bool) -> PlayerClock {
        PlayerClock { shared, origin: Instant::now(), speed, ticks: 0, loading: 0, skipped: 0, joystick }
    }

    /// Wait until the game's cycle `cycles` is due.
    pub fn wait_for(&mut self, cycles: u64) {
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
        self.joystick.then(joystick).flatten()
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

/// For --route: an event log of the original (the port's refemu `--events`) replayed at
/// its checkpoints, paced by the player's clock; when it runs out, the player's clock
/// takes over.
pub struct RouteClock {
    replay: Replay,
    play: PlayerClock,
    handed: bool,
}

impl RouteClock {
    pub fn new(replay: Replay, play: PlayerClock) -> RouteClock {
        RouteClock { replay, play, handed: false }
    }

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
pub struct ScriptClock {
    script: VecDeque<(u64, Vec<u16>)>,
    ticks: u64,
}

impl ScriptClock {
    /// "SECONDS:KEYS;..." (KEYS typed as text; `|` is Enter, `<U>` `<D>` `<L>` `<R>`
    /// the arrows, `<S>` the same arrow again, `<CL>` `<CR>` Ctrl+Left and Right, `<ESC>`
    /// Esc).
    pub fn parse(plan: &str) -> ScriptClock {
        let mut script = VecDeque::new();
        for item in plan.split(';').filter(|i| !i.trim().is_empty()) {
            let (t, keys) = item.split_once(':').expect("SECONDS:KEYS");
            let t: f64 = t.trim().parse().expect("SECONDS");
            let mut codes = Vec::new();
            let mut rest = keys;
            while let Some(c) = rest.chars().next() {
                let named = [
                    ("<U>", 0x4800),
                    ("<D>", 0x5000),
                    ("<L>", 0x4B00),
                    ("<R>", 0x4D00),
                    ("<CL>", 0x7300),
                    ("<CR>", 0x7400),
                    ("<ESC>", 0x011B),
                ];
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

/// The drive and the Play diskette, for the window and --headless alike: a disk the
/// game asks for goes in the drive (the player presses Enter), and the Play diskette's
/// image is written to its file once the game has stopped writing to it, and when the
/// game ends. The engine's state is given to it as values: the screen's text, the drive,
/// the Play diskette's image and its count of writes.
pub struct Drives {
    /// The Play diskette's file; none keeps it in memory (a route's).
    play: Option<PathBuf>,
    saved_writes: u64,
    pending: bool,
}

impl Drives {
    pub fn new(play: Option<PathBuf>) -> Drives {
        Drives { play, saved_writes: 0, pending: false }
    }

    /// The Play diskette's image from its file (`len` bytes), if there is one. The
    /// player puts it in the drive: the game plays from it once it has been made (it
    /// holds a copy of the game's data), and saves to it and restores from it without
    /// asking for it; the original's player boots the Master and then puts the Play
    /// diskette in.
    pub fn load(&mut self, len: usize, writes: u64) -> Option<Vec<u8>> {
        self.saved_writes = writes;
        self.play.as_ref().and_then(|p| std::fs::read(p).ok()).filter(|i| i.len() == len)
    }

    /// At each frame: `screen` the text on the screen, its rows in order.
    pub fn look(&mut self, screen: &str, drive: &mut usize, image: &[u8], writes: u64) {
        // The latest request on the screen: in graphics the text window scrolls (it
        // keeps the ones before, and the copy's label line names the Play diskette too);
        // the text screen, cleared, has its request at the top.
        let asked = screen.rfind("insert your");
        // A write error after it: the game writes only to the Play diskette.
        if screen.rfind("Write Error").is_some_and(|e| asked.is_none_or(|a| e > a)) {
            *drive = 1;
        } else if let Some(at) = asked {
            let asked = &screen[at..];
            if asked.contains("Master") {
                *drive = 0;
            } else if asked.contains("Play") {
                *drive = 1;
            }
        }
        if writes != self.saved_writes {
            self.saved_writes = writes;
            self.pending = true;
        } else if self.pending {
            self.write(image, writes);
        }
    }

    /// The Play diskette to its file.
    fn write(&mut self, image: &[u8], writes: u64) {
        self.pending = false;
        self.saved_writes = writes;
        if let Some(p) = &self.play {
            if let Err(e) = std::fs::write(p, image) {
                eprintln!("{}: {e}", p.display());
            }
        }
    }

    /// The game has ended: anything not yet written.
    pub fn finish(&mut self, image: &[u8], writes: u64) {
        if self.pending || writes != self.saved_writes {
            self.write(image, writes);
        }
    }
}

/// Where the screen goes in a `cw` × `ch` window: the largest 4:3 rectangle, centred
/// (x, y, width, height).
pub fn screen_rect(cw: usize, ch: usize) -> (usize, usize, usize, usize) {
    let (w, h) = if cw * 3 > ch * 4 { (ch * 4 / 3, ch) } else { (cw, cw * 3 / 4) };
    ((cw - w) / 2, (ch - h) / 2, w, h)
}

/// The `sw` × `sh` screen into a `cw` × `ch` buffer in one step (nearest pixel), in its
/// 4:3 rectangle, the rest black.
#[cfg_attr(windows, allow(dead_code))]
pub fn fit(src: &[u32], sw: usize, sh: usize, cw: usize, ch: usize, out: &mut Vec<u32>) {
    out.clear();
    out.resize(cw * ch, 0);
    let (x0, y0, w, h) = screen_rect(cw, ch);
    if w == 0 || h == 0 {
        return;
    }
    let cols: Vec<usize> = (0..w).map(|x| x * sw / w).collect();
    for y in 0..h {
        let row = &src[(y * sh / h) * sw..][..sw];
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

/// The keys that type no character: the walking keys (the numeric keypad gives these
/// with NumLock off, and digits with it on), and with Ctrl, Left and Right (scan codes
/// 73h and 74h, which the PC moves the screen with).
pub fn special(k: Key, ctrl: bool) -> Option<u16> {
    Some(match k {
        Key::Left if ctrl => 0x7300,
        Key::Right if ctrl => 0x7400,
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

/// The game's disk: given, or found next to the program, in the current folder or its
/// `original` folder: an `.mfm` or `.img` (not a Play diskette) that `is_game` takes,
/// the original's dump first. `missing` is the message when there is none.
pub fn find_disk(given: Option<&str>, is_game: impl Fn(Vec<u8>) -> bool, missing: &str) -> Result<PathBuf, String> {
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
        found.sort_by_key(|p| (p.extension().and_then(|e| e.to_str()) != Some("mfm"), p.clone()));
        if let Some(p) = found.into_iter().find(|p| is_game(std::fs::read(p).unwrap_or_default())) {
            return Ok(p);
        }
    }
    Err(missing.into())
}

/// The joystick (crate::joystick: winmm on Windows, /dev/input/js0 on Linux) as the
/// game reads it, from positions 0.0-1.0 across each axis; the hat (d-pad) pushes to
/// the edges.
pub fn joystick() -> Option<Joystick> {
    let s = crate::joystick::read()?;
    let (mut x, mut y) = (s.axes.0 as f64 / 65535.0, s.axes.1 as f64 / 65535.0);
    if let Some(degrees) = s.hat {
        let a = degrees.to_radians();
        (x, y) = (0.5 + 0.5 * a.sin(), 0.5 - 0.5 * a.cos());
    }
    Some(Joystick::from_position(x, y, s.buttons))
}

/// The sound output: the port's sound asked for samples, 1/30 s at a time, on a thread
/// of its own; silent while `on` is cleared (F8, --mute).
pub mod audio {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// The sample rate the ports' sound is made at.
    pub const RATE: u32 = 44_100;

    /// A port's sound: the next samples (mono, -1.0-1.0, at [`RATE`]).
    pub trait Render: Send {
        fn render(&mut self, out: &mut [f32]);
    }

    /// Start the output for `sound` (none on systems without one).
    pub fn start<R: Render + 'static>(sound: Arc<Mutex<R>>, on: Arc<AtomicBool>) {
        output::start(sound, on);
    }

    #[cfg(windows)]
    mod output {
        //! Sound through winmm's waveOut: four buffers of 1/30 s kept queued.
        use super::{Render, RATE as AUDIO_RATE};
        use std::ffi::c_void;
        use std::sync::atomic::{AtomicBool, Ordering};
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

        pub fn start<R: Render + 'static>(audio: Arc<Mutex<R>>, on: Arc<AtomicBool>) {
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
                            if !on.load(Ordering::Relaxed) {
                                f.fill(0.0);
                            }
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
    mod output {
        //! Sound through cpal (ALSA, or PulseAudio/PipeWire through it), on a thread of
        //! its own that keeps the stream.
        use super::Render;
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};

        pub fn start<R: Render + 'static>(audio: Arc<Mutex<R>>, on: Arc<AtomicBool>) {
            std::thread::spawn(move || {
                let run = || -> Option<()> {
                    let device = cpal::default_host().default_output_device()?;
                    let config = device.default_output_config().ok()?;
                    let channels = config.channels() as usize;
                    let rate = config.sample_rate().0;
                    if rate != super::RATE {
                        eprintln!("sound: the device runs at {rate} Hz; the sound plays at {} Hz", super::RATE);
                    }
                    let mut mono = Vec::new();
                    let stream = device
                        .build_output_stream(
                            &config.into(),
                            move |data: &mut [f32], _| {
                                mono.resize(data.len() / channels, 0.0);
                                audio.lock().unwrap().render(&mut mono);
                                if !on.load(Ordering::Relaxed) {
                                    mono.fill(0.0);
                                }
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
    mod output {
        pub fn start<R: super::Render + 'static>(
            _audio: std::sync::Arc<std::sync::Mutex<R>>,
            _on: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ) {
        }
    }
}

/// The window: how it starts.
pub struct WindowSetup {
    pub title: &'static str,
    /// The frame's size ([`Shared::rgb`]), shown at 4:3.
    pub width: usize,
    pub height: usize,
    /// N times 320 × 240 to start (--scale N).
    pub scale: usize,
    pub fullscreen: bool,
    /// The front end's keys, printed at the start.
    pub help: &'static str,
    /// For the error box's caption.
    pub caption: &'static str,
    /// Ctrl+Left and Ctrl+Right go to the game as themselves (the PC moves the screen
    /// with them), not as the arrows.
    pub ctrl_arrows: bool,
}

/// Run the window until it is closed or the game stops: the frames from `shared`, the
/// typed keys and the walking keys to the game, and the front end's keys: F8 the sound
/// (`sound_on`), F9 fast loads, F11/F12 the speed, Alt+Enter full screen. `extra` gets
/// every key pressed besides, with the window (to set its title), for the port's own.
pub fn run_window(
    setup: WindowSetup,
    shared: Arc<Mutex<Shared>>,
    sound_on: Arc<AtomicBool>,
    mut extra: impl FnMut(Key, &mut Window),
) {
    let WindowSetup { title, width, height, scale, fullscreen, help, caption, ctrl_arrows } = setup;
    crate::hook::dpi_aware();
    // N times 320 × 240 (the screen at its 4:3 shape); on Windows the hook sizes it
    // again at the display's scale.
    let scale = scale.max(1);
    let mut window = Window::new(
        title,
        320 * scale,
        240 * scale,
        WindowOptions {
            resize: true,
            scale: Scale::X1,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .unwrap_or_else(|e| crate::fatal(caption, &format!("no window: {e}")));
    crate::hook::size_window(&window, 320 * scale as u32, 240 * scale as u32);
    let l = shared.clone();
    crate::hook::install(
        &window,
        crate::hook::Picture { width, height, aspect: (4, 3) },
        Box::new(move |buf| buf.copy_from_slice(&l.lock().unwrap().rgb)),
    );
    window.set_input_callback(Box::new(Chars(shared.clone())));
    let mut full = crate::fullscreen::Fullscreen::default();
    let mut want_full = fullscreen;
    println!("{help}");
    let fast_title = |on: bool| format!("{title}{}", if on { " - fast loads" } else { "" });
    if shared.lock().unwrap().fast_loads {
        window.set_title(&fast_title(true));
    }
    let mut buf = vec![0u32; width * height];
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
        window.update_with_buffer(&buf, width, height).ok();
        // Elsewhere: scaled here to the window's size, so minifb doesn't scale again.
        #[cfg(not(windows))]
        {
            let (cw, ch) = window.get_size();
            let (cw, ch) = (cw.max(1), ch.max(1));
            fit(&buf, width, height, cw, ch, &mut fitted);
            window.update_with_buffer(&fitted, cw, ch).ok();
        }
        // After an update, so that minifb sees the size change.
        #[cfg(not(windows))]
        let alt_enter = (window.is_key_down(Key::LeftAlt) || window.is_key_down(Key::RightAlt))
            && window.is_key_pressed(Key::Enter, KeyRepeat::No);
        #[cfg(windows)]
        let alt_enter = false;
        if std::mem::take(&mut want_full) || crate::hook::take_alt_enter() || alt_enter {
            full.toggle(&mut window);
        }
        let ctrl = ctrl_arrows && (window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl));
        for k in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(code) = special(k, ctrl) {
                shared.lock().unwrap().keys.push_back(code);
            }
            match k {
                Key::F8 => {
                    let on = !sound_on.load(Ordering::Relaxed);
                    sound_on.store(on, Ordering::Relaxed);
                    window.set_title(&format!("{title}{}", if on { "" } else { " - muted" }));
                }
                Key::F9 => {
                    let mut s = shared.lock().unwrap();
                    s.fast_loads = !s.fast_loads;
                    window.set_title(&fast_title(s.fast_loads));
                }
                Key::F11 | Key::F12 => {
                    let mut s = shared.lock().unwrap();
                    s.speed = if k == Key::F12 { (s.speed * 2.0).min(16.0) } else { (s.speed / 2.0).max(0.25) };
                    window.set_title(&format!("{title} - speed x{}", s.speed));
                }
                k => extra(k, &mut window),
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    shared.lock().unwrap().quit = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scaled in one step, the screen's rows are all within a pixel of each other in
    /// height, and its columns in width, whatever the window's size; and the picture
    /// keeps its 4:3 shape. For both ports' frames (the PCjr's 320 × 200, the PC's
    /// 640 × 200).
    #[test]
    fn rows_and_columns_scale_evenly() {
        for (sw, sh) in [(320, 200), (640, 200)] {
            // Each source pixel its own colour number, so that the output says where each is.
            let src: Vec<u32> = (0..(sw * sh) as u32).collect();
            let mut out = Vec::new();
            for (cw, ch) in [(640, 480), (960, 720), (1440, 1080), (1000, 700), (1920, 1080), (801, 599)] {
                fit(&src, sw, sh, cw, ch, &mut out);
                let (x0, y0, w, h) = screen_rect(cw, ch);
                assert!((w as f64 / h as f64 - 4.0 / 3.0).abs() < 0.01, "{cw}x{ch}: {w}x{h}");
                let mut heights = vec![0usize; sh];
                for y in 0..h {
                    heights[out[(y0 + y) * cw + x0] as usize / sw] += 1;
                }
                let mut widths = vec![0usize; sw];
                for x in 0..w {
                    widths[out[y0 * cw + x0 + x] as usize % sw] += 1;
                }
                for (name, v) in [("rows", &heights), ("columns", &widths)] {
                    let (lo, hi) = (v.iter().min().unwrap(), v.iter().max().unwrap());
                    assert!(hi - lo <= 1 && *lo > 0, "{sw}x{sh} in {cw}x{ch}: {name} from {lo} to {hi} pixels");
                }
            }
        }
    }
}
