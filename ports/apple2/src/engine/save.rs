//! Save and restore (port plan M5). The original writes one track per letter on the save
//! disk (volume 10): a header sector carrying the signature "ame  " ($7039, bytes 1-5),
//! then four blocks ($5941):
//!
//!   object table  $1A80  $500 bytes  (111 records, names, and 80 further bytes)
//!   actor table   $0100  $07C bytes
//!   zero page     $0060  $090 bytes  (engine globals, parsed words, v0-v31 at $D0)
//!   variables     $6E06  $0DF bytes  (v32-v222 live at $6E06+v)
//!
//! The port keeps exactly this data, in this layout, in a file per letter. Restoring
//! re-enters the saved room the way $4F88-$4FC7 does, so the room scripts redraw it.

use super::{f, World, MAX_ACTORS};

const MAGIC: &[u8; 8] = b"KQSAVE01";
const OBJECTS: usize = 0x500;
const ACTORS: usize = 0x7C;
const ZP: usize = 0x90;
const VARS: usize = 0xDF;
pub const SAVE_LEN: usize = 8 + OBJECTS + ACTORS + ZP + VARS;

impl World {
    /// The saved state, in the original's block layout.
    pub fn save_image(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        // Object table: the 1,200 bytes the port holds, then the 80 bytes beyond them,
        // which the port does not model (written as zero).
        let mut objects: Vec<u8> = self.objects.iter().flatten().copied().collect();
        objects.resize(OBJECTS, 0);
        out.extend_from_slice(&objects);
        for s in 0..MAX_ACTORS {
            out.extend_from_slice(&self.actors[s]);
        }
        out.extend_from_slice(&self.zero_page());
        let mut vars = [0u8; VARS];
        vars[0x20..].copy_from_slice(&self.vars[0x20..VARS]);
        out.extend_from_slice(&vars);
        out
    }

    /// Zero page $60-$EF as the engine would hold it, for the fields the port models.
    fn zero_page(&self) -> [u8; ZP] {
        let g = &self.g;
        let mut z = [0u8; ZP];
        let mut put = |addr: usize, v: u8| z[addr - 0x60] = v;
        put(0x60, g.actor_count);
        put(0x61, g.barrier_x);
        put(0x62, g.barrier_y);
        for (i, &b) in g.barrier.iter().enumerate() {
            put(0x63 + i, b);
        }
        put(0x67, g.ego_auto);
        put(0x68, g.ego_auto2);
        put(0x69, g.target_x);
        put(0x6A, g.target_y);
        put(0x6B, g.slack[0]);
        put(0x6C, g.slack[1]);
        put(0x6D, g.follow_dist);
        put(0x6E, g.follow_state);
        put(0x6F, g.wander_timer);
        put(0x73, g.special);
        put(0x74, g.joy_dir);
        put(0x75, g.key_dir);
        put(0x76, g.patrol_side);
        put(0x9A, g.said[0]);
        put(0x9B, g.said[1]);
        for v in 0..0x20 {
            put(0xD0 + v, self.vars[v]);
        }
        z
    }

    fn load_image(&mut self, img: &[u8]) -> Result<(), String> {
        if img.len() != SAVE_LEN || &img[..8] != MAGIC {
            return Err("not a King's Quest save file".into());
        }
        let mut p = 8;
        let objects = &img[p..p + OBJECTS];
        for (o, rec) in self.objects.iter_mut().enumerate() {
            rec.copy_from_slice(&objects[o * 8..o * 8 + 8]);
        }
        p += OBJECTS;
        for s in 0..MAX_ACTORS {
            self.actors[s].copy_from_slice(&img[p + s * 31..p + s * 31 + 31]);
        }
        p += ACTORS;
        let z = &img[p..p + ZP];
        let get = |addr: usize| z[addr - 0x60];
        let g = &mut self.g;
        g.actor_count = get(0x60);
        g.barrier_x = get(0x61);
        g.barrier_y = get(0x62);
        for i in 0..4 {
            g.barrier[i] = get(0x63 + i);
        }
        g.ego_auto = get(0x67);
        g.ego_auto2 = get(0x68);
        g.target_x = get(0x69);
        g.target_y = get(0x6A);
        g.slack = [get(0x6B), get(0x6C)];
        g.follow_dist = get(0x6D);
        g.follow_state = get(0x6E);
        g.wander_timer = get(0x6F);
        g.special = get(0x73);
        g.joy_dir = get(0x74);
        g.key_dir = get(0x75);
        g.patrol_side = get(0x76);
        g.said = [get(0x9A), get(0x9B)];
        for v in 0..0x20 {
            self.vars[v] = get(0xD0 + v);
        }
        p += ZP;
        self.vars[0x20..VARS].copy_from_slice(&img[p + 0x20..p + VARS]);
        Ok(())
    }

