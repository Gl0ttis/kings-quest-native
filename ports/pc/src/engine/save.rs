//! Saving, restoring, restarting and making a Play diskette (`$1C33`, `$4CA3`-`$0785`).
//!
//! The original saves to its Play diskette: 3 sectors a slot (26 slots, `a`-`z`, from
//! sector 282h), holding "vrsn#", the slot, the variables, the object table, and the
//! actor table with the globals after it, as they are in its memory. The port writes
//! the same blocks, so its Play diskettes and the original's are the same. The actor
//! records hold pointers into the original's memory (to a view, and to a cel in it),
//! which its restore takes as they are: the room's script, run again, loads its views
//! where they were. The port keeps that memory's layout (`g.heap`, actor.rs) and writes
//! the same pointers; restoring, it takes a record's view when one is loaded where it
//! points (until then the view of the record's view number). Saves the port wrote
//! before it kept the layout name a view as 2000h + 100h × its number; they read too.
//!
//! Restarting restores slot FFh (the Master holds it at sector 27Fh), then clears most
//! of the state again: of it, only the object table and two bytes stay.

use super::{Actor, CelRef, Object, World, ACTORS, OBJECTS};

/// The save block: "vrsn#", the slot, v0-v255, the objects, DS:17E7-127C.
const SIGNATURE: &[u8; 5] = b"vrsn#";
const VARS_AT: usize = 6;
const OBJECTS_AT: usize = VARS_AT + 256;
const ACTORS_AT: usize = OBJECTS_AT + 8 * OBJECTS;
/// The original's actor table holds 4 records; the globals DS:185F-127C follow.
const RECORDS: usize = 4;
const GLOBALS_AT: usize = ACTORS_AT + 30 * RECORDS;
pub const SAVE_LEN: usize = GLOBALS_AT + 0x24;
/// Where the port's earlier saves said a view was: 2000h + 100h × its number.
const VIEW_BASE: u16 = 0x2000;
/// The stack pointer the start-up saves at DS:0000 (`$53`): ([1168h] − DS) × 16, with
/// [1168h] = 1300h and DS = 0571h.
const START_SP: u16 = 0xD8F0;
/// The background save offset of each slot (DS:1320).
pub const SAVE_OFFSETS: usize = 0x1320;
/// "Games may only be saved on your Play diskette.", "Disk Write Error!...".
const NOT_ON_PLAY: usize = 0x1064;
const WRITE_ERROR: usize = 0x1024;
/// The Play diskette's strings: the question, the label, "insert Master", "insert
/// Play", "write error", "ready".
const PLAY_ASK: usize = 0x0CC3;
const PLAY_LABEL: usize = 0x0D15;
const INSERT_MASTER: usize = 0x0D67;
const INSERT_PLAY: usize = 0x0DAE;
const COPY_WRITE_ERROR: usize = 0x0E3F;
const PLAY_READY: usize = 0x0E00;

/// Raised to jump to the restart (`jmp $1C33`), which resets the stack: caught by
/// [`World::start`].
pub struct RestartNow;

impl World {
    /// The 3 sectors of slot `slot` (FF: the initial state).
    /// On the PC (`$07DA`): 3Fh + 3 × slot, the initial state (FF, taken as 1Ah) at
    /// 8Dh, the data area's first sector.
    fn slot_sector(slot: u8) -> usize {
        let n = if slot == 0xFF { 0x1A } else { slot as usize };
        0x3F + 3 * n
    }

    // ---- The save block.

    fn encode_actor(&self, a: &Actor, out: &mut [u8]) {
        let base = a.pending_view.unwrap_or(a.view_addr);
        out[0] = a.room;
        out[1..3].copy_from_slice(&base.to_le_bytes());
        out[3] = a.lp;
        out[4] = a.no_move;
        out[5] = a.view_num;
        out[6] = 0;
        out[7] = a.cel_count;
        out[8] = a.cel;
        out[9] = a.delay;
        out[10] = a.countdown;
        out[11] = a.flags;
        out[12] = a.step_x;
        out[13] = a.step_y;
        out[14] = a.cycle;
        out[15] = a.motion;
        out[16] = a.dir;
        out[17..19].copy_from_slice(&a.save_offset.to_le_bytes());
        out[19] = a.slot;
        out[20..22].copy_from_slice(&a.cel_drawn.ptr.to_le_bytes());
        out[22..24].copy_from_slice(&a.cel_new.ptr.to_le_bytes());
        out[24] = a.x;
        out[25] = a.y;
        out[26] = a.new_x;
        out[27] = a.new_y;
        out[28] = a.hidden;
        out[29] = a.priority;
    }

