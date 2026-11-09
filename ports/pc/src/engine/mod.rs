//! The 1984 IBM PC King's Quest engine (the CGA release), reimplemented natively: the
//! PCjr port's engine (ports/pcjr/src/engine), the same design, with the PC's
//! differences found against the original (docs/engine.md, "How the PC differs").
//!
//! The original is a C program with some hand-written assembly, in segment 0060h. This
//! follows its behaviour routine by routine, as the Apple port follows its 6502 engine
//! (ports/apple2/src/engine). Comments name the routine each function reimplements
//! (`/// $293A: ...`), and docs/engine.md maps the original's code.
//!
//! The state is native: decoded views, scripts and sounds; the variables, objects and
//! actors as records with named fields; the engine's other globals in [`Globals`]; the
//! two video pages as 160 × 168 planes. Where the original's behaviour depends on its
//! memory (a buffer it never refills, a pointer it never resets), that is modelled
//! explicitly and named.
//!
//! The timer interrupt prints the messages, plays the music and reads the keys
//! (timer.rs). It fires at checkpoints (clock.rs): when replaying refemu-pcjr's event
//! log, at the same points as in the original.

pub mod actor;
pub mod clock;
pub mod cost;
pub mod input;
pub mod motion;
pub mod res;
pub mod save;
pub mod screen;
pub mod screens;
pub mod script;
pub mod sprite;
pub mod text;
pub mod timer;

use crate::disk::Disk;
use clock::{Clock, Event};
use res::{Resources, Script, View};
use std::collections::VecDeque;
use std::rc::Rc;

/// Actor records: the ego and up to 4 more. (The original's table has room for 4; a
/// fifth would overlap its globals, which the game never does.)
pub const ACTORS: usize = 5;
/// Objects in the object table.
pub const OBJECTS: usize = 111;

/// A cel of a view: its loop and its number, and where it is in the original's memory
/// (the actor records hold that pointer, and a save keeps it).
#[derive(Clone, Default)]
pub struct CelRef {
    pub view: Option<Rc<View>>,
    pub lp: u8,
    pub cel: u8,
    pub ptr: u16,
}

impl CelRef {
    /// The cel's height and width (0, 0 with no view).
    pub fn size(&self) -> (u8, u8) {
        match self.get() {
            Some(c) => (c.height, c.width),
            None => (0, 0),
        }
    }
    pub fn get(&self) -> Option<&crate::view::Cel> {
        let v = self.view.as_ref()?;
        let l = v.view.loops.get(self.lp as usize)?;
        l.cels.get(self.cel as usize).or_else(|| l.cels.last())
    }
}

/// An actor (DS:17E7 + 30 × slot in the original).
#[derive(Clone, Default)]
pub struct Actor {
    /// v1 while on screen, FF off screen; 0 ends the table.
    pub room: u8,
    /// The view (+01), and where it is in the original's memory (the pointer the
    /// record holds).
    pub view: Option<Rc<View>>,
    pub view_addr: u16,
    /// Restored from a save: the view pointer, until a view is loaded there (the
    /// original's restore relies on the room's script loading its views where they
    /// were); meanwhile the view named by the record's view number stands in.
    pub pending_view: Option<u16>,
    pub lp: u8,
    /// 1: ignores the barriers; 2: stopped by one this cycle.
    pub no_move: u8,
    /// The view number (and, for actors added by op 02, the object number).
    pub view_num: u8,
    pub cel_count: u8,
    pub cel: u8,
    pub delay: u8,
    pub countdown: u8,
    /// Bit 0 fixed priority, bit 1 ignores priority 15, bit 2 ignores priority 0.
    pub flags: u8,
    pub step_x: u8,
    pub step_y: u8,
    /// How it animates: 0 cycle, 1 still, 2 back to cel 0, 3 to the last cel, 4 to the
    /// last cel from the next update, 5-7 the special view of op 2D.
    pub cycle: u8,
    /// The motion mode (0C-11h), or a direction to walk in.
    pub motion: u8,
    pub dir: u8,
    /// Its slot, which picks where its background is saved.
    pub slot: u8,
    /// The background saved under it, and where (+11: its slot's offset, DS:1320,
    /// given when the actor is added; 0 in a record never used).
    pub saved: Vec<u8>,
    pub save_offset: u16,
    /// The cel last drawn, and the cel to draw.
    pub cel_drawn: CelRef,
    pub cel_new: CelRef,
    pub x: u8,
    pub y: u8,
    pub new_x: u8,
    pub new_y: u8,
    /// The last draw showed no pixel.
    pub hidden: u8,
    pub priority: u8,
}

