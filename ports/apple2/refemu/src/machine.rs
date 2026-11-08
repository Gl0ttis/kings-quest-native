//! A 128K Apple IIe as far as King's Quest needs it, with no Apple ROM: the few firmware
//! routines the game calls are substituted at their entry points (see `trap`).

use crate::cpu::{Bus, Cpu, C, V};
use std::collections::VecDeque;

pub const NIB_TRACK: usize = 6656;

pub struct Disk {
    pub tracks: Vec<Vec<u8>>, // 35 raw nibble tracks
}

impl Disk {
    pub fn from_nib(raw: &[u8]) -> Self {
        Disk { tracks: raw.chunks(NIB_TRACK).map(|t| t.to_vec()).collect() }
    }
    pub fn to_nib(&self) -> Vec<u8> {
        self.tracks.concat()
    }
}

#[derive(Default)]
pub struct Switches {
    pub store80: bool,
    pub ramrd: bool,
    pub ramwrt: bool,
    pub altzp: bool,
    pub col80: bool,
    pub text: bool,
    pub mixed: bool,
    pub page2: bool,
    pub hires: bool,
    pub an3: bool,
    // Language card
    pub lc_read_ram: bool,
    pub lc_bank1: bool,
    pub lc_write: bool,
    pub lc_prewrite: bool,
}

pub struct Machine {
    pub main: Vec<u8>,
    pub aux: Vec<u8>,
    /// Language card RAM: [main/aux][bank1 $D000, bank2 $D000, $E000-$FFFF].
    pub lc: [[Vec<u8>; 3]; 2],
    pub sw: Switches,
    pub cycles: u64,
    // Keyboard
    pub key: u8,
    pub key_ready: bool,
    /// Keyboard reads ($C000) so far: a key-wait loop polls constantly.
    pub key_polls: u64,
    pub key_queue: VecDeque<u8>,
    pub next_key_at: u64,
    // Paddles
    pub paddle_until: u64,
    pub speaker_clicks: u64,
    pub bells: u64,
    // Disk II, slot 6
    pub disks: Vec<Disk>,
    pub cur_disk: usize,
    pub halftrack: i32,
    pub phases: u8,
    pub last_phase: i32,
    pub motor: bool,
    pub q6: bool,
    pub q7: bool,
    pub write_latch: u8,
    /// Which disks have been written to.
    pub disk_written: Vec<bool>,
    pub last_nibble: u64,
    /// Text page written by the COUT substitute (for logs).
    pub text_log: String,
    pub notes: Vec<String>,
}

impl Machine {
    pub fn new(disks: Vec<Disk>) -> Self {
        let lcb = || [vec![0u8; 0x1000], vec![0u8; 0x1000], vec![0u8; 0x2000]];
        Machine {
            main: vec![0; 0x10000],
            aux: vec![0; 0x10000],
            lc: [lcb(), lcb()],
            sw: Switches { text: true, ..Default::default() },
            cycles: 0,
            key: 0,
            key_ready: false,
            key_polls: 0,
            key_queue: VecDeque::new(),
            next_key_at: 0,
            paddle_until: 0,
            speaker_clicks: 0,
            bells: 0,
            disks,
            cur_disk: 0,
            halftrack: 0,
            phases: 0,
            last_phase: 0,
            motor: false,
            q6: false,
            q7: false,
            write_latch: 0,
            disk_written: vec![false; 8],
            last_nibble: u64::MAX,
            text_log: String::new(),
            notes: Vec::new(),
        }
    }

    /// Which 64K bank a main-memory address ($0200-$BFFF) uses, for read or write.
    fn bank_aux(&self, addr: u16, write: bool) -> bool {
        let s = &self.sw;
        if s.store80 && ((0x0400..0x0800).contains(&addr) || (s.hires && (0x2000..0x4000).contains(&addr))) {
            return s.page2;
        }
        if write {
            s.ramwrt
        } else {
            s.ramrd
        }
    }

    fn lc_slot(&self, addr: u16) -> (usize, usize, usize) {
        let which = self.sw.altzp as usize;
        if addr < 0xE000 {
            (which, if self.sw.lc_bank1 { 0 } else { 1 }, (addr - 0xD000) as usize)
        } else {
            (which, 2, (addr - 0xE000) as usize)
        }
    }