    fn decode_actor(&mut self, b: &[u8], keep_saved: Vec<u8>) -> Actor {
        let word = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let base = word(1);
        // The view: a save the port wrote before it kept the original's memory names
        // it by its pointer; otherwise the view number stands in until a view is loaded
        // where the pointer points (load_view).
        let old_port =
            base >= VIEW_BASE && (base - VIEW_BASE).is_multiple_of(0x100) && (base - VIEW_BASE) / 0x100 < 111;
        let num = if old_port { ((base - VIEW_BASE) / 0x100) as u8 } else { b[5] };
        let view = if base == 0 { None } else { self.res.view(num) };
        let cel = |p: u16| -> CelRef {
            match &view {
                Some(v) => {
                    let (lp, cel) = v.cel_at(p.wrapping_sub(base) as usize).unwrap_or((b[3], b[8]));
                    CelRef { view: Some(v.clone()), lp, cel, ptr: p }
                }
                None => CelRef { ptr: p, ..CelRef::default() },
            }
        };
        Actor {
            room: b[0],
            view: view.clone(),
            view_addr: base,
            pending_view: (base != 0 && !old_port).then_some(base),
            lp: b[3],
            no_move: b[4],
            view_num: b[5],
            cel_count: b[7],
            cel: b[8],
            delay: b[9],
            countdown: b[10],
            flags: b[11],
            step_x: b[12],
            step_y: b[13],
            cycle: b[14],
            motion: b[15],
            dir: b[16],
            slot: b[19],
            saved: keep_saved,
            save_offset: word(17),
            cel_drawn: cel(word(20)),
            cel_new: cel(word(22)),
            x: b[24],
            y: b[25],
            new_x: b[26],
            new_y: b[27],
            hidden: b[28],
            priority: b[29],
        }
    }

    /// The globals DS:185F-127C, in order.
    fn globals_bytes(&self) -> [u8; 0x24] {
        let g = &self.g;
        [
            g.spare[0],
            g.input_off as u8,
            g.actor_count,
            g.barrier_x,
            g.barrier_y,
            g.band_x.0,
            g.band_x.1,
            g.band_y.0,
            g.band_y.1,
            g.program as u8,
            g.program2 as u8,
            g.target.0,
            g.target.1,
            g.slack.0,
            g.slack.1,
            g.follow_dist,
            g.follow_state,
            g.kept_step,
            g.detour_count,
            g.detour_turn,
            g.patrol_side,
            g.special_step,
            g.room_changed,
            g.kept_step28,
            g.detour_len,
            g.detour_state,
            g.barrier_latch,
            g.spare[1],
            g.saved_dir,
            g.chase_dir,
            g.joy_timeout,
            g.joy_dir,
            g.key_dir,
            g.spare[2],
            g.hpos,
            g.pending_loop,
        ]
    }

    fn set_globals_bytes(&mut self, b: &[u8]) {
        let g = &mut self.g;
        g.spare[0] = b[0];
        g.input_off = b[1] != 0;
        g.actor_count = b[2];
        g.barrier_x = b[3];
        g.barrier_y = b[4];
        g.band_x = (b[5], b[6]);
        g.band_y = (b[7], b[8]);
        g.program = b[9] != 0;
        g.program2 = b[10] != 0;
        g.target = (b[11], b[12]);
        g.slack = (b[13], b[14]);
        g.follow_dist = b[15];
        g.follow_state = b[16];
        g.kept_step = b[17];
        g.detour_count = b[18];
        g.detour_turn = b[19];
        g.patrol_side = b[20];
        g.special_step = b[21];
        g.room_changed = b[22];
        g.kept_step28 = b[23];
        g.detour_len = b[24];
        g.detour_state = b[25];
        g.barrier_latch = b[26];
        g.spare[1] = b[27];
        g.saved_dir = b[28];
        g.chase_dir = b[29];
        g.joy_timeout = b[30];
        g.joy_dir = b[31];
        g.key_dir = b[32];
        g.spare[2] = b[33];
        g.hpos = b[34];
        g.pending_loop = b[35];
    }