impl Actor {
    /// The loop's header: step x, step y, delay, cel count.
    pub fn header(&self, lp: u8) -> [u8; 4] {
        self.view.as_ref().and_then(|v| v.view.loops.get(lp as usize)).map_or([0; 4], |l| l.header)
    }
    pub fn loops(&self) -> u8 {
        self.view.as_ref().map_or(0, |v| v.view.loops.len() as u8)
    }
}

/// An object (DS:1967 + 8 × n).
#[derive(Clone, Copy, Default, Debug)]
pub struct Object {
    /// Where its name is in the engine's data.
    pub name: u16,
    /// The room it is in; bit 7 set while carried.
    pub room: u8,
    /// Byte 3, unused.
    pub extra: u8,
    /// Loop << 4 | cel, kept when its actor leaves the room.
    pub loop_cel: u8,
    pub x: u8,
    pub y: u8,
    /// The actor slot showing it.
    pub slot: u8,
}

/// A message being printed, or queued: its text and how far printing has got.
#[derive(Clone)]
pub struct Msg {
    pub text: Rc<Vec<u8>>,
    pub at: usize,
}

/// A sound stream: its data and the position of the next record.
#[derive(Clone)]
pub struct Stream {
    pub data: Rc<Vec<u8>>,
    pub at: usize,
}

impl PartialEq for Stream {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.data, &o.data) && self.at == o.at
    }
}