    pub fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x01FF => {
                if self.sw.altzp {
                    self.aux[addr as usize]
                } else {
                    self.main[addr as usize]
                }
            }
            0x0200..=0xBFFF => {
                if self.bank_aux(addr, false) {
                    self.aux[addr as usize]
                } else {
                    self.main[addr as usize]
                }
            }
            0xD000..=0xFFFF if self.sw.lc_read_ram => {
                let (w, b, o) = self.lc_slot(addr);
                self.lc[w][b][o]
            }
            _ => 0,
        }
    }

    /// Main memory as the engine's own code sees it (no switches).
    pub fn main_at(&self, addr: u16) -> u8 {
        self.main[addr as usize]
    }

    fn io_read(&mut self, addr: u16) -> u8 {
        match addr {
            0xC000..=0xC00F => {
                self.key_polls += 1;
                if !self.key_ready && self.cycles >= self.next_key_at {
                    if let Some(k) = self.key_queue.pop_front() {
                        self.key = k;
                        self.key_ready = true;
                    }
                }
                self.key | if self.key_ready { 0x80 } else { 0 }
            }
            0xC010 => {
                self.key_ready = false;
                // Leave a gap before the next injected key, like a typist would.
                self.next_key_at = self.cycles + 30_000;
                self.key
            }
            0xC013 => (self.sw.ramrd as u8) << 7,
            0xC014 => (self.sw.ramwrt as u8) << 7,
            0xC016 => (self.sw.altzp as u8) << 7,
            0xC018 => (self.sw.store80 as u8) << 7,
            0xC030 => {
                self.speaker_clicks += 1;
                0
            }
            0xC050..=0xC05F => {
                self.soft(addr);
                0
            }
            0xC064..=0xC067 => {
                // Paddles read as centred.
                if self.cycles < self.paddle_until {
                    0x80
                } else {
                    0
                }
            }
            0xC061..=0xC063 => 0, // buttons up
            0xC070 => {
                self.paddle_until = self.cycles + 128 * 11;
                0
            }
            0xC080..=0xC08F => {
                self.lc_switch(addr, false);
                0
            }
            0xC0E0..=0xC0EF => self.disk_io(addr, None),
            _ => 0,
        }
    }

    fn io_write(&mut self, addr: u16, v: u8) {
        match addr {
            0xC000 => self.sw.store80 = false,
            0xC001 => self.sw.store80 = true,
            0xC002 => self.sw.ramrd = false,
            0xC003 => self.sw.ramrd = true,
            0xC004 => self.sw.ramwrt = false,
            0xC005 => self.sw.ramwrt = true,
            0xC008 => self.sw.altzp = false,
            0xC009 => self.sw.altzp = true,
            0xC00C => self.sw.col80 = false,
            0xC00D => self.sw.col80 = true,
            0xC010 => {
                self.key_ready = false;
                self.next_key_at = self.cycles + 30_000;
            }
            0xC030 => self.speaker_clicks += 1,
            0xC050..=0xC05F => self.soft(addr),
            0xC070 => self.paddle_until = self.cycles + 128 * 11,
            0xC080..=0xC08F => self.lc_switch(addr, true),
            0xC0E0..=0xC0EF => {
                self.disk_io(addr, Some(v));
            }
            _ => {}
        }
    }

    fn soft(&mut self, addr: u16) {
        let s = &mut self.sw;
        match addr {
            0xC050 => s.text = false,
            0xC051 => s.text = true,
            0xC052 => s.mixed = false,
            0xC053 => s.mixed = true,
            0xC054 => s.page2 = false,
            0xC055 => s.page2 = true,
            0xC056 => s.hires = false,
            0xC057 => s.hires = true,
            0xC05E => s.an3 = false,
            0xC05F => s.an3 = true,
            _ => {}
        }
    }

    /// $C080-$C08F language card control.
    fn lc_switch(&mut self, addr: u16, write: bool) {
        let s = &mut self.sw;
        let a = addr & 0x0F;
        s.lc_bank1 = a & 8 != 0;
        s.lc_read_ram = matches!(a & 3, 0 | 3);
        if a & 1 != 0 {
            if s.lc_prewrite && !write {
                s.lc_write = true;
            }
            s.lc_prewrite = !write;
        } else {
            s.lc_write = false;
            s.lc_prewrite = false;
        }
    }

    /// Disk II controller in slot 6. Reads return nibbles from the current track at one
    /// per 32 cycles; the latch reads as "not ready" until the next nibble arrives.
    fn disk_io(&mut self, addr: u16, write: Option<u8>) -> u8 {
        let a = (addr & 0x0F) as u8;
        match a {
            0..=7 => {
                let phase = (a >> 1) as i32;
                if a & 1 != 0 {
                    self.phases |= 1 << phase;
                    let diff = (phase - self.last_phase).rem_euclid(4);
                    if diff == 1 {
                        self.halftrack = (self.halftrack + 1).min(69);
                    } else if diff == 3 {
                        self.halftrack = (self.halftrack - 1).max(0);
                    }
                    self.last_phase = phase;
                } else {
                    self.phases &= !(1 << phase);
                }
            }
            8 => self.motor = false,
            9 => {
                self.motor = true;
                self.choose_disk();
            }
            0xA | 0xB => {}
            0xC => self.q6 = false,
            0xD => self.q6 = true,
            0xE => self.q7 = false,
            0xF => self.q7 = true,
            _ => {}
        }
        // Writing (Q7 on): a write with Q6 on loads the data latch (STA $C08D,X); an
        // access to $C08C shifts it out (ORA $C08C,X), one nibble per 32 cycles. The
        // nibble goes at the disk's current position.
        if self.q7 && self.q6 {
            if let Some(v) = write {
                self.write_latch = v;
            }
        }
        if a == 0xC && self.q7 {
            let track = (self.halftrack / 2) as usize;
            let n = self.cycles / 32;
            let latch = self.write_latch;
            if let Some(t) = self.disks.get_mut(self.cur_disk).and_then(|d| d.tracks.get_mut(track)) {
                let len = t.len() as u64;
                t[(n % len) as usize] = latch;
                self.disk_written[self.cur_disk] = true;
            }
            return 0;
        }
        if a == 0xC && !self.q7 {
            let track = (self.halftrack / 2) as usize;
            let t = match self.disks.get(self.cur_disk).and_then(|d| d.tracks.get(track)) {
                Some(t) => t,
                None => return 0,
            };
            let n = self.cycles / 32;
            let b = t[(n % t.len() as u64) as usize];
            if n != self.last_nibble {
                self.last_nibble = n;
                return b | 0x80;
            }
            return b & 0x7F;
        }
        if a == 0xE && self.q6 {
            return 0; // write-protect sense: not protected
        }
        0
    }

    /// Insert the disk the game expects. KQ puts the wanted disk number in the RWTS
    /// volume field ($6F33); before the engine is loaded that is 0, meaning disk 1.
    fn choose_disk(&mut self) {
        let mut want = self.main[0x6F33];
        // The save disk is volume 10; it is the fourth image, if one was given.
        if want == 10 && self.disks.len() >= 4 {
            want = 4;
        }
        if (1..=self.disks.len() as u8).contains(&want) {
            self.cur_disk = want as usize - 1;
        }
    }

    fn put_text_char(&mut self, c: u8) {
        let ch = self.main[0x24];
        let base = self.main[0x28] as u16 | (self.main[0x29] as u16) << 8;
        if c & 0x7F == 0x0D {
            self.text_log.push('\n');
            self.main[0x24] = 0;
            let cv = self.main[0x25].saturating_add(1).min(23);
            self.main[0x25] = cv;
            self.bascalc(cv);
            return;
        }
        self.text_log.push((c & 0x7F) as char);
        let addr = base.wrapping_add(ch as u16);
        if (0x0400..0x0800).contains(&addr) {
            self.main[addr as usize] = c;
        }
        self.main[0x24] = ch.wrapping_add(1);
    }

    /// $FBC1 BASCALC: text line base address for row A into $28/$29.
    fn bascalc(&mut self, row: u8) {
        let r = row as u16;
        let addr = 0x0400 + (r & 7) * 0x80 + (r >> 3) * 0x28;
        self.main[0x28] = addr as u8;
        self.main[0x29] = (addr >> 8) as u8;
    }
}