    /// The save block for `slot`.
    fn encode(&self, slot: u8) -> Vec<u8> {
        let mut b = vec![0u8; SAVE_LEN];
        b[..5].copy_from_slice(SIGNATURE);
        b[5] = slot;
        b[VARS_AT..VARS_AT + 256].copy_from_slice(&self.vars);
        for (i, o) in self.objects.iter().enumerate() {
            let r = &mut b[OBJECTS_AT + 8 * i..OBJECTS_AT + 8 * i + 8];
            r[..2].copy_from_slice(&o.name.to_le_bytes());
            r[2] = o.room;
            r[3] = o.extra;
            r[4] = o.loop_cel;
            r[5] = o.x;
            r[6] = o.y;
            r[7] = o.slot;
        }
        for (i, a) in self.actors.iter().take(RECORDS).enumerate() {
            self.encode_actor(a, &mut b[ACTORS_AT + 30 * i..ACTORS_AT + 30 * i + 30]);
        }
        b[GLOBALS_AT..].copy_from_slice(&self.globals_bytes());
        b
    }

    fn objects_from(block: &[u8]) -> [Object; OBJECTS] {
        let mut o = [Object::default(); OBJECTS];
        for (i, r) in o.iter_mut().enumerate() {
            let b = &block[OBJECTS_AT + 8 * i..OBJECTS_AT + 8 * i + 8];
            *r = Object {
                name: u16::from_le_bytes([b[0], b[1]]),
                room: b[2],
                extra: b[3],
                loop_cel: b[4],
                x: b[5],
                y: b[6],
                slot: b[7],
            };
        }
        o
    }

    /// The state from a save block (`$08A9`): the variables, the objects, the actors
    /// and the globals after them.
    fn decode(&mut self, b: &[u8]) {
        self.vars.copy_from_slice(&b[VARS_AT..VARS_AT + 256]);
        self.objects = Self::objects_from(b);
        for i in 0..RECORDS {
            let saved = std::mem::take(&mut self.actors[i].saved);
            let r = b[ACTORS_AT + 30 * i..ACTORS_AT + 30 * i + 30].to_vec();
            self.actors[i] = self.decode_actor(&r, saved);
        }
        for a in &mut self.actors[RECORDS..ACTORS] {
            *a = Actor::default();
        }
        self.set_globals_bytes(&b[GLOBALS_AT..SAVE_LEN]);
    }

    // ---- The disks.

    /// INT 13h on the disk in the drive: read or write `count` sectors from `lba`
    /// (`data` the bytes to write, or filled by a read). The status (3: write-protected).
    fn disk(&mut self, write: bool, lba: usize, count: usize, data: &mut Vec<u8>) -> u8 {
        let d = &mut self.disks[self.drive];
        let range = lba * 512..(lba + count) * 512;
        if write {
            if d.write_protected {
                return 3;
            }
            if let Some(dst) = d.image.get_mut(range) {
                dst.copy_from_slice(&data[..count * 512]);
                d.writes += 1;
            }
        } else {
            *data = d.image.get(range).map_or_else(|| vec![0; count * 512], <[u8]>::to_vec);
        }
        0
    }

    /// `$0867`: read slot `slot` from the disk in the drive: its block, or the error
    /// (10: not that slot).
    fn load_slot(&mut self, slot: u8) -> Result<Vec<u8>, u8> {
        self.checkpoint(0x0867);
        self.read_sectors_counted(3);
        let mut data = Vec::new();
        self.disk(false, Self::slot_sector(slot), 3, &mut data);
        if &data[..5] != SIGNATURE || data[5] != slot {
            return Err(10);
        }
        Ok(data)
    }