/// The engine's globals besides the variables, objects and actors. Each names the
/// original's location.
#[derive(Clone)]
pub struct Globals {
    // -- The timer and the keyboard.
    /// 0CDC: the joystick was polled last tick.
    pub joy_skip: bool,
    /// 0CDD: ticks to the next word printed.
    pub print_countdown: u8,
    /// 0E48: the key ring (80 characters).
    pub ring: VecDeque<u8>,
    /// 0DD2/0EA8: where the messages print.
    pub text_row: u8,
    pub text_col: u8,
    /// 0E24: ticks to the next music record.
    pub music_countdown: u8,
    /// 0E26: sound on (key '1').
    pub sound_on: bool,
    /// 0E27: the timer may read the keyboard.
    pub keys_on: bool,
    /// 0E28: the prompt is current.
    pub prompt_shown: bool,
    /// 0E29: a parser rule matched the line.
    pub matched: bool,
    /// 0E2D: the prompt is to be drawn again.
    pub prompt_due: bool,
    /// 0E2E-0E33: keys '3' (save game), '5' (restore), '7' (restart), '9' (retype),
    /// Tab (status), '.'.
    pub key_flags: [bool; 6],
    /// 0E34: Ctrl-V, the debug mode.
    pub debug: bool,
    /// 0E35: the message queue (8).
    pub queue: VecDeque<Msg>,
    /// 0E47: print whole messages at once.
    pub print_all: bool,
    /// 0E99: op 32 asked to save.
    pub save_requested: bool,
    /// 0E9B, 0E9C, 0E9E/0EA0: the music: playing, the tune to go on to, the record
    /// being played and the tune's start.
    pub music_on: bool,
    pub music_next: Option<Stream>,
    pub music: Option<Stream>,
    pub music_start: Option<Stream>,
    /// 0EA5: paused (Esc).
    pub paused: bool,
    /// 0D24: the sound table, 40 streams (none loaded: silence).
    pub sounds: Vec<Option<Stream>>,
    // -- The parser.
    pub parse: input::Parse,
    // -- Actors and motion.
    /// 0EA6/0EAB: the actor op 02's helpers last selected.
    pub selected: usize,
    /// 125A: input off (game over).
    pub input_off: bool,
    /// 125B: the actor count (the highest slot in use).
    pub actor_count: u8,
    /// 125C/125D: the barriers (0: none), and 125E-1261 their bands.
    pub barrier_x: u8,
    pub barrier_y: u8,
    pub band_x: (u8, u8),
    pub band_y: (u8, u8),
    /// 1262/1263: the ego under the program's control.
    pub program: bool,
    pub program2: bool,
    /// 1264/1265: the motion target.
    pub target: (u8, u8),
    /// 1266/1267: the chase's slack.
    pub slack: (u8, u8),
    /// 1268/1269: the follow distance and state.
    pub follow_dist: u8,
    pub follow_state: u8,
    /// 126A, 1270: an x step kept by ops 2D and 28.
    pub kept_step: u8,
    pub kept_step28: u8,
    /// 126B, 126C, 1271, 1272: the chase's detour (shared by every actor).
    pub detour_count: u8,
    pub detour_turn: u8,
    pub detour_len: u8,
    pub detour_state: u8,
    /// 126D: the follower's patrol side.
    pub patrol_side: u8,
    /// 126E: op 2D's step.
    pub special_step: u8,
    /// 126F: a room change happened.
    pub room_changed: u8,
    /// 1273: the chase follows a barrier.
    pub barrier_latch: u8,
    /// 1275: the ego's direction before the update.
    pub saved_dir: u8,
    /// 1276: the chase's last direction.
    pub chase_dir: u8,
    /// 1277, 1278, 1279: the joystick timed out; its direction; the keypad's.
    pub joy_timeout: u8,
    pub joy_dir: u8,
    /// The joystick's centre, as the first good read took it (DS:115E set, its counts),
    /// for the thresholds.
    pub joy_centre: Option<(u8, u8)>,
    pub key_dir: u8,
    /// 127C: a set_loop waiting for the actor's next update (its view number; FF
    /// none).
    pub pending_loop: u8,
    /// 127E: op 42 asked for a Play diskette.
    pub play_disk: bool,
    /// 180D/180E: the last position any motion call left.
    pub last_pos: (u8, u8),
    /// 1810: the actor the sprite routines work on.
    pub current: usize,
    /// 1814: the wanderer's edge toggle.
    pub wander_toggle: u8,
    /// 1815: the random byte's place in the engine's code; 1817 the wander timer.
    pub random: u16,
    /// The original's memory for resources, from DS:20D8 up: its top (DS:1DD8), where
    /// the next resource goes; the last room script's sections (DS:1DD0-1F5E); and the
    /// room's, kept by new_room (DS:15CA-0FCA), to which clearing the actors goes back.
    /// The port reads its resources from its own copy of the disk; it keeps these only
    /// for the pointers the actor records hold, which a save keeps.
    pub heap: u16,
    pub room_ptrs: [u16; 4],
    pub room_marks: [u16; 4],
    pub wander_timer: u16,
    /// 1820-1826: the last three (key, slot) pairs of the draw order, which the sort
    /// includes but never refills.
    pub order_spare: [(u8, u8); 3],
    /// 1E91: restoring after a Play diskette was made (nothing drawn meanwhile).
    pub restoring: bool,
    /// 1F15/1F16/1F18: set_ego_view's fit check, the ego's kept loop and cel, the
    /// water placement's target.
    pub fit_check: bool,
    pub kept_loop_cel: (u8, u8),
    pub water_target: u8,
    // -- Picture drawing.
    pub pic: crate::pic::State,
    /// The picture loaded (linear F000h).
    pub picture: Option<Rc<Vec<u8>>>,
    /// The first start (0DD3).
    pub first_start: bool,
    // -- The text screen and the BIOS.
    /// 1EF9: the text screen's attribute; 1EF7/1EF8: its cursor (row, column).
    pub text_attr: u8,
    pub text_cursor: (u8, u8),
    /// 1EF4: the graphics cursor, kept while the text screen is up.
    pub gfx_cursor: (u8, u8),
    pub hpos_gfx: u8,
    pub hpos_text: u8,
    pub hpos: u8,
    /// 1259, 1274, 127A: bytes among the saved globals that nothing uses.
    pub spare: [u8; 3],
}

