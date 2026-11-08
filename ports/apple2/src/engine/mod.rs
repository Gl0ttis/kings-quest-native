//! Native reimplementation of the King's Quest (Apple II) engine state and routines.
//!
//! This follows the documented behaviour of the 6502 engine routine by routine (see
//! docs/script-format.md, docs/view-format.md and docs/motion.md). Addresses in comments
//! name the routine being reproduced. There is no CPU emulation: actors are kept as their
//! original 31-byte records so that field-level behaviour, including 8-bit wraparound,
//! carries over exactly.

pub mod cost;
pub mod cycle;
pub mod display;
pub mod parser;
pub mod save;
pub mod text;
pub use text::{Io, IoImpl, Joystick, Scripted};
pub mod vm;

use crate::kq::{DiskAddr, Disks};
use crate::pic::{self, HEIGHT, WIDTH};
use crate::view::{self, View};
use std::rc::Rc;

/// Actor table size: $0100-$017B holds four 31-byte records ($473F refuses a fifth).
pub const MAX_ACTORS: usize = 4;

/// Actor record field offsets (engine table at $0100, 31 bytes per actor).
pub mod f {
    pub const ROOM: usize = 0x00; // $FF = not in any room
    pub const LOOP: usize = 0x03;
    pub const NO_MOVE: usize = 0x04; // 1 = ignores barriers/movement step, 2 = blocked this cycle
    pub const VIEW: usize = 0x05; // view number (for v6/v21 reporting and saving)
    pub const LOOP_COUNT: usize = 0x06;
    pub const CEL_COUNT: usize = 0x07;
    pub const CEL: usize = 0x08;
    pub const DELAY: usize = 0x09;
    pub const COUNTDOWN: usize = 0x0A;
    pub const FLAGS: usize = 0x0B; // bit0 fixed priority, bit1/2 collision options, bit3 above horizon
    pub const STEP_X: usize = 0x0C;
    pub const STEP_Y: usize = 0x0D;
    pub const CYCLE: usize = 0x0E;
    pub const MOTION: usize = 0x0F;
    pub const DIR: usize = 0x10;
    pub const SLOT: usize = 0x13;
    pub const X: usize = 0x18;
    pub const Y: usize = 0x19;
    pub const NEW_X: usize = 0x1A;
    pub const NEW_Y: usize = 0x1B;
    pub const INVISIBLE: usize = 0x1C;
    pub const PRIORITY: usize = 0x1D;
    pub const DRAWN: usize = 0x1E;
}