    /// `$078E`: save to slot `slot`. Before a real slot, the disk's boot sector is
    /// written with the start of the engine's data segment, DS:0000-01FF, whose first
    /// word is the stack pointer the start-up keeps there (`$53`: D8F0h) (it fails on
    /// the Master, which is write-protected). Writing on the Master fails with "Games may only be
    /// saved on your Play diskette."; another error asks for a key and tries again.
    fn save_slot(&mut self, slot: u8) {
        self.checkpoint(0x078E);
        if slot != 0xFF {
            let mut boot: Vec<u8> = self.res.ds[..512].to_vec();
            boot[..2].copy_from_slice(&START_SP.to_le_bytes());
            self.disk(true, 0, 1, &mut boot);
        }
        // $07A0: the screen position goes in the save (DS:1881): on the PC the game's own
        // (`$19D3` reads DS:1881 back), where the PCjr takes the BIOS's.
        let mut b = self.encode(slot);
        b.resize(3 * 512, 0);
        self.g.keys_on = false;
        loop {
            self.cost.sectors += 3;
            match self.disk(true, Self::slot_sector(slot), 3, &mut b) {
                0 => break,
                3 => {
                    self.print_now(NOT_ON_PLAY);
                    break;
                }
                _ => {
                    self.print_now(WRITE_ERROR);
                    self.g.keys_on = false;
                    self.blocking_key();
                    self.g.keys_on = true;
                }
            }
        }
        self.g.keys_on = true;
    }

    /// `$0842`: print an engine string at once (whole, and waiting for it).
    fn print_now(&mut self, at: usize) {
        self.g.print_all = true;
        self.queue_engine_string(at);
        self.wait_print_done();
        self.g.print_all = false;
    }

    /// INT 16h AH=0: wait for a key and take it.
    pub fn blocking_key(&mut self) -> u16 {
        self.site_keys();
        self.wait_until("a key", |w| !w.keys.is_empty());
        self.keys.pop_front().unwrap_or(0)
    }

    /// `$08DF`.
    fn key_wait(&mut self) -> u8 {
        self.checkpoint(0x08DF);
        self.blocking_key() as u8
    }

    /// `$08D4`: the key waiting, not taken (0 if none).
    fn key_poll(&mut self) -> u8 {
        self.checkpoint(0x08D4);
        self.site_keys();
        self.keys.front().map_or(0, |&k| k as u8)
    }

    /// `$1FB6` for a read whose data the caller takes from the disk.
    fn read_sectors_counted(&mut self, n: u16) {
        self.g.keys_on = false;
        self.checkpoint(0x1FB6);
        self.site(super::clock::Site::Disk);
        self.cost.sectors += n as u32;
        self.g.keys_on = true;
    }

    // ---- The dialog.

    /// `$4CA3`: the save or restore dialog. After making a Play diskette (1E91), slot 0
    /// is restored without asking.
    pub fn save_restore_dialog(&mut self) {
        self.checkpoint(0x4CA3);
        let mut slot = 0u8;
        let mut ask = true;
        if self.g.restoring {
            self.g.restoring = false;
            ask = false;
        }
        loop {
            if ask {
                slot = self.ask_slot();
                if slot == 0x1A {
                    return;
                }
                if self.g.save_requested {
                    // The hidden slot FF (Space, then Ctrl-]) writes the initial state.
                    self.save_slot(slot);
                    return;
                }
            }
            ask = true;
            match self.load_slot(slot) {
                Ok(b) => {
                    self.decode(&b);
                    self.restored();
                    return;
                }
                Err(_) => self.queue_engine_string(0x2018),
            }
        }
    }

    /// `$4D20`: after a restore: the ego off screen, object 0 in this room, the actors
    /// cleared, the room's script loaded again; v29 = 1 (restored), v25 = 0.
    fn restored(&mut self) {
        self.actors[0].room = 0xFF;
        self.wait_print_done();
        let r = self.vars[1];
        self.objects[0].room = r;
        self.clear_actors();
        self.stop_music();
        self.room = Some(self.load_room_script(r));
        // $4D40: the room script's sections kept, as new_room keeps them.
        self.g.room_marks = self.g.room_ptrs;
        self.vars[29] = 1;
        self.vars[25] = 0;
    }