/// A diskette: 360K, its sectors in order.
#[derive(Clone)]
pub struct Floppy {
    pub image: Vec<u8>,
    pub write_protected: bool,
    /// Writes made to it (a front end saves the image when this changes).
    pub writes: u64,
}

/// Asked to stop: the run is over (a pass limit, the end of a replay), or diverged.
#[derive(Debug)]
pub struct Stop(pub String);

/// One pass of the game cycle, as refemu-pcjr's trace records it: at the entry of
/// `$293A`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassRow {
    pub pass: u64,
    pub room: u8,
    pub ego_x: u8,
    pub ego_y: u8,
    pub ego_dir: u8,
    pub score: u8,
    pub vars_hash: u64,
    /// The speed model's cycles since power-on, at the pass's start.
    pub cycles: u64,
}

pub struct World {
    pub res: Resources,
    pub vars: [u8; 256],
    pub objects: [Object; OBJECTS],
    pub actors: [Actor; ACTORS],
    /// The view table (DS:1709): views loaded by op 24, by number.
    pub view_table: Vec<Option<Rc<View>>>,
    /// Where each of them is in the original's memory.
    pub view_table_addr: Vec<u16>,
    /// The scripts: the global one (room 0), the room's, and op 40's extra one.
    pub global: Option<Rc<Script>>,
    pub room: Option<Rc<Script>>,
    pub extra: Option<Rc<Script>>,
    pub g: Globals,
    pub screen: screen::Screen,
    pub text: text::Text,
    /// The BIOS keyboard buffer.
    pub keys: VecDeque<u16>,
    /// The Master and the Play diskette, and which is in the drive.
    pub disks: [Floppy; 2],
    pub drive: usize,
    /// What the engine does to the speaker, in order, for the sound output.
    pub speaker: Vec<Speaker>,
    /// When the timer fires, and the events of the interval since the last
    /// checkpoint not yet fired.
    pub clock: Box<dyn Clock>,
    pub pending: VecDeque<Event>,
    pub checkpoint: u64,
    /// When set, the address of every checkpoint passed (for finding where the port
    /// and the original part).
    pub checkpoint_trace: Option<Vec<u16>>,
    /// Estimated CPU cycles since power-on (the speed model).
    pub cycles: u64,
    /// The game's timer handler is installed (`$19D8`).
    pub timer_on: bool,
    pub passes: u64,
    pub trace: Vec<PassRow>,
    /// The variables at the start of each pass, when `keep_vars` is set.
    pub keep_vars: bool,
    pub vars_trace: Vec<[u8; 256]>,
    pub max_passes: u64,
    pub log: Vec<String>,
    /// Print each action run (KQ_TRACE_OPS).
    pub trace_ops: bool,
    /// Print the ego and the keys at the start of passes FROM-TO (KQ_PROBE=FROM-TO).
    pub probe: Option<(u64, u64)>,
    /// Print the checkpoints numbered FROM-TO as they pass (KQ_CPS=FROM-TO, as
    /// refemu-pcjr's).
    pub cps: Option<(u64, u64)>,
    /// Each pass's screen as the player sees it ([`World::frame`]), hashed, at the
    /// start of the pass (as the trace rows), when Some.
    pub frame_hashes: Option<Vec<u64>>,
    /// The work of the pass under way (the speed model's counts), and, when Some,
    /// every finished pass's.
    pub cost: cost::Cost,
    pub cost_trace: Option<Vec<cost::Cost>>,
    /// Each room picture's drawing, as shown (with a front end watching): at each
    /// mark, the bytes written to the displayed page so far and the work done (for
    /// timing it against the original's, examples/drawcmp.rs).
    pub drawing_trace: Option<Vec<Vec<(usize, crate::pic::Work)>>>,
    drawing_room: bool,
    /// A front end's fixed rate (`--rate`): every pass lasts this many cycles of the
    /// game's time, whatever its work (busy waits and delays still take theirs), and
    /// the pass's start. None: the speed model's time.
    pub fixed_pass_cycles: Option<u64>,
    pass_start: u64,
    /// A front end's starting room (`--room`): the title is skipped and the game goes
    /// there from room 1, before room 1's script runs.
    pub start_room: Option<u8>,
    /// A front end's look at the game, about 60 times a second of the game's time (at
    /// checkpoints and in busy waits): to show the screen, play the sound bytes, put
    /// the disk asked for in the drive, keep the Play diskette.
    pub on_frame: Option<Box<dyn FnMut(&mut World)>>,
    next_frame: u64,
    /// The cycles of the pass's work already added to `cycles`, and the fraction of a
    /// cycle left over.
    charged: f64,
    carry: f64,
    /// Of `cycles`, those of loading (cost::Cost::loading_cycles), as the clock was
    /// last told; those of the passes before this one.
    pub loading_cycles: u64,
    loading_before: f64,
    /// The BIOS's font, for drawing the text into the frame as the BIOS does (the
    /// player's own, from its BIOS image); without it the text's cells are blank, as in
    /// refemu-pc without one.
    pub font: Option<Rc<crate::font::Font>>,
    /// The whole frame at the start of pass N (`frame_at`), once reached.
    pub frame_at: Option<u64>,
    pub frame: Option<Vec<u8>>,
}