/// A rectangle of the picture buffer saved from under an actor.
#[derive(Clone)]
pub struct SavedBg {
    pub x: usize,
    pub top: usize,
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// Write a saved block into a picture-sized buffer with its top left at (x, top),
/// leaving out what falls outside the picture.
fn put_block(buf: &mut [u8], b: &SavedBg, x: i32, top: i32) {
    for r in 0..b.h as i32 {
        let y = top + r;
        if !(0..HEIGHT as i32).contains(&y) {
            continue;
        }
        for c in 0..b.w as i32 {
            let xx = x + c;
            if (0..WIDTH as i32).contains(&xx) {
                buf[y as usize * WIDTH + xx as usize] = b.data[(r * b.w as i32 + c) as usize];
            }
        }
    }
}

/// Start-up times from power-on, in seconds (refemu, REFEMU_PCLOG): the engine's screen
/// set-up ($452C), the title ($4015), initialisation ($4018) and the first pass ($4026).
pub const BOOT_SCREEN_SETUP: f64 = 5.620;
pub const BOOT_TITLE: f64 = 12.759;
pub const BOOT_INIT: f64 = 31.549;
pub const BOOT_FIRST_PASS: f64 = 33.422;
/// The title ($432F), per picture (82, the Sierra logo; 83, the title scroll): loading
/// it ($4208), drawing it ($5FA4) and printing its text, in seconds (refemu); then each
/// waits 3.840 s for a key ($4396: 65,280 polls).
const TITLE_TIMES: [(f64, f64, f64); 2] = [(1.917, 1.219, 0.253), (2.202, 5.103, 0.416)];
const TITLE_WAIT: f64 = 3.840;

/// All game resources, decoded once from the disk images.
#[derive(Default, Clone)]
pub struct Resources {
    pub pictures: Vec<Option<Rc<Vec<u8>>>>,
    pub views: Vec<Option<Rc<View>>>,
    pub scripts: Vec<Option<Rc<Vec<u8>>>>,
    pub objects: Vec<[u8; 8]>,
    pub engine: Vec<u8>,
    /// Parser vocabulary (fixed resource at $0800).
    pub vocab: Vec<u8>,
    /// Font (fixed resource at aux $1D00).
    pub font: Vec<u8>,
    /// Sectors the loader reads for each picture, view and script (for timing).
    pub sectors: [Vec<u32>; 3],
    /// The disk (volume) each picture, view and script asks for; 0 for the shared tracks.
    pub disks: [Vec<u8>; 3],
}

impl Resources {
    pub fn load(disks: &Disks, fix_script_064: bool) -> Result<Self, String> {
        // Each entry: the resource bytes, and how many sectors the loader reads for it.
        // Each entry: the resource bytes, how many sectors the loader reads for it, and
        // the disk (volume) it asks for (0: the shared tracks, on every disk).
        let dir = |page: u16| -> Result<(Vec<Option<Vec<u8>>>, Vec<u32>, Vec<u8>), String> {
            let mut data = Vec::new();
            let mut sectors = Vec::new();
            let mut want = Vec::new();
            for (i, r) in disks.directory(page)?.iter().enumerate() {
                let kind = crate::kq::DIRS.iter().find(|d| d.0 == page).map_or("", |d| d.1);
                let res = if r[0] == 0 || crate::kq::UNUSED.contains(&(kind, i)) {
                    None
                } else {
                    disks.read(DiskAddr::from_record(r), 1).ok()
                };
                sectors.push(res.as_ref().map_or(0, |x| ((x.span.end - 1) / 256 - x.span.start / 256 + 1) as u32));
                want.push(if res.is_some() { DiskAddr::from_record(r).disk } else { 0 });
                data.push(res.map(|x| x.data.to_vec()));
            }
            Ok((data, sectors, want))
        };
        let (pics, pic_sectors, pic_disks) = dir(0x1700)?;
        let pictures = pics.into_iter().map(|p| p.map(Rc::new)).collect();
        let (vs, view_sectors, view_disks) = dir(0x1900)?;
        let views = vs.into_iter().map(|v| v.and_then(|d| view::decode(&d).ok()).map(Rc::new)).collect();
        let (mut scripts, script_sectors, script_disks) = dir(0x1800)?;
        if fix_script_064 {
            // Original data defect: section 1 is declared one byte longer than it is
            // (docs/script-format.md). Port policy: use the corrected boundary.
            if let Some(Some(s)) = scripts.get_mut(64) {
                if s.len() >= 4 && s[2] == 65 && s[3] == 0 {
                    s[2] = 64;
                }
            }
        }
        let objects = disks.fixed(0x6790)?.data.chunks(8).map(|c| c.try_into().unwrap()).collect();
        Ok(Resources {
            pictures,
            views,
            scripts: scripts.into_iter().map(|s| s.map(Rc::new)).collect(),
            objects,
            engine: disks.engine().to_vec(),
            vocab: disks.fixed(0x6787)?.data.to_vec(),
            font: disks.fixed(0x6784)?.data.to_vec(),
            sectors: [pic_sectors, view_sectors, script_sectors],
            disks: [pic_disks, view_disks, script_disks],
        })
    }
}

/// Engine zero-page and global variables that are not script variables.
#[derive(Clone, Default)]
pub struct Globals {
    pub actor_count: u8,  // $60
    pub barrier_x: u8,    // $61
    pub barrier_y: u8,    // $62
    pub barrier: [u8; 4], // $63-$66
    pub ego_auto: u8,     // $67: 1 = program controls the ego
    pub ego_auto2: u8,    // $68
    pub target_x: u8,     // $69
    pub target_y: u8,     // $6A
    pub slack: [u8; 2],   // $6B/$6C
    pub follow_dist: u8,  // $6D
    pub follow_state: u8, // $6E
    pub wander_timer: u8, // $6F
    pub special: u8,      // $73
    pub joy_dir: u8,      // $74
    /// Joystick thresholds from power-on ($4817): $F4/$F5 (x) and $F6/$F7 (y). $F5 = 0
    /// means no joystick.
    pub joy_x: (u8, u8),
    pub joy_y: (u8, u8),
    pub key_dir: u8,         // $75
    pub cel_w: u8,           // $F2: width of the last selected cel
    pub cel_h: u8,           // $F3: height of the last selected cel
    pub saved_motion: u8,    // $6EFE
    pub saved_loop: u8,      // $6651
    pub saved_cel: u8,       // $6650
    pub said: [u8; 2],       // $9A/$9B
    pub said_matched: u8,    // $6F2E
    pub restore_request: u8, // $7030
    pub flag_6f05: u8,       // $6F05
    pub nudge: [u8; 3],      // $702D-$702F
    pub nudge_limit: u8,     // $701F
    pub rng_ptr: u16,        // $4B/$4C
    /// Self-modified JSR operands in the engine image, which the RNG reads like any other
    /// engine byte (0 = still as loaded): $5561/$5562 holds the last motion handler
    /// ($5557, from table $6AE4) and $58D7/$58D8 the last action handler ($58CC, from
    /// table $6AF3). `word_cmp` is the low byte of the parser's `CMP $6A70,X` operand at
    /// $523F ($70 or $84, set by $5228/$522E; $5240 is always $6A).
    pub motion_jsr: u16,
    pub action_jsr: u16,
    pub word_cmp: u8,
    pub cycle: u8,         // $4D
    pub last_pos: [u8; 2], // $7020/$7021: position at the last motion update
    /// $57/$58/$55/$56: the moving actor's x, y, step x and step y, loaded by the motion
    /// dispatcher ($5531) for slot `disp_slot` (0xFF: none). `$55F8` (arrival) reuses
    /// $55/$57 as the loop/cel to select and zeroes them, so the follow motion that goes
    /// on in the same call sees x = 0 and step x = 0.
    pub disp: [u8; 4],
    pub disp_slot: u8,
    pub edge_toggle: u8, // $6CCF
    pub patrol_side: u8, // $76
    /// Set by restart: the rest of the current pass is abandoned ($4018 resets the stack).
    pub abort_pass: bool,
}

pub struct World {
    pub res: Resources,
    pub vars: [u8; 256],
    pub objects: Vec<[u8; 8]>,
    pub actors: [[u8; 31]; MAX_ACTORS + 1],
    /// Which view's data each actor's view pointer (fields 1-2) refers to.
    pub actor_view: [Option<u8>; MAX_ACTORS + 1],
    pub loaded_views: [bool; 256],
    pub room_script: Option<Rc<Vec<u8>>>,
    pub pic_d000: Option<u8>,
    pub pic_d800: Option<u8>,
    /// Picture buffer ($4000 aux): low nibble visual, high nibble priority. Includes
    /// objects added to the picture, but not ordinary sprites.
    pub base: Vec<u8>,
    pub g: Globals,
    pub log: Vec<String>,
    /// The last screen produced by a draw pass.
    pub screen: Vec<u8>,
    pub text: text::Text,
    pub io: Option<Io>,
    /// Background saved under each actor (aux $15BF), restored when it is erased ($15F5).
    pub saved_bg: [Option<SavedBg>; MAX_ACTORS + 1],
    /// The cel width each actor was last drawn with (the old cel of aux $13C4).
    pub drawn_w: [u8; MAX_ACTORS + 1],
    /// The height of the cel last committed for each slot (with `drawn_w`: the old cel
    /// of the next update rectangle).
    pub drawn_h: [u8; MAX_ACTORS + 1],
    /// Work done since `take_cost` (for the original-speed model).
    pub cost: cost::Cost,
    /// Pictures drawn this pass, for front ends that show the drawing (take_pic_draws).
    pub pic_draws: Vec<cost::PicDraw>,
    /// Display options (not game state).
    pub display: display::Display,
    /// Show the original's disk-swap prompts ("Please insert side B and press RETURN.")
    /// when the game reads from another disk (kq --swap-prompts). Off by default:
    /// the port reads every disk up front, so there is nothing to insert.
    pub swap_prompts: bool,
    /// The disk "in the drive" ($6F3E: the volume last read).
    pub drive: u8,
}

/// A copy of the whole game state, without the front end (`io` is left empty).
impl Clone for World {
    fn clone(&self) -> Self {
        World {
            res: self.res.clone(),
            vars: self.vars,
            objects: self.objects.clone(),
            actors: self.actors,
            actor_view: self.actor_view,
            loaded_views: self.loaded_views,
            room_script: self.room_script.clone(),
            pic_d000: self.pic_d000,
            pic_d800: self.pic_d800,
            base: self.base.clone(),
            g: self.g.clone(),
            log: self.log.clone(),
            screen: self.screen.clone(),
            text: self.text.clone(),
            io: None,
            saved_bg: self.saved_bg.clone(),
            drawn_w: self.drawn_w,
            drawn_h: self.drawn_h,
            cost: self.cost.clone(),
            pic_draws: self.pic_draws.clone(),
            display: self.display.clone(),
            swap_prompts: self.swap_prompts,
            drive: self.drive,
        }
    }
}

impl World {
    pub fn new(res: Resources) -> Self {
        let objects = res.objects.clone();
        World {
            res,
            vars: [0; 256],
            objects,
            actors: [[0; 31]; MAX_ACTORS + 1],
            actor_view: [None; MAX_ACTORS + 1],
            loaded_views: [false; 256],
            room_script: None,
            pic_d000: None,
            pic_d800: None,
            base: vec![0x1F; WIDTH * HEIGHT],
            g: Globals { disp_slot: 0xFF, ..Globals::default() },
            log: Vec::new(),
            screen: vec![0x1F; WIDTH * HEIGHT],
            text: text::Text::default(),
            io: None,
            saved_bg: Default::default(),
            drawn_w: [0; MAX_ACTORS + 1],
            drawn_h: [0; MAX_ACTORS + 1],
            cost: cost::Cost::default(),
            pic_draws: Vec::new(),
            display: Default::default(),
            swap_prompts: false,
            drive: 1,
        }
    }