    /// `$4DBE`: which slot? A letter (0-25), 1Ah for Enter, or FF for Space then
    /// Ctrl-] when saving.
    fn ask_slot(&mut self) -> u8 {
        self.wait_print_done();
        self.g.print_all = true;
        self.g.keys_on = false;
        self.queue_engine_string(0x2052);
        self.queue_engine_string(0x2077);
        let col = if self.g.save_requested {
            self.queue_engine_string(0x2091);
            0x20
        } else {
            self.queue_engine_string(0x20A0);
            self.queue_engine_string(0x20AD);
            0x25
        };
        self.queue_engine_string(0x20B4);
        self.wait_print_done();
        self.g.print_all = false;
        let r = loop {
            let k = self.key_wait();
            if k == 0x0D {
                break 0x1A;
            }
            if k == b' ' {
                let mut k = self.key_poll();
                while k == 0 {
                    k = self.key_poll();
                }
                if self.g.save_requested && k == 0x1D {
                    break 0xFF;
                }
                continue;
            }
            if k.is_ascii_lowercase() {
                self.text.cursor = (0x18, col);
                self.put_char(k);
                break k - b'a';
            }
        };
        self.text_newline();
        self.g.keys_on = true;
        r
    }

    // ---- Restarting.

    /// `$1C33`: restart. Slot FF is restored from the disk in the drive; then
    /// everything from the key flags to the keypad's direction (DS:1432-1279: the
    /// variables, the actors, the barriers, the parser...) is cleared, and the music's
    /// countdown and the flags before them set to 1. What is left of the restored
    /// state is the object table, the horizontal position and the pending set_loop.
    pub fn restart(&mut self) {
        let block = match self.load_slot(0xFF) {
            Ok(b) => b,
            Err(e) => self.stop(format!("restart: no initial state on the disk (error {e})")),
        };
        self.objects = Self::objects_from(&block);
        let kept = (block[GLOBALS_AT + 0x21], block[GLOBALS_AT + 0x22], block[GLOBALS_AT + 0x23]);
        // DS:1428-0E2D = 1.
        self.g.music_countdown = 1;
        self.g.sound_on = true;
        self.g.keys_on = true;
        self.g.prompt_shown = true;
        self.g.matched = true;
        self.g.prompt_due = true;
        // DS:1432-1279 = 0.
        let line_len = self.g.parse.line.len();
        let (count, at, column) = (self.g.parse.count, self.g.parse.at, self.g.parse.column);
        self.g.parse =
            super::input::Parse { phrase: 1, line: vec![0; line_len], count, at, column, ..Default::default() };
        self.g.key_flags = [false; 6];
        self.g.debug = false;
        self.g.queue.clear();
        self.g.print_all = false;
        self.g.ring.clear();
        self.g.save_requested = false;
        self.g.music_on = false;
        self.g.music_next = None;
        self.g.music = None;
        self.g.music_start = None;
        self.g.paused = false;
        self.g.selected = 0;
        self.g.text_col = 0;
        self.global = None;
        self.room = None;
        self.extra = None;
        self.vars = [0; 256];
        self.view_table = vec![None; 256];
        self.view_table_addr = vec![0; 256];
        self.actors = Default::default();
        let mut g = [0u8; 0x24];
        (g[0x21], g[0x22], g[0x23]) = kept;
        self.set_globals_bytes(&g);
        self.clear_actors();
        // $1C5B: the resource memory from its start.
        self.g.heap = 0x20D8;
        self.vars[24] = 3;
    }

    // ---- The Play diskette.

    /// `$0488`: op 42: make a Play diskette. On "y" (read from the BIOS, `$04B3`): a blank
    /// diskette is formatted and the Master's data area (sectors 141-719) copied to it in
    /// three passes, through memory (the second part of each pass at segment 1000h,
    /// over the picture buffer), the disks swapped by the player as asked; then the
    /// game is saved to slot 0, and the game restarts and restores it.
    pub fn make_play_diskette(&mut self) {
        self.g.text_attr = 0x0F;
        self.enter_text_mode();
        self.g.text_cursor = (0, 0);
        self.print_ds(PLAY_ASK);
        self.g.keys_on = false;
        loop {
            match self.blocking_key() as u8 {
                b'y' => break,
                b'n' => {
                    self.g.keys_on = true;
                    self.leave_text_mode();
                    return;
                }
                _ => {}
            }
        }
        self.stop_music();
        self.print_ds(PLAY_LABEL);
        self.copy_master();
        self.save_slot(0);
        self.g.restoring = true;
        self.g.key_flags[2] = true;
        self.g.keys_on = true;
        std::panic::panic_any(RestartNow);
    }