    /// A string from the engine image (the original prompt texts), up to its NUL.
    fn engine_text(&self, addr: u16) -> Vec<u8> {
        let o = (addr - crate::kq::ENGINE_BASE) as usize;
        self.res.engine.get(o..).unwrap_or(&[]).iter().take_while(|&&c| c != 0).map(|&c| c & 0x7F).collect()
    }

    /// $59C7: print a prompt and wait for a letter A-Z (returned 1-26) or Esc (None).
    fn ask_letter(&mut self, prompt: u16) -> Option<u8> {
        let text = self.engine_text(prompt);
        self.put_str(&text);
        loop {
            let k = self.io_wait_key();
            if k == 0x1B {
                self.put_char(0x0D);
                return None;
            }
            let up = if k.is_ascii_lowercase() { k - 0x20 } else { k };
            if up.is_ascii_uppercase() {
                self.put_char(up);
                return Some(up - 0x40);
            }
        }
    }

    /// $4F3C: wait for RETURN (true) or Esc (false).
    fn wait_return(&mut self) -> bool {
        loop {
            match self.io_wait_key() {
                0x0D => return true,
                0x1B => return false,
                _ => {}
            }
        }
    }

    /// $5905 save_game. The disk-swap prompt ($6C08) is not shown: the port saves to a
    /// file. The confirmation uses the first sentence of $6C86.
    pub fn save_game(&mut self) {
        if let Some(slot) = self.ask_letter(0x6B8B) {
            self.persist_actors(); // $491F
            let img = self.save_image();
            match self.io_save(slot, &img) {
                Ok(()) => {
                    let msg = self.engine_text(0x6C86);
                    let first = msg.split(|&c| c == b'.').next().unwrap_or(&[]).to_vec();
                    self.put_str(&first);
                    self.put_str(b".\r");
                }
                Err(e) => {
                    self.io_double_bell(); // $60AE: disk error
                    self.put_str(format!("\r{e}\r").as_bytes());
                }
            }
        }
        self.text.need_prompt = true;
    }

    /// $4F56 restore_game. Returns true if a game was restored.
    pub fn restore_game(&mut self) -> bool {
        self.g.restore_request = 0;
        loop {
            let Some(slot) = self.ask_letter(0x6922) else {
                self.text.need_prompt = true;
                return false;
            };
            self.put_char(0x0D);
            let Some(img) = self.io_load(slot) else {
                // $4FE7: "That game was not saved on this disk."
                let msg = self.engine_text(0x69FE);
                self.put_str(&msg);
                if self.wait_return() {
                    continue;
                }
                self.text.need_prompt = true;
                return false;
            };
            if let Err(e) = self.load_image(&img) {
                self.io_double_bell(); // $60AE: disk error
                self.put_str(format!("{e}\r").as_bytes());
                self.text.need_prompt = true;
                return false;
            }
            // $4F8B-$4FC7: put the actors back into their objects, re-enter the room
            // (keeping v18 and v14), and let the room scripts set it up again.
            self.clear_actors();
            let (v18, v14) = (self.vars[18], self.vars[14]);
            let room = self.vars[1];
            self.new_room_carry(room, true);
            self.vars[18] = v18;
            self.vars[14] = v14;
            self.vars[25] = 0;
            self.g.flag_6f05 = 0;
            self.vars[29] = 1;
            self.text.need_prompt = true;
            for a in self.actors.iter_mut() {
                a[f::DRAWN] = 0;
            }
            return true;
        }
    }

    pub(crate) fn io_save(&mut self, slot: u8, data: &[u8]) -> Result<(), String> {
        match self.io.as_mut() {
            Some(io) => io.save(slot, data),
            None => Err("no save storage".into()),
        }
    }

    pub(crate) fn io_load(&mut self, slot: u8) -> Option<Vec<u8>> {
        self.io.as_mut()?.load(slot)
    }
}