    /// Power-on: the title screens ($432F), then the game's initialisation ($45A1).
    /// Power-on, as the original starts ($4011): DOS boots and loads the engine, which
    /// clears the screen to white and shows "Please wait..." while it loads the rest of
    /// its data ($44EC); then the title screens ($432F) and initialisation ($45A1).
    ///
    /// The loading phases take fixed times (the same disks every time), measured in
    /// refemu from power-on: the screen set-up ($4A04) at 5.620 s, the title at
    /// 12.759 s, initialisation at 31.549 s and the first pass at 33.422 s. Front ends
    /// with real time show them (`IoImpl::pause`); the title's own time comes from the
    /// speed model, as a pass's does. The work counted here is spent by then, so it is
    /// not charged to the first pass.
    pub fn boot(&mut self) {
        self.calibrate_joystick(); // $44ED -> $4817
        let black = vec![0u32; text::FRAME_W * text::FRAME_H];
        self.io_pause(&black, BOOT_SCREEN_SETUP, true);
        // $4A04: the screen cleared to white; $4537: "Please wait..." at row 9, column 13.
        self.base = vec![0x1F; WIDTH * HEIGHT];
        self.screen = self.base.clone();
        self.text.cv = 9;
        self.text.ch = 13;
        let s = self.engine_string(0x6796).0;
        self.put_str(&s);
        let frame = self.render_frame();
        self.io_pause(&frame, BOOT_TITLE - BOOT_SCREEN_SETUP, true);
        self.title();
        self.init_game();
        let frame = self.render_frame();
        self.io_pause(&frame, BOOT_FIRST_PASS - BOOT_INIT, false);
        self.take_cost();
        // The first pass starts with its prompt redraw ($4029-$4036), which `cycle` does
        // at the end of the pass before; for the first pass, here. It is charged to it.
        let chars = self.cost.chars;
        self.refresh_prompt();
        self.cost.next_chars += self.cost.chars - chars;
        self.cost.chars = chars;
    }

    fn io_pause(&mut self, frame: &[u32], secs: f64, loading: bool) {
        if let Some(io) = self.io.as_mut() {
            io.pause(frame, secs, loading);
        }
    }

    /// $4018 restart: initialise again and abandon the rest of the current pass.
    pub fn restart(&mut self) {
        self.init_game();
        self.g.abort_pass = true;
    }

    /// $45A1: clear the game state ($5D4C: variables, actors, engine globals; the RNG
    /// pointer goes back to $4011), reload the object table, then run the global cycle
    /// rules once, which start room 1. The pass counter $4D, the screen and the text are
    /// kept.
    pub fn init_game(&mut self) {
        let io = self.io.take();
        let res = std::mem::take(&mut self.res);
        let text = std::mem::take(&mut self.text);
        let cost = std::mem::take(&mut self.cost);
        let cycle = self.g.cycle;
        // $5D4C clears $60-$EF; the joystick thresholds at $F4-$F7 stay.
        let joy = (self.g.joy_x, self.g.joy_y);
        let screen = std::mem::take(&mut self.screen);
        let base = std::mem::take(&mut self.base);
        let log = std::mem::take(&mut self.log);
        let display = std::mem::take(&mut self.display);
        let (swap_prompts, drive) = (self.swap_prompts, self.drive);
        *self = World::new(res);
        self.display = display;
        (self.swap_prompts, self.drive) = (swap_prompts, drive);
        self.io = io;
        self.text = text;
        self.cost = cost;
        self.screen = screen;
        self.base = base;
        self.log = log;
        self.g.cycle = cycle;
        (self.g.joy_x, self.g.joy_y) = joy;
        self.g.rng_ptr = 0x4011;
        self.run_rules(vm::Which::Global, 1);
    }

    /// $432F: the Sierra logo (picture 82) and the title scroll (picture 83) with the
    /// credits, each shown until a key is pressed or about 3.8 seconds pass ($4396:
    /// 65,280 polls of about 59 cycles). Strings and the credits table come from the
    /// engine image.
    fn title(&mut self) {
        for (pic, (load, draw, print)) in [82u8, 83].into_iter().zip(TITLE_TIMES) {
            self.vars[1] = pic;
            self.pic_d800 = Some(pic);
            let sectors = self.cost.sectors;
            self.cost_load(0, pic);
            let sectors = self.cost.sectors - sectors;
            self.bind_view(0);
            let data = self.res.pictures.get(pic as usize).cloned().flatten();
            let p = crate::pic::draw(data.as_deref().map_or(&[][..], |v| v));
            self.cost.plots += p.plots as u64;
            self.cost.fill_plots += p.fill_plots as u64;
            // The original plots it to the screen as it goes (after the disk read).
            if let (Some(data), Some(mut io)) = (data, self.io.take()) {
                let draw = cost::PicDraw {
                    before: cost::Cost { sectors, ..Default::default() },
                    onto: vec![0x1F; WIDTH * HEIGHT],
                    data,
                    plots: p.plots,
                    runs: p.runs.clone(),
                    timing: Some((load, draw)),
                };
                io.show_drawing(&draw, &|n| self.render_frame_with(&draw.partial(n), false));
                self.io = Some(io);
            }
            self.base = p.buf;
            self.screen = self.base.clone();
            // $5FA4 clears the whole screen, the text rows too: the copyright line printed
            // under the logo is gone from the scroll.
            self.text.written = [[false; text::COLS]; text::ROWS];
            self.text.hires = [[b' '; text::COLS]; text::ROWS];
            // The text is printed character by character; it shows when done.
            let frame = self.render_frame();
            self.io_pause(&frame, print, false);
            if pic == 82 {
                self.text.cv = 13;
                self.text.ch = 16;
                let s = self.engine_string(0x6669).0;
                self.put_str(&s);
                self.text.cv = 20;
                self.text.ch = 0;
                let s = self.engine_string(0x6673).0;
                self.put_str(&s);
            } else {
                let mut at = 0x66B5usize;
                loop {
                    let row = self.res.engine[at - 0x4000];
                    if row == 0 {
                        break;
                    }
                    let (s, end) = self.engine_string(at + 1);
                    self.text.cv = row;
                    self.text.ch = 10;
                    self.put_str(&s);
                    at = end + 1;
                }
            }
            let frame = self.render_frame();
            if let Some(io) = self.io.as_mut() {
                io.wait_key_timeout(&frame, TITLE_WAIT);
            }
        }
        self.text.cv = 20;
        self.text.ch = 0;
    }

