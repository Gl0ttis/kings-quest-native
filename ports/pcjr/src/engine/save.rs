//! Saving, restoring and restarting (`$0A30`, `$38B8`-`$3C1C`).
//!
//! The original saves to its Play diskette: 3 sectors a slot (26 slots, `a`-`z`, from
//! sector 282h), holding "vrsn#", the slot, the variables, the object table and the
//! actor table with the globals after it, as they are in its memory. Restarting
//! restores slot FFh, which the Master holds at sector 27Fh, and then clears most of
//! the state again: of it, only the object table and two bytes stay.

use super::{Object, World, OBJECTS};

/// The save block's layout.
const SIGNATURE: &[u8; 5] = b"vrsn#";
const VARS_AT: usize = 6;
const OBJECTS_AT: usize = VARS_AT + 256;
const ACTORS_AT: usize = OBJECTS_AT + 8 * OBJECTS;
/// Bytes saved: the signature and slot, the variables, the objects, DS:11E1-127C.
pub const SAVE_LEN: usize = ACTORS_AT + 0x9C;

impl World {
    /// The 3 sectors of slot `slot` (FF: the initial state on the Master).
    fn slot_sector(slot: u8) -> usize {
        if slot == 0xFF {
            0x27F
        } else {
            0x282 + 3 * slot as usize
        }
    }

    /// `$3B99`: read slot `slot` from the disk in the drive: its block, or the error
    /// (10: not that slot).
    fn read_slot(&mut self, slot: u8) -> Result<Vec<u8>, u8> {
        self.checkpoint(0x3B99);
        self.g.keys_on = false;
        self.checkpoint(0x4D93);
        self.site(super::clock::Site::Disk);
        self.cycles += 3 * super::actor::SECTOR_CYCLES;
        self.g.keys_on = true;
        let at = Self::slot_sector(slot) * 512;
        let block = self.disk_image().get(at..at + SAVE_LEN).map(<[u8]>::to_vec).unwrap_or_default();
        if block.len() < SAVE_LEN || &block[..5] != SIGNATURE || block[5] != slot {
            return Err(10);
        }
        Ok(block)
    }

    /// The image of the disk in the drive.
    fn disk_image(&self) -> &[u8] {
        &self.res.disk_image
    }

    /// The object table from a save block.
    fn objects_from(block: &[u8]) -> [Object; OBJECTS] {
        let mut o = [Object::default(); OBJECTS];
        for (i, r) in o.iter_mut().enumerate() {
            let b = &block[OBJECTS_AT + 8 * i..OBJECTS_AT + 8 * i + 8];
            *r = Object {
                name: u16::from_le_bytes([b[0], b[1]]),
                room: b[2],
                loop_cel: b[4],
                x: b[5],
                y: b[6],
                slot: b[7],
            };
        }
        o
    }

    /// `$0A30`: restart. Slot FF is restored; then everything from the key flags to the
    /// joystick's direction (DS:0E2E-1279: the variables, the actors, the barriers,
    /// the parser...) is cleared, and the music's countdown and the input flags before
    /// them set to 1. What is left of the restored state is the object table and the
    /// pending set_loop. Then the start again (without the title).
    pub fn restart(&mut self) {
        let block = match self.read_slot(0xFF) {
            Ok(b) => b,
            Err(e) => self.stop(format!("restart: the initial state isn't on the disk (error {e})")),
        };
        self.objects = Self::objects_from(&block);
        self.g.pending_loop = block[ACTORS_AT + (0x127C - 0x11E1)];
        // DS:0E24-0E2D = 1.
        self.g.music_countdown = 1;
        self.g.sound_on = true;
        self.g.keys_on = true;
        self.g.prompt_shown = true;
        self.g.matched = true;
        self.g.parse.phrase = 1;
        self.g.prompt_due = true;
        // DS:0E2E-1279 = 0.
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
        let line = std::mem::take(&mut self.g.parse.line);
        let (count, at, column) = (self.g.parse.count, self.g.parse.at, self.g.parse.column);
        self.g.parse = super::input::Parse {
            phrase: 1,
            line: line.iter().map(|_| 0).collect(),
            count,
            at,
            column,
            ..Default::default()
        };
        self.global = None;
        self.room = None;
        self.extra = None;
        self.vars = [0; 256];
        self.view_table = vec![None; 256];
        self.actors = Default::default();
        let g = &mut self.g;
        g.input_off = false;
        g.actor_count = 0;
        g.barrier_x = 0;
        g.barrier_y = 0;
        g.band_x = (0, 0);
        g.band_y = (0, 0);
        g.program = false;
        g.program2 = false;
        g.target = (0, 0);
        g.slack = (0, 0);
        g.follow_dist = 0;
        g.follow_state = 0;
        g.kept_step = 0;
        g.detour_count = 0;
        g.detour_turn = 0;
        g.patrol_side = 0;
        g.special_step = 0;
        g.room_changed = 0;
        g.kept_step28 = 0;
        g.detour_len = 0;
        g.detour_state = 0;
        g.barrier_latch = 0;
        g.saved_dir = 0;
        g.chase_dir = 0;
        g.joy_timeout = 0;
        g.joy_dir = 0;
        g.key_dir = 0;
        self.clear_actors();
        self.vars[24] = 3;
    }

    /// `$38B8`: the save or restore dialog.
    pub fn save_restore_dialog(&mut self) {
        self.stop("the save and restore dialog isn't ported yet".into());
    }

    /// `$3C1C`: op 42: make a Play diskette.
    pub fn make_play_diskette(&mut self) {
        self.stop("making a Play diskette isn't ported yet".into());
    }

    /// `$3ED9`: op 31: the inventory screen.
    pub fn inventory_screen(&mut self) {
        self.stop("the inventory screen isn't ported yet".into());
    }

    /// `$4031`: op 3C: the text screen.
    pub fn text_screen(&mut self, _fg: u8, _bg: u8) {
        self.stop("the text screen isn't ported yet".into());
    }

    /// `$3F9C`: op 3D.
    pub fn print_at(&mut self, _s: &super::res::Script, _row: u8, _col: u8, _msg: u16) {
        self.stop("print_at isn't ported yet".into());
    }

    /// `$3FB9`: op 3E.
    pub fn print_text(&mut self, _s: &super::res::Script, _msg: u16) {
        self.stop("print_text isn't ported yet".into());
    }

    /// `$40A6`: back to graphics.
    pub fn leave_text_mode(&mut self) {
        self.stop("the text screen isn't ported yet".into());
    }
}