    /// `$0552`: the copy: three passes of 65 + 128 sectors from sector 141 (`$04DE`),
    /// the first pass formatting the Play diskette (`$062C`).
    fn copy_master(&mut self) {
        let mut next = 0x8Dusize;
        let mut passes = 4u8;
        loop {
            let (at1, at2) = (next, next + 0x41);
            self.prompt_enter(INSERT_MASTER);
            self.verify_master();
            self.read_sectors_counted(0x41);
            let mut chunk1 = Vec::new();
            self.disk(false, at1, 0x41, &mut chunk1);
            self.read_sectors_counted(0x80);
            let mut chunk2 = Vec::new();
            self.disk(false, at2, 0x80, &mut chunk2);
            // The second part is read into segment 1000h (`$05C7`), 64 KB, over the
            // picture buffer at 1400h.
            self.screen.load_pages(&chunk2);
            self.prompt_enter(INSERT_PLAY);
            if passes == 4 {
                // The format also counts a pass.
                passes -= 1;
                self.format_play();
            }
            self.write_retrying(at1, 0x41, &mut chunk1);
            self.write_retrying(at2, 0x80, &mut chunk2);
            next = at2 + 0x80;
            passes -= 1;
            if passes == 0 {
                break;
            }
        }
        // $0591: back to graphics (mode 4 clears the screen, `$19A4`), at the graphics
        // screen's position (DS:112F, `$059C`); "ready".
        self.screen.shown.fill(0);
        self.text.text_mode = false;
        self.g.hpos = self.g.hpos_gfx;
        self.text.graphics = Default::default();
        self.text.cursor = (0, 0);
        for b in self.res.ds_bytes(PLAY_READY, 0) {
            self.text.teletype(b, 0x0F);
        }
    }

    /// A prompt, then Enter: on the PC through the Enter wait (`$0785`, a checkpoint at
    /// each key), where the PCjr reads the keys from the BIOS directly.
    fn prompt_enter(&mut self, at: usize) {
        self.print_ds(at);
        self.wait_enter();
    }

    /// `$05E8`: is the disk in the drive the Master? Its sectors 10-11 are the resource
    /// table the engine holds at CS:0088 (all 1,024 bytes compared); until they are,
    /// "insert the Master".
    fn verify_master(&mut self) {
        loop {
            self.read_sectors_counted(2);
            let mut s = Vec::new();
            self.disk(false, 10, 2, &mut s);
            if s[..0x400] == self.res.code[0x88..0x488] {
                return;
            }
            self.clear_text_screen(true);
            self.print_ds(INSERT_MASTER);
            self.wait_enter();
        }
    }

    /// `$0785`: wait for Enter.
    fn wait_enter(&mut self) {
        loop {
            self.checkpoint(0x0785);
            if self.blocking_key() as u8 == 0x0D {
                return;
            }
        }
    }

    /// `$062C`: a write, retried (after "Disk Write Error!" and Enter) until it works.
    fn write_retrying(&mut self, lba: usize, count: usize, data: &mut Vec<u8>) {
        loop {
            self.cost.sectors += count as u32;
            if self.disk(true, lba, count, data) == 0 {
                return;
            }
            let keep = self.g.text_cursor;
            self.print_ds(COPY_WRITE_ERROR);
            self.g.text_cursor = keep;
            self.wait_enter();
        }
    }

    /// `$06C8`: format the disk in the drive, 40 cylinders, 2 heads, each sector filled
    /// with F6. On a write-protected disk (the Master), "insert the Play diskette" and
    /// Enter, and that track again.
    fn format_play(&mut self) {
        for track in 0..80usize {
            loop {
                self.cost.sectors += 9;
                let d = &mut self.disks[self.drive];
                if !d.write_protected {
                    d.image[track * 9 * 512..(track + 1) * 9 * 512].fill(0xF6);
                    d.writes += 1;
                    break;
                }
                self.clear_text_screen(true);
                self.print_ds(INSERT_PLAY);
                self.wait_enter();
            }
        }
    }
}