    /// A NUL-terminated string in the engine image at `addr`, and the address of its NUL.
    fn engine_string(&self, addr: usize) -> (Vec<u8>, usize) {
        let o = addr - 0x4000;
        let e = self.res.engine[o..].iter().position(|&c| c == 0).map_or(self.res.engine.len(), |p| o + p);
        (self.res.engine[o..e].iter().map(|&c| c & 0x7F).collect(), e + 0x4000)
    }

    pub fn note(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
    }

    /// $49A9: the actor slot linked to object `o` (object byte 7).
    pub fn slot_of(&self, o: u8) -> usize {
        (self.objects.get(o as usize).map_or(0, |r| r[7]) as usize).min(MAX_ACTORS)
    }

    pub fn cel_dims(&self, slot: usize) -> Option<(u8, u8)> {
        let c = self.actor_cel(slot)?;
        Some((c.width, c.height))
    }

    /// The cel the engine draws for an actor. An animation can step the cel number one
    /// past the end of its loop (the castle doors, view 9, go 4 -> 5 of 5 cels for a
    /// pass). A lookup of that cel would read past the cel table (its entry is the first
    /// cel's header), but the original still shows the last cel (refemu, pass 6571 of
    /// the max-score route): the drawing pointer, actor field $16, set only by aux
    /// $0A13/$0A25, stays on the cel last selected. So the last cel of the loop is used.
    pub fn actor_cel(&self, slot: usize) -> Option<&crate::view::Cel> {
        let a = &self.actors[slot];
        let v = self.res.views.get(self.actor_view[slot]? as usize)?.as_ref()?;
        let l = v.loops.get(a[f::LOOP] as usize)?;
        let c = a[f::CEL] as usize;
        l.cels.get(c).or_else(|| if c == l.cels.len() { l.cels.last() } else { None })
    }

    /// aux $0AAE ($4A24): load $F2/$F3 with the current cel's size.
    pub fn select_cel_dims(&mut self, slot: usize) {
        if let Some((w, h)) = self.cel_dims(slot) {
            self.g.cel_w = w;
            self.g.cel_h = h;
        }
    }

    /// aux $0A25/$0A13/$0A1C: select loop/cel. `header` copies loop header bytes 0-2 into
    /// step x/y and delay. Returns the loop's cel count.
    pub fn select_loop_cel(&mut self, slot: usize, header: bool) -> u8 {
        let (l, c) = (self.actors[slot][f::LOOP], self.actors[slot][f::CEL]);
        let Some(v) = self.actor_view[slot].and_then(|v| self.res.views.get(v as usize).cloned().flatten()) else {
            self.note(format!("actor {slot}: no view loaded"));
            return 0;
        };
        self.actors[slot][f::LOOP_COUNT] = v.loops.len() as u8;
        let Some(lp) = v.loops.get(l as usize) else {
            self.note(format!("actor {slot}: loop {l} out of range"));
            return 0;
        };
        if header {
            self.actors[slot][f::STEP_X] = lp.header[0];
            self.actors[slot][f::STEP_Y] = lp.header[1];
            self.actors[slot][f::DELAY] = lp.header[2];
        }
        if c as usize >= lp.cels.len() {
            self.note(format!("actor {slot}: cel {c} out of range for loop {l}"));
        }
        lp.header[3]
    }

    /// $5BB8: set loop and cel, copying the loop header; store the cel count.
    pub fn set_loop_cel(&mut self, slot: usize, l: u8, c: u8) {
        self.actors[slot][f::LOOP] = l;
        self.actors[slot][f::CEL] = c;
        let n = self.select_loop_cel(slot, true);
        self.actors[slot][f::CEL_COUNT] = n;
    }

    /// $47BB + $473A + $47FF: load view `v` and bind it to the next actor slot.
    pub fn bind_view(&mut self, v: u8) -> Option<usize> {
        if self.g.actor_count as usize >= MAX_ACTORS {
            self.note(format!("bind view {v}: no free actor slot"));
            return None;
        }
        let slot = self.g.actor_count as usize;
        self.loaded_views[v as usize] = true;
        self.cost_load(1, v);
        self.actor_view[slot] = Some(v);
        let o = self.objects.get(v as usize).copied().unwrap_or_default();
        if let Some(r) = self.objects.get_mut(v as usize) {
            r[7] = slot as u8;
        }
        self.g.actor_count += 1;
        let a = &mut self.actors[slot];
        a[f::SLOT] = slot as u8;
        a[f::VIEW] = v;
        a[f::ROOM] = 0xFF;
        a[f::X] = o[5];
        a[f::Y] = o[6];
        if a[f::DELAY] == 0 {
            a[f::DELAY] = 1;
        }
        a[f::COUNTDOWN] = a[f::DELAY];
        for k in [f::NO_MOVE, f::FLAGS, f::DRAWN, f::CYCLE, f::MOTION] {
            a[k] = 0;
        }
        Some(slot)
    }

    /// $1374: the priority an actor would have with its bottom row at `y`.
    pub fn priority_at(&self, slot: usize, y: u8) -> u8 {
        let a = &self.actors[slot];
        if a[f::FLAGS] & 1 != 0 && a[f::PRIORITY] != 0 {
            return a[f::PRIORITY];
        }
        if y < 0x30 {
            0x10
        } else {
            (y.wrapping_sub(0x28) << 1) & 0xF0
        }
    }

    fn buf_index(x: u8, y: u8) -> Option<usize> {
        let x = x.checked_sub(10)? as usize;
        let y = y.checked_sub(3)? as usize;
        (x < WIDTH && y < HEIGHT).then(|| y * WIDTH + x)
    }