/// The PC's speaker, as the engine drives it: the timer's channel 2 set to a divisor (in
/// square-wave mode; the tone is 1,193,182 Hz over it), or the speaker's gate (port
/// 61h, its two low bits) switched on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Divisor(u16),
    Gate(bool),
}

impl Speaker {
    /// The event as refemu-pc's speaker log writes it (reference/*.speaker).
    pub fn log_line(&self) -> String {
        match self {
            Speaker::Divisor(d) => format!("D {d:04X}"),
            Speaker::Gate(on) => format!("G {}", *on as u8),
        }
    }
}

/// A frame's hash (FNV-1a), as refemu's tools hash the original's.
pub fn frame_hash(f: &[u8]) -> u64 {
    f.iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211))
}

impl World {
    pub fn new(disk: &Disk, clock: Box<dyn Clock>) -> Result<World, String> {
        let res = Resources::new(disk)?;
        let objects = res.initial_objects();
        let g = Globals {
            joy_skip: false,
            print_countdown: res.ds[0x12E1],
            ring: VecDeque::new(),
            text_row: res.ds[0x13D6],
            text_col: 0,
            music_countdown: 1,
            sound_on: true,
            keys_on: true,
            prompt_shown: true,
            matched: true,
            prompt_due: true,
            key_flags: [false; 6],
            debug: false,
            queue: VecDeque::new(),
            print_all: false,
            save_requested: false,
            music_on: false,
            music_next: None,
            music: None,
            music_start: None,
            paused: false,
            sounds: vec![None; 40],
            parse: input::Parse::default(),
            selected: 0,
            input_off: false,
            actor_count: 0,
            barrier_x: 0,
            barrier_y: 0,
            band_x: (0, 0),
            band_y: (0, 0),
            program: false,
            program2: false,
            target: (0, 0),
            slack: (0, 0),
            follow_dist: 0,
            follow_state: 0,
            kept_step: 0,
            kept_step28: 0,
            detour_count: 0,
            detour_turn: 0,
            detour_len: 0,
            detour_state: 0,
            patrol_side: 0,
            special_step: 0,
            room_changed: 0,
            barrier_latch: 0,
            saved_dir: 0,
            chase_dir: 0,
            joy_timeout: 0,
            joy_dir: 0,
            joy_centre: None,
            key_dir: 0,
            pending_loop: res.ds[0x1882],
            play_disk: false,
            last_pos: (0, 0),
            current: 0,
            wander_toggle: 0,
            random: u16::from_le_bytes([res.ds[0x1E61], res.ds[0x1E62]]),
            heap: res.ds_word(0x1DD8),
            room_ptrs: std::array::from_fn(|i| res.ds_word(0x1DD0 + 2 * i)),
            room_marks: std::array::from_fn(|i| res.ds_word(0x15CA + 2 * i)),
            wander_timer: u16::from_le_bytes([res.ds[0x1E63], res.ds[0x1E64]]),
            order_spare: [(0, 0); 3],
            restoring: false,
            fit_check: false,
            kept_loop_cel: (0, 0),
            water_target: 0,
            pic: crate::pic::State::default(),
            picture: None,
            first_start: true,
            text_attr: res.ds[0x1135],
            text_cursor: (res.ds[0x1134], res.ds[0x1133]),
            gfx_cursor: (0, 0),
            hpos_gfx: res.ds[0x112F],
            hpos_text: res.ds[0x1132],
            hpos: res.ds[0x1881],
            spare: [res.ds[0x185F], res.ds[0x187A], res.ds[0x1880]],
        };
        let disks = [
            Floppy { image: res.disk_image.clone(), write_protected: true, writes: 0 },
            Floppy { image: vec![0; crate::disk::IMAGE_LEN], write_protected: false, writes: 0 },
        ];
        Ok(World {
            res,
            vars: [0; 256],
            objects,
            actors: Default::default(),
            view_table: vec![None; 256],
            view_table_addr: vec![0; 256],
            global: None,
            room: None,
            extra: None,
            g,
            screen: screen::Screen::default(),
            text: text::Text::default(),
            keys: VecDeque::new(),
            disks,
            drive: 0,
            speaker: Vec::new(),
            clock,
            pending: VecDeque::new(),
            checkpoint: 0,
            checkpoint_trace: None,
            cycles: 0,
            timer_on: false,
            passes: 0,
            trace: Vec::new(),
            keep_vars: false,
            vars_trace: Vec::new(),
            max_passes: u64::MAX,
            log: Vec::new(),
            trace_ops: std::env::var_os("KQ_TRACE_OPS").is_some(),
            probe: std::env::var("KQ_PROBE").ok().and_then(|v| {
                let (a, b) = v.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            }),
            frame_hashes: None,
            cost: cost::Cost::default(),
            cost_trace: None,
            drawing_trace: None,
            drawing_room: false,
            fixed_pass_cycles: None,
            pass_start: 0,
            start_room: None,
            charged: 0.0,
            carry: 0.0,
            loading_cycles: 0,
            loading_before: 0.0,
            on_frame: None,
            next_frame: 0,
            font: None,
            frame_at: None,
            frame: None,
            cps: std::env::var("KQ_CPS").ok().and_then(|v| {
                let (a, b) = v.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            }),
        })
    }