impl Bus for Machine {
    fn read(&mut self, addr: u16) -> u8 {
        if (0xC000..0xC100).contains(&addr) {
            return self.io_read(addr);
        }
        self.peek(addr)
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x01FF => {
                if self.sw.altzp {
                    self.aux[addr as usize] = v
                } else {
                    self.main[addr as usize] = v
                }
            }
            0x0200..=0xBFFF => {
                if self.bank_aux(addr, true) {
                    self.aux[addr as usize] = v
                } else {
                    self.main[addr as usize] = v
                }
            }
            0xC000..=0xC0FF => self.io_write(addr, v),
            0xD000..=0xFFFF if self.sw.lc_write => {
                let (w, b, o) = self.lc_slot(addr);
                self.lc[w][b][o] = v;
            }
            _ => {}
        }
    }

    /// Firmware substitutes. They act only where the Apple ROM would be visible: the
    /// $C1xx-$CFxx space, or $D000-$FFFF while the language card reads ROM.
    fn trap(&mut self, cpu: &mut Cpu) -> bool {
        self.cycles = cpu.cycles;
        let pc = cpu.pc;
        let rom = pc >= 0xD000 && !self.sw.lc_read_ram;
        if !(rom || (0xC100..0xD000).contains(&pc) || (0xC000..0xC100).contains(&pc)) {
            return false;
        }
        match pc {
            // $C314 XFER: C selects aux RAMRD/RAMWRT, V selects the alternate zero page;
            // continue at the address in $03ED/$03EE.
            0xC314 => {
                // The target is read before switching, from whichever bank is mapped.
                let target = self.peek(0x3ED) as u16 | (self.peek(0x3EE) as u16) << 8;
                let aux = cpu.flag(C);
                self.sw.ramrd = aux;
                self.sw.ramwrt = aux;
                self.sw.altzp = cpu.flag(V);
                cpu.pc = target;
                cpu.cycles += 20;
            }
            // Disk II boot ROM sector read ($C65C): read physical sector $3D of track 0
            // into page $27, advance $27, continue at $0801.
            0xC65C => {
                let sector = self.main[0x3D] as usize;
                let page = self.main[0x27] as usize;
                match crate::boot_sector(&self.disks[0], sector) {
                    Some(data) => self.main[page * 256..page * 256 + 256].copy_from_slice(&data),
                    None => self.notes.push(format!("boot: sector {sector} unreadable")),
                }
                self.main[0x27] = self.main[0x27].wrapping_add(1);
                cpu.pc = 0x0801;
                cpu.x = 0x60;
                cpu.cycles += 25_000;
            }
            0xFB2F => {
                // INIT: text mode, full window.
                self.sw.text = true;
                self.main[0x20] = 0;
                self.main[0x21] = 40;
                self.main[0x22] = 0;
                self.main[0x23] = 24;
                cpu.rts(self);
            }
            0xFE84 | 0xFE89 | 0xFE93 => {
                if pc == 0xFE93 {
                    self.main[0x36] = 0xF0;
                    self.main[0x37] = 0xFD;
                }
                cpu.rts(self);
            }
            0xFC58 => {
                // HOME
                for a in 0x0400..0x0800 {
                    self.main[a] = 0xA0;
                }
                self.main[0x24] = 0;
                self.main[0x25] = 0;
                self.bascalc(0);
                cpu.rts(self);
            }
            0xFBC1 => {
                self.bascalc(cpu.a);
                cpu.rts(self);
            }
            0xFDED => {
                // COUT through the CSW vector.
                cpu.pc = self.main[0x36] as u16 | (self.main[0x37] as u16) << 8;
            }
            0xFDF0 => {
                self.put_text_char(cpu.a);
                cpu.cycles += 40;
                cpu.rts(self);
            }
            0xFDDA => {
                let s = format!("{:02X}", cpu.a);
                for c in s.bytes() {
                    self.put_text_char(c | 0x80);
                }
                cpu.rts(self);
            }
            0xFBDD => {
                // BELL: about 116,000 cycles of speaker toggling in the real ROM.
                self.bells += 1;
                self.speaker_clicks += 192;
                cpu.cycles += 116_000;
                cpu.rts(self);
            }
            _ => {
                self.notes.push(format!("unhandled firmware call at ${pc:04X} (from ROM space)"));
                cpu.bad_opcode = Some((pc, 0));
            }
        }
        true
    }
}