    /// aux $12C7: check the actor's bottom row at (x, y) against the priority buffer.
    /// Returns true if blocked. Updates v4 for the ego and the actor's priority field.
    fn collide(&mut self, buf: &[u8], slot: usize, x: u8, y: u8, w: u8) -> bool {
        let prio = self.priority_at(slot, y);
        let flags = self.actors[slot][f::FLAGS];
        let trace = std::env::var_os("KQ_TRACE").is_some();
        let check = |p: u8| -> bool {
            // $135E: true = blocked. Priority 15 blocks unless flag bit 1 is set; a pixel in
            // the actor's own priority band blocks (obstacle base lines are drawn that way).
            let blocked = if p == 0xF0 { flags & 2 == 0 } else { p == prio };
            if blocked && trace {
                eprintln!("collide: slot {slot} at ({x},{y}) w {w}: pixel priority {:X}, own {:X}", p >> 4, prio >> 4);
            }
            blocked
        };
        let px = |i: u8| Self::buf_index(x.wrapping_add(i), y).map_or(0, |k| buf[k] & 0xF0);
        if self.actors[slot][f::SLOT] != 0 {
            for i in (0..w).rev() {
                let p = px(i);
                if p == 0x10 {
                    continue;
                }
                if p == 0 && flags & 4 == 0 {
                    return true;
                }
                if check(p) {
                    return true;
                }
            }
        } else {
            let on_water = self.vars[4] != 0;
            let mut count = 0u8;
            for i in (0..w).rev() {
                let p = px(i);
                if (!on_water && p != 0) || (on_water && p == 0x10) {
                    count = count.wrapping_add(1);
                }
                if p >= 0x20 && check(p) {
                    return true;
                }
            }
            self.vars[4] = (count == 0) as u8;
        }
        self.actors[slot][f::PRIORITY] = prio;
        false
    }

    /// aux $11E2: step (if due), clamp to the screen and horizon, and collide.
    /// Stores the result in NEW_X/NEW_Y and returns true if blocked.
    pub fn move_actor(&mut self, slot: usize) -> bool {
        let Some((w, _h)) = self.cel_dims(slot) else { return true };
        let a = self.actors[slot];
        let (mut x, mut y) = (a[f::X], a[f::Y]);
        // $122B: movement step
        if a[f::COUNTDOWN] == 0 && a[f::NO_MOVE] == 0 {
            const DIRS: [u8; 9] = [0x00, 0x82, 0x83, 0x01, 0x03, 0x02, 0x43, 0x41, 0xC3]; // aux $16F0
            let d = DIRS.get(a[f::DIR] as usize).copied().unwrap_or(0);
            if d & 1 != 0 {
                x = if d & 0x40 != 0 { x.wrapping_sub(a[f::STEP_X]) } else { x.wrapping_add(a[f::STEP_X]) };
            }
            if d & 2 != 0 {
                y = if d & 0x80 != 0 { y.wrapping_sub(a[f::STEP_Y]) } else { y.wrapping_add(a[f::STEP_Y]) };
            }
        }
        // $1280: clamp
        let mut edge = 0u8;
        if x < 0x0B {
            edge = 1;
            x = 0x0B;
        } else if x as u16 + w as u16 >= 0x96 {
            edge = 2;
            x = 0x96 - w;
        }
        if a[f::FLAGS] & 8 == 0 && y < 0x29 {
            edge = 3;
            y = 0x29;
        } else if y >= 0xA3 {
            edge = 4;
            y = 0xA2;
        }
        let base = std::mem::take(&mut self.base);
        let blocked = self.collide(&base, slot, x, y, w);
        self.base = base;
        if blocked {
            x = a[f::X];
            y = a[f::Y];
            edge = 0;
        }
        if edge != 0 {
            if a[f::SLOT] == 0 {
                self.vars[3] = edge;
            } else {
                self.vars[22] = edge;
                self.vars[21] = a[f::VIEW];
            }
        }
        self.actors[slot][f::NEW_X] = x;
        self.actors[slot][f::NEW_Y] = y;
        blocked
    }

    /// aux $0AC5: does this actor's new position touch another actor on the same bottom
    /// row? Only contacts involving the ego count; they set v6 to the other actor's view.
    pub fn overlaps(&mut self, slot: usize) -> bool {
        let me = self.actors[slot];
        let Some((w, _)) = self.cel_dims(slot) else { return false };
        let (x, y) = (me[f::NEW_X], me[f::NEW_Y]);
        let right = x.wrapping_add(w);
        let left = x.wrapping_sub(1);
        // The engine's loop also visits index $60 (one past the last actor).
        let last = (self.g.actor_count as usize).min(MAX_ACTORS - 1);
        for other in 0..=last {
            if other == slot {
                continue;
            }
            let o = self.actors[other];
            if o[f::ROOM] != self.vars[1] || o[f::NEW_Y] != y || o[f::NEW_X] >= right {
                continue;
            }
            let ow = self.cel_dims(other).map_or(0, |d| d.0);
            if o[f::NEW_X].wrapping_add(ow) < left {
                continue;
            }
            // $0B05: decide whether this contact blocks.
            let follower = if slot == 0 {
                other
            } else if other == 0 {
                slot
            } else {
                return false;
            };
            if self.actors[follower][f::MOTION] == 0x11 {
                return false;
            }
            self.vars[6] = self.actors[follower][f::VIEW];
            return true;
        }
        false
    }

    /// $5425: try to put an actor on screen at its current position.
    /// $4E7F: is the actor on screen in this room?
    pub fn participates(&self, s: usize) -> bool {
        self.actors[s][f::ROOM] == self.vars[1] && self.actors[s][f::DRAWN] != 0
    }

    /// aux $15BF: save the buffer under the actor's new cel position.
    pub fn save_bg(&mut self, s: usize) {
        let Some((w, h)) = self.cel_dims(s) else { return };
        let (x, y) = (self.actors[s][f::NEW_X] as i32 - 10, self.actors[s][f::NEW_Y] as i32 - 3);
        let top = y - h as i32 + 1;
        if x < 0 || top < 0 || y as usize >= HEIGHT {
            self.saved_bg[s] = None;
            return;
        }
        let (x, top, w, h) = (x as usize, top as usize, (w as usize).min(WIDTH.saturating_sub(x as usize)), h as usize);
        let mut data = Vec::with_capacity(w * h);
        for r in top..top + h {
            data.extend_from_slice(&self.base[r * WIDTH + x..r * WIDTH + x + w]);
        }
        self.saved_bg[s] = Some(SavedBg { x, top, w, h, data });
    }

    /// aux $15F5: put back the buffer saved under the actor. For an ordinary sprite this
    /// changes nothing (the port keeps sprites out of `base`); for an actor that was added
    /// to the picture and then made active again, it removes the permanent drawing.
    pub fn restore_bg(&mut self, s: usize) {
        if let Some(b) = self.saved_bg[s].clone() {
            for r in 0..b.h {
                let d = (b.top + r) * WIDTH + b.x;
                self.base[d..d + b.w].copy_from_slice(&b.data[r * b.w..(r + 1) * b.w]);
            }
        }
    }