    // ---- Time.

    /// Pass the checkpoint at the original's address `at` (clock::CHECKPOINTS): what
    /// is left of the last interval's events fires, and this interval's come.
    pub fn checkpoint(&mut self, at: u16) {
        while let Some(e) = self.pending.pop_front() {
            self.event(e);
        }
        self.charge();
        self.frame_due();
        self.checkpoint += 1;
        self.cost.checkpoint(at);
        if self.cps.is_some_and(|(a, b)| (a..=b).contains(&self.checkpoint)) {
            eprintln!("checkpoint {} at {at:04X}, v25 {}", self.checkpoint, self.vars[25]);
        }
        if let Some(t) = self.checkpoint_trace.as_mut() {
            t.push(at);
        }
        self.pending = self.clock.at(self.checkpoint, self.cycles).into();
    }

    /// The speed model: the cycles of the work counted since the last charge added to
    /// `cycles` (the original's time; the paced clock's ticks follow it).
    pub fn charge(&mut self) {
        let est = self.cost.cycles() + cost::Cost::text_cycles(self.text.drawn, self.text.scrolled);
        self.carry += est - self.charged;
        self.charged = est;
        if self.fixed_pass_cycles.is_some() {
            // The pass's time is fixed (at its end, actor.rs); the work doesn't count.
            self.carry = 0.0;
            return;
        }
        if self.carry >= 1.0 {
            let whole = self.carry.floor();
            self.cycles += whole as u64;
            self.carry -= whole;
        }
        let loading = (self.loading_before + self.cost.loading_cycles()) as u64;
        if loading != self.loading_cycles {
            self.loading_cycles = loading;
            self.clock.loading(loading);
        }
    }