    /// $4DD7: erase every on-screen actor.
    ///
    /// aux $15F5 puts the saved block back at the actor's current position (`[$18]`,
    /// `[$19]`), not where it was saved from (`[$1A]`, `[$1B]` at the time). The two are
    /// the same unless a script moved the actor while it was on screen (the goat at the
    /// troll's bridge, room 25). Then the original writes the background a little off,
    /// and leaves the part of the sprite the block no longer covers in the picture, until
    /// the room is drawn again. Its buffer holds the sprites, as the port's `screen`
    /// does, so the restores are replayed on a copy of that, in slot order, and the
    /// result is kept where an actor had moved.
    pub fn restore_all(&mut self) {
        let mut live: Option<Vec<u8>> = None;
        let mut moved: Vec<(usize, usize, usize, usize)> = Vec::new();
        for s in 0..(self.g.actor_count as usize).min(MAX_ACTORS) {
            if !self.participates(s) {
                continue;
            }
            self.restore_bg(s);
            let Some(b) = &self.saved_bg[s] else { continue };
            let (x, y) = (self.actors[s][f::X] as i32 - 10, self.actors[s][f::Y] as i32 - 3);
            let top = y - b.h as i32 + 1;
            if (x, top) == (b.x as i32, b.top as i32) && live.is_none() {
                continue;
            }
            let buf = live.get_or_insert_with(|| {
                let mut l = if self.screen.len() == self.base.len() { self.screen.clone() } else { self.base.clone() };
                // The restores so far, which the original made on this buffer too.
                for t in 0..s {
                    if self.participates(t) {
                        if let Some(o) = &self.saved_bg[t] {
                            let (tx, ty) = (self.actors[t][f::X] as i32 - 10, self.actors[t][f::Y] as i32 - 3);
                            put_block(&mut l, o, tx, ty - o.h as i32 + 1);
                        }
                    }
                }
                l
            });
            put_block(buf, b, x, top);
            if (x, top) != (b.x as i32, b.top as i32) {
                moved.push((b.x, b.top, b.w, b.h));
                if x >= 0 && top >= 0 {
                    moved.push((x as usize, top as usize, b.w, b.h));
                }
            }
        }
        if let Some(l) = live {
            for (x, top, w, h) in moved {
                for r in top..(top + h).min(HEIGHT) {
                    let (a, z) = (r * WIDTH + x.min(WIDTH), r * WIDTH + (x + w).min(WIDTH));
                    self.base[a..z].copy_from_slice(&l[a..z]);
                }
            }
        }
    }

    /// $4E2C: save the background under every on-screen actor.
    pub fn save_all(&mut self) {
        for s in 0..(self.g.actor_count as usize).min(MAX_ACTORS) {
            if self.participates(s) {
                self.save_bg(s);
            }
        }
    }

    /// $4DED: move every on-screen actor in draw order, cancelling moves that touch the ego.
    pub fn move_all(&mut self) {
        self.vars[3] = 0;
        self.vars[21] = 0;
        self.vars[22] = 0;
        for s in self.draw_order() {
            if !self.participates(s) {
                continue;
            }
            self.move_actor(s);
            if self.overlaps(s) {
                self.actors[s][f::NEW_X] = self.actors[s][f::X];
                self.actors[s][f::NEW_Y] = self.actors[s][f::Y];
            }
        }
    }

    /// $4E69: the new positions become the current ones.
    pub fn commit_all(&mut self) {
        for s in 0..(self.g.actor_count as usize).min(MAX_ACTORS) {
            if self.participates(s) {
                self.update_rect(s);
                self.commit(s);
            }
        }
    }

    /// aux $13C4 (through $0EBA): the screen rectangle redrawn for an actor covers its
    /// old and new cels. Its width is left in $F2, where later code (the follow
    /// motion's patrol target, $5CD7) reads it: max(old, new width) + |dx|, clipped
    /// at x = $96.
    fn update_rect(&mut self, s: usize) {
        let a = self.actors[s];
        let (new_w, new_h) = self.cel_dims(s).unwrap_or((0, 0));
        let old_w = if self.drawn_w[s] == 0 { new_w } else { self.drawn_w[s] };
        let (x0, x1) = (a[f::X].min(a[f::NEW_X]), a[f::X].max(a[f::NEW_X]));
        let mut w = old_w.max(new_w).wrapping_add(x1 - x0);
        if 0x96u8.wrapping_sub(x0) < w {
            w = 0x96u8.wrapping_sub(x0);
        }
        self.g.cel_w = w;
        self.drawn_w[s] = new_w;
        self.drawn_h[s] = new_h;
    }

    pub fn try_draw(&mut self, slot: usize) -> bool {
        if self.actors[slot][f::DRAWN] != 0 {
            self.actors[slot][f::DRAWN] = 0;
            return false;
        }
        self.actors[slot][f::DRAWN] = 1;
        if self.actors[slot][f::SLOT] == 0 {
            self.vars[3] = 0;
            self.vars[6] = 0;
        }
        if self.move_actor(slot) || self.overlaps(slot) {
            self.actors[slot][f::DRAWN] = 0;
            return false;
        }
        // $4DB9: erase the others, then put this actor in the room, save backgrounds,
        // and redraw every sprite ($4E42, which also sets v7 from the ego).
        self.actors[slot][f::ROOM] = 0xFF;
        self.restore_all();
        self.actors[slot][f::ROOM] = self.vars[1];
        self.save_all();
        self.cost_redraw_all();
        self.screen = self.compose_at_new();
        self.commit_all();
        true
    }

    /// Final pass (aux $13C2): the new position becomes the current one.
    fn commit(&mut self, slot: usize) {
        self.actors[slot][f::X] = self.actors[slot][f::NEW_X];
        self.actors[slot][f::Y] = self.actors[slot][f::NEW_Y];
    }

    /// $64BC: draw object `o`, nudging it in a spiral until it fits ($655A).
    pub fn draw_object(&mut self, o: u8) {
        let slot = self.slot_of(o);
        self.select_cel_dims(slot);
        if self.actors[slot][f::DRAWN] != 0 {
            return;
        }
        self.actors[slot][f::DIR] = 0;
        self.g.nudge_limit = 0x95u8.wrapping_sub(self.g.cel_w);
        if self.actors[slot][f::X].wrapping_add(self.g.cel_w) >= 0x95 {
            self.actors[slot][f::X] = self.g.nudge_limit;
        }
        self.g.nudge = [1, 1, 1];
        let mut tries = 0;
        while !self.try_draw(slot) {
            self.nudge(slot);
            tries += 1;
            if tries > 20000 {
                self.note(format!("draw {o}: no free position found"));
                break;
            }
        }
        let carried = self.objects[o as usize][2] & 0x80;
        self.objects[o as usize][2] = carried | self.vars[0];
    }

    /// $655A: one step of the spiral search, skipping positions out of bounds.
    pub(crate) fn nudge(&mut self, slot: usize) {
        loop {
            let [d, run, len] = self.g.nudge;
            let a = &mut self.actors[slot];
            match d {
                4 => a[f::Y] = a[f::Y].wrapping_sub(1),
                2 => a[f::Y] = a[f::Y].wrapping_add(1),
                _ => {
                    if d == 3 {
                        a[f::X] = a[f::X].wrapping_add(1);
                    } else {
                        a[f::X] = a[f::X].wrapping_sub(1);
                    }
                    if run == 1 {
                        self.g.nudge[2] = len.wrapping_add(1);
                    }
                }
            }
            self.g.nudge[1] = self.g.nudge[1].wrapping_sub(1);
            if self.g.nudge[1] == 0 {
                self.g.nudge[1] = self.g.nudge[2];
                self.g.nudge[0] = self.g.nudge[0].wrapping_sub(1);
                if self.g.nudge[0] == 0 {
                    self.g.nudge[0] = 4;
                }
            }
            let a = &self.actors[slot];
            let (x, y) = (a[f::X], a[f::Y]);
            if x >= 0x0B && x < self.g.nudge_limit && (0x04..0xA2).contains(&y) {
                break;
            }
        }
        self.vars[3] = 0;
    }

    /// $6476 + aux $10B5: draw the actor permanently into the picture with its priority,
    /// then retire it as a sprite.
    pub fn add_to_picture(&mut self, slot: usize) {
        // $6476: erase and move every on-screen actor, save this actor's background
        // (aux $15BF), then draw it permanently (aux $10B5).
        self.restore_all();
        let was = self.actors[slot][f::ROOM];
        self.actors[slot][f::ROOM] = self.vars[1];
        let drawn = self.actors[slot][f::DRAWN];
        self.actors[slot][f::DRAWN] = 1;
        self.move_all();
        self.actors[slot][f::ROOM] = was;
        self.actors[slot][f::DRAWN] = drawn;
        if !self.participates(slot) {
            self.move_actor(slot);
        }
        self.save_bg(slot);
        let a = self.actors[slot];
        let prio = a[f::PRIORITY];
        self.cost_cel(slot);
        self.cost_screen(slot);
        let mut buf = std::mem::take(&mut self.base);
        self.draw_cel(&mut buf, slot, a[f::NEW_X], a[f::NEW_Y], prio, true);
        // $11BB/$11C8: priority-15 box: bottom row, the row at the 8-line boundary, and
        // the two sides between them.
        if let Some((w, _)) = self.cel_dims(slot) {
            let (x, y) = (a[f::NEW_X], a[f::NEW_Y]);
            let mut mark = |xx: u8, yy: u8| {
                if let Some(k) = Self::buf_index(xx, yy) {
                    buf[k] |= 0xF0;
                }
            };
            for i in 0..w {
                mark(x.wrapping_add(i), y);
            }
            let top = y & 0xF8;
            if top != y {
                for i in 0..w {
                    mark(x.wrapping_add(i), top);
                }
                for yy in top.wrapping_add(1)..=y {
                    mark(x, yy);
                    mark(x.wrapping_add(w).wrapping_sub(1), yy);
                }
            }
        }
        self.base = buf;
        self.commit(slot);
        self.actors[slot][f::DRAWN] = 1;
        self.actors[slot][f::ROOM] = 0xFF;
        // $64A2-$64A8: save the others' backgrounds and redraw them over the new picture.
        self.save_all();
        self.screen = self.compose_at_new();
        self.commit_all();
    }

    /// aux $10EF/$1124: draw an actor's current cel with its bottom row at (x, y).
    /// `permanent` = $10B5 (write the actor's priority); otherwise the buffer priority is
    /// kept, except where a priority-15 pixel takes the priority found below it.
    /// Returns true if any pixel was drawn.
    pub fn draw_cel(&self, buf: &mut [u8], slot: usize, x: u8, y: u8, prio: u8, permanent: bool) -> bool {
        let Some(c) = self.actor_cel(slot) else {
            return false;
        };
        let top = y.wrapping_sub(c.height).wrapping_add(1);
        let mut any = false;
        for r in 0..c.height {
            for col in 0..c.width {
                let Some(colour) = c.pixels[r as usize * c.width as usize + col as usize] else { continue };
                let Some(k) = Self::buf_index(x.wrapping_add(col), top.wrapping_add(r)) else { continue };
                let p = buf[k] & 0xF0;
                let write_prio = if p <= prio {
                    Some(if permanent { prio } else { p })
                } else if p == 0xF0 {
                    if permanent {
                        Some(0xF0)
                    } else {
                        // $116C: use the first priority below that isn't 15.
                        let mut q = 0u8;
                        let mut kk = k + WIDTH;
                        while kk < buf.len() {
                            if buf[kk] & 0xF0 != 0xF0 {
                                q = buf[kk] & 0xF0;
                                break;
                            }
                            kk += WIDTH;
                        }
                        (q <= prio).then_some(q)
                    }
                } else {
                    None
                };
                if let Some(wp) = write_prio {
                    buf[k] = wp | colour;
                    any = true;
                }
            }
        }
        any
    }

    /// $4EA1: draw-order key for an actor.
    pub(crate) fn sort_key(&self, slot: usize) -> u8 {
        let a = &self.actors[slot];
        if a[f::FLAGS] & 1 != 0 {
            let fixed = a[f::PRIORITY];
            if a[f::Y].wrapping_sub(0x28) << 1 != fixed {
                let k = (fixed >> 1).wrapping_add(0x28);
                return if k < a[f::Y] { k } else { k.wrapping_add(7) };
            }
        }
        a[f::Y]
    }

    /// The screen as the engine would show it: the picture buffer with every visible
    /// actor drawn in sort order ($4E42). Also updates v7 from the ego ($4E5B).
    pub fn compose(&mut self) -> Vec<u8> {
        self.compose_with(false)
    }

    /// As `compose`, but drawing each actor at its new (not yet committed) position, as
    /// the draw pass does before the commit step.
    pub fn compose_at_new(&mut self) -> Vec<u8> {
        self.compose_with(true)
    }