    /// At `site` in the code: the interval's events up to the last tick that
    /// interrupted it fire now.
    pub fn site(&mut self, site: clock::Site) {
        let Some(last) = self.pending.iter().rposition(|e| matches!(e, Event::Tick(ip) if site.contains(*ip))) else {
            return;
        };
        for _ in 0..=last {
            if let Some(e) = self.pending.pop_front() {
                self.event(e);
            }
        }
    }

    /// Code reading the BIOS keyboard with none waiting: the interval's events up to
    /// its next key press fire now.
    pub fn site_keys(&mut self) {
        if !self.keys.is_empty() {
            return;
        }
        let Some(last) = self.pending.iter().position(|e| matches!(e, Event::Key(_))) else { return };
        for _ in 0..=last {
            if let Some(e) = self.pending.pop_front() {
                self.event(e);
            }
        }
    }

    /// A busy wait until `done`: the original loops until its timer handler changes
    /// something. The interval's events fire one by one until it is done.
    pub fn wait_until(&mut self, what: &str, done: impl Fn(&World) -> bool) {
        while !done(self) {
            if self.pending.is_empty() {
                self.pending = self.clock.waiting(self.checkpoint, self.cycles).into();
                if self.pending.is_empty() {
                    self.stop(format!("waiting for {what} at checkpoint {}, with no tick to come", self.checkpoint));
                }
            }
            if let Some(e) = self.pending.pop_front() {
                if matches!(e, Event::Tick(_)) {
                    self.cost.wait_ticks += 1;
                    self.cycles = (self.cycles / clock::CYCLES_PER_TICK + 1) * clock::CYCLES_PER_TICK;
                }
                self.event(e);
                self.frame_due();
            }
        }
    }

    /// A timed delay of `n` cycles (the title's): with a clock for play, the time passes
    /// a frame at a time and the keys and ticks due come in; a delay that polls the
    /// keyboard (`keys_end_it`) ends early on a key (true). With a replay, the time is
    /// added at once.
    pub fn delay(&mut self, n: u64, keys_end_it: bool) -> bool {
        let end = self.cycles + n;
        while self.cycles < end {
            self.cycles += (end - self.cycles).min(clock::CYCLES_PER_FRAME);
            for e in self.clock.poll(self.cycles) {
                self.event(e);
            }
            self.frame_due();
            if keys_end_it && !self.keys.is_empty() {
                return true;
            }
        }
        false
    }

    /// The front end's look, when 1/60 s of the game's time has gone by.
    fn frame_due(&mut self) {
        if self.on_frame.is_none() || self.cycles < self.next_frame {
            return;
        }
        self.next_frame = self.cycles + clock::CYCLES_PER_FRAME;
        if let Some(mut f) = self.on_frame.take() {
            f(self);
            self.on_frame = Some(f);
        }
    }

    fn event(&mut self, e: Event) {
        // KQ_EVENTS=N: print each event fired at checkpoint N, with the keys and the ring.
        if std::env::var("KQ_EVENTS").ok().and_then(|v| v.parse::<u64>().ok()) == Some(self.checkpoint) {
            eprintln!(
                "event {e:?} at checkpoint {}: keys {:?} ring {:?} keys_on {}",
                self.checkpoint, self.keys, self.g.ring, self.g.keys_on
            );
        }
        match e {
            Event::Tick(_) => {
                if self.timer_on {
                    self.cost.ticks += 1;
                    self.int1c();
                }
            }
            Event::Key(k) => self.keys.push_back(k),
            Event::Disk(d) => self.drive = (d as usize).min(1),
        }
    }

    /// End the run.
    pub fn stop(&self, why: String) -> ! {
        std::panic::panic_any(Stop(why))
    }

    /// Run the game from power-on until it stops; the reason it stopped.
    pub fn run(&mut self) -> String {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let p = info.payload();
            if p.downcast_ref::<Stop>().is_none() && p.downcast_ref::<save::RestartNow>().is_none() {
                hook(info);
            }
        }));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.start()));
        let _ = std::panic::take_hook();
        match r {
            Ok(()) => "the game ended".into(),
            Err(e) => match e.downcast::<Stop>() {
                Ok(s) => s.0,
                Err(e) => std::panic::resume_unwind(e),
            },
        }
    }

    /// The screen as the player sees it, as refemu-pc's runner gives it (`frame`): 1
    /// and the 40 × 25 characters when a text mode is up; else 0, the CGA's memory in its
    /// 640-dot mode (200 lines of 80 bytes) and the characters. The picture's 168 lines:
    /// each pixel its colour's dot pattern, two to a byte, as the copy to the screen makes
    /// them (`$1966`); over them, and in text rows 21-24, the characters as the BIOS draws
    /// them in mode 4 (the font's dots in the attribute's colour, over the cell, or XORed
    /// onto it with attribute bit 7), where they are still shown.
    pub fn frame(&self) -> Vec<u8> {
        let mut f = Vec::with_capacity(1 + 80 * 200 + 1000);
        f.push(self.text.text_mode as u8);
        if !self.text.text_mode {
            let p = |c: u8| crate::composite::PATTERN[(c & 15) as usize];
            let mut cga: Vec<u8> = self.screen.shown.chunks(2).map(|c| p(c[0]) << 4 | p(c[1])).collect();
            cga.resize(80 * 200, 0);
            let g = &self.text.graphics;
            for r in 0..25 {
                for c in 0..40 {
                    if r < 21 && !g.visible[r][c] {
                        continue;
                    }
                    let (ch, attr) = (g.cells[r][c], g.attrs[r][c]);
                    for line in 0..8 {
                        let glyph = self.font.as_ref().map_or(0, |f| f.row(ch, line));
                        let [hi, lo] = crate::font::expand(glyph, attr);
                        let at = (r * 8 + line) * 80 + c * 2;
                        if attr & 0x80 != 0 {
                            cga[at] ^= hi;
                            cga[at + 1] ^= lo;
                        } else {
                            cga[at] = hi;
                            cga[at + 1] = lo;
                        }
                    }
                }
            }
            f.extend(cga);
        }
        f.extend(self.text.grid().cells.iter().flatten());
        f
    }

    /// The trace row for the pass starting now (refemu-pcjr's columns).
    pub fn pass_row(&self) -> PassRow {
        let vars_hash =
            self.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
        PassRow {
            pass: self.passes,
            room: self.vars[1],
            ego_x: self.actors[0].x,
            ego_y: self.actors[0].y,
            ego_dir: self.actors[0].motion,
            score: self.vars[19],
            vars_hash,
            cycles: self.cycles,
        }
    }
}