    fn compose_with(&mut self, at_new: bool) -> Vec<u8> {
        let mut buf = self.base.clone();
        let n = (self.g.actor_count as usize).min(MAX_ACTORS);
        let order: Vec<usize> = self
            .draw_order()
            .into_iter()
            .filter(|&s| self.actors[s][f::ROOM] == self.vars[1] && self.actors[s][f::DRAWN] != 0)
            .collect();
        for s in order {
            let a = self.actors[s];
            let (x, y) = if at_new { (a[f::NEW_X], a[f::NEW_Y]) } else { (a[f::X], a[f::Y]) };
            let drawn = self.draw_cel(&mut buf, s, x, y, a[f::PRIORITY], false);
            self.actors[s][f::INVISIBLE] = (!drawn) as u8;
        }
        if n > 0 {
            self.vars[7] = self.actors[0][f::INVISIBLE];
        }
        buf
    }

    /// $48DF: save actors back into their objects, then free all actor slots.
    pub fn clear_actors(&mut self) {
        self.persist_actors();
        let n = self.g.actor_count as usize;
        for a in self.actors.iter_mut().take(n) {
            a[f::DRAWN] = 0;
        }
        self.g.actor_count = 0;
    }

    /// $491F: write each actor in the current room back into its object record (room,
    /// position, loop, cel). The ego (object 0) first, then actor slots 1.. by view number.
    pub fn persist_actors(&mut self) {
        let n = self.g.actor_count as usize;
        for k in 0..n.max(1) {
            let o = if k == 0 { 0 } else { self.actors[k][f::VIEW] };
            let slot = if k == 0 { self.slot_of(0) } else { k };
            let a = self.actors[slot];
            if a[f::ROOM] != self.vars[1] {
                continue;
            }
            if let Some(r) = self.objects.get_mut(o as usize) {
                r[2] = (r[2] & 0x80) | self.vars[1];
                r[5] = a[f::X];
                r[6] = a[f::Y];
                r[3] = a[f::LOOP];
                r[4] = a[f::CEL];
            }
        }
    }

    /// $4BA3 + $4B1E: leave the current room and load room `r`'s script.
    pub fn new_room(&mut self, r: u8) {
        self.new_room_carry(r, false);
    }

    /// new_room, reached with the 6502 carry `carry`. When the ego left by no edge
    /// (v3 = 0), $4BA3 goes straight to the random call at $4BBD and the caller's carry
    /// reaches it: clear from the scripts, set from restore ($4F8B: the loop at $48DF
    /// ends with carry set). After an edge exit the edge-table arithmetic clears it.
    pub(crate) fn new_room_carry(&mut self, r: u8, carry: bool) {
        self.note(format!(
            "new_room {} -> {r} (v3={}, ego at {},{})",
            self.vars[1],
            self.vars[3],
            self.actors[0][f::X],
            self.actors[0][f::Y]
        ));
        let edge = self.vars[3];
        if (1..=4).contains(&edge) {
            const ENTRY: [u8; 4] = [148, 11, 161, 41]; // $6913
            let byte = 5 + ((edge - 1) >> 1) as usize;
            self.objects[0][byte] = ENTRY[(edge - 1) as usize];
        }
        let before = (self.g.rng_ptr, self.g.cycle);
        self.vars[14] = self.random_c(carry && edge == 0).0 & 1;
        if std::env::var_os("KQ_RNG").is_some() {
            eprintln!(
                "rng: room {} ptr {:04X} counter {:02X} -> v14 {}",
                self.vars[1], before.0, before.1, self.vars[14]
            );
        }
        self.vars[18] = self.vars[1];
        for v in [3, 6, 21, 22] {
            self.vars[v] = 0;
        }
        let ego = self.slot_of(0);
        self.g.saved_motion = self.actors[ego][f::MOTION];
        self.vars[1] = r;
        self.room_script = self.res.scripts.get(r as usize).cloned().flatten();
        self.cost_load(2, r);
        self.cost.room_changes += 1;
        if self.room_script.is_none() {
            self.note(format!("room {r}: no script"));
        }
        self.g.ego_auto = 0;
        self.g.ego_auto2 = 0;
        self.g.barrier_x = 0;
        self.g.barrier_y = 0;
    }

    /// $65F1. The engine reads its own code bytes as a random table, XORed with the cycle
    /// counter. Carry-dependent pointer stepping is not modelled yet (docs/motion.md).
    /// $65F1: `INC $4B; BCC; INC $4C`. INC does not touch the carry, so the pointer's
    /// high byte advances only when the caller arrives with carry set; otherwise the low
    /// byte wraps within its page. Past $64FF the pointer restarts at $4011. The byte read
    /// from the engine image is XORed with the pass counter $4D.
    /// Returns the value and the carry on exit (from `CMP #$65`).
    pub fn random_c(&mut self, carry_in: bool) -> (u8, bool) {
        if self.g.rng_ptr == 0 {
            self.g.rng_ptr = 0x4011; // $5D85
        }
        let mut lo = (self.g.rng_ptr & 0xFF) as u8;
        let mut hi = (self.g.rng_ptr >> 8) as u8;
        lo = lo.wrapping_add(1);
        if carry_in {
            hi = hi.wrapping_add(1);
        }
        let carry_out = hi >= 0x65;
        if carry_out {
            lo = 0x11;
            hi = 0x40;
        }
        self.g.rng_ptr = (hi as u16) << 8 | lo as u16;
        let b = match (self.g.rng_ptr, self.g.motion_jsr, self.g.action_jsr) {
            (0x5561, m, _) if m != 0 => m as u8,
            (0x5562, m, _) if m != 0 => (m >> 8) as u8,
            (0x58D7, _, a) if a != 0 => a as u8,
            (0x58D8, _, a) if a != 0 => (a >> 8) as u8,
            (0x523F, _, _) if self.g.word_cmp != 0 => self.g.word_cmp,
            (p, _, _) => self.res.engine.get((p - 0x4000) as usize).copied().unwrap_or(0),
        };
        if std::env::var_os("KQ_RNG_ALL").is_some() {
            eprintln!("rng: ptr {:04X} counter {:02X} byte {b:02X}", self.g.rng_ptr, self.g.cycle);
        }
        (b ^ self.g.cycle, carry_out)
    }

    /// A handler address from one of the engine's dispatch tables.
    pub(crate) fn engine_word(&self, addr: u16) -> u16 {
        let o = (addr - 0x4000) as usize;
        let e = &self.res.engine;
        match (e.get(o), e.get(o + 1)) {
            (Some(&lo), Some(&hi)) => lo as u16 | (hi as u16) << 8,
            _ => 0,
        }
    }

    /// RGB image of a composed screen buffer, 2x1 pixel aspect.
    pub fn to_rgb(buf: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(WIDTH * 2 * HEIGHT * 3);
        for &b in buf {
            let c = pic::dhgr_rgb(b & 0x0F);
            v.extend_from_slice(&c);
            v.extend_from_slice(&c);
        }
        v
    }
}
