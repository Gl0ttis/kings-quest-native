//! An IBM PC with the Color/Graphics Adapter, for measuring the original: 256 KB of RAM,
//! the CGA's 16 KB at B800 and its registers, the 8253 timer (channel 0's 18.2 Hz tick,
//! channel 2 and port 61h for the speaker), and the disk. No IBM ROM: the BIOS services
//! the game calls are answered here, natively (see `service`). Every interrupt vector
//! starts at a stub in segment F000; running into a stub calls the matching service.
//!
//! The engine is started as the loader starts it ([`Machine::load_engine`]): the boot
//! sector, `LOAD` and the copy protection aren't run (README.md, "Start-up").

use crate::cpu::{self, Bus, Cpu, AX, BX, CF, CS, CX, DX, ES, IF, SP, SS, ZF};
use std::collections::{HashMap, VecDeque};

/// CPU clocks per BIOS timer tick: the timer runs at a quarter of the 4.77 MHz CPU clock
/// and counts 65,536 per tick.
pub const CYCLES_PER_TICK: u64 = 4 * 65_536;
pub const CPU_HZ: f64 = 14_318_180.0 / 3.0;

/// Where the vector stubs are: F000:0100 + n.
const STUB_SEG: u16 = 0xF000;
const STUB_BASE: u16 = 0x0100;
/// A standard 360K diskette-parameter table, at F000:0010.
const PARAM_TABLE: u16 = 0x0010;
/// The RAM: 256 KB.
pub const RAM_KB: u16 = 256;
/// The CGA's memory: 16 KB at B8000, seen again at BC000.
const CGA: usize = 0xB8000;

/// A disk as the controller sees it: sectors by (cylinder, head, record, size code).
#[derive(Default, Clone)]
pub struct Disk {
    pub sectors: HashMap<(u8, u8, u8, u8), Vec<u8>>,
    pub written: bool,
    /// A write-protected disk (the Master): writes and formats fail with status 3.
    pub write_protected: bool,
}

impl Disk {
    pub fn from_tracks(tracks: &[kq1_formats::mfm::Track]) -> Disk {
        let mut d = Disk::default();
        for t in tracks {
            for s in &t.sectors {
                d.sectors.insert((t.cylinder, t.side, s.record, s.size), s.data.clone());
            }
        }
        d
    }

    /// A 360K sector image: 9 sectors of 512 bytes a track.
    pub fn from_image(raw: &[u8]) -> Disk {
        let mut d = Disk::default();
        for (i, s) in raw.chunks(512).enumerate() {
            let (c, h, r) = ((i / 18) as u8, (i / 9 % 2) as u8, (i % 9 + 1) as u8);
            d.sectors.insert((c, h, r, 2), s.to_vec());
        }
        d
    }
}

/// The CGA's registers as the game sets them.
#[derive(Clone, Default, Debug)]
pub struct Cga {
    /// Mode control (3D8h): `1Ah` is 640-dot graphics with the colour burst on.
    pub mode: u8,
    /// Colour select (3D9h): the 640-dot mode's foreground in the low four bits.
    pub colour: u8,
    pub crtc_index: u8,
    pub crtc: [u8; 18],
}

#[derive(Clone)]
pub struct Machine {
    pub ram: Vec<u8>,
    pub cga: Cga,
    /// The BIOS's video mode (INT 10h AH=00h).
    pub video_mode: u8,
    pub pic_mask: u8,
    pub port61: u8,
    /// Speaker events: (cycle, port, byte) for the timer's channel 2 (42h, 43h) and the
    /// gate (61h).
    pub sound: Vec<(u64, u16, u8)>,
    pub disks: Vec<Disk>,
    pub drive: usize,
    pub keys: VecDeque<u16>,
    /// The character in each text cell (25 rows of 40), as the BIOS last wrote it.
    pub text: [[u8; 40]; 25],
    /// The BIOS's 8 × 8 font for characters 0-7Fh (F000:FA6E), from the user's BIOS
    /// image (`set_font`): characters are then drawn as the BIOS draws them; without it
    /// their cells are left blank.
    pub font: Option<Vec<u8>>,
    /// The text row the BIOS last drew a character in. The game clears its message box
    /// by writing memory, which `text` doesn't see, so the current message is the one
    /// round this row.
    pub last_row: u8,
    pub cursor: (u8, u8),
    pub next_tick: u64,
    /// BIOS calls made: (vector, AH) -> count.
    pub calls: HashMap<(u8, u8), u64>,
    /// Ports read or written that nothing here models (writes with bit 15 set).
    pub unknown_ports: HashMap<u16, u64>,
    pub notes: Vec<String>,
    /// Keyboard polls since the last key (INT 16h AH=0/1 with nothing waiting).
    pub idle_polls: u64,
    /// Blocking keyboard reads (INT 16h AH=0) that found nothing typed, since this was
    /// reset: the game is waiting at a prompt.
    pub blocked_reads: u64,
    /// The drive: the cylinder its head is on, and when it was last used (its motor
    /// stops 2 s after).
    head: u8,
    last_disk: Option<u64>,
    /// Disk time so far, in cycles.
    pub disk_cycles: u64,
    /// Clocks charged per instruction byte fetched, in quarters of a clock (README.md,
    /// "Timing"; 6: 1.5 clocks, fitted to 86Box).
    pub fetch_quarters: u64,
    fetch_rest: u64,
    /// Wait states per access to the CGA's memory (4, fitted to 86Box).
    pub cga_wait: u64,
    cga_accesses: u64,
    pub sectors_read: u64,
    /// Checkpoints passed so far (the port's clock's), counted by the runner.
    pub checkpoint: u64,
    /// The event log: ticks, keys and disk swaps against the checkpoint count, for
    /// replaying in the port (refemu-pcjr's format).
    pub events: Option<String>,
}

impl Machine {
    pub fn new(game: Disk) -> Machine {
        let mut m = Machine {
            ram: vec![0; 0x100000],
            cga: Cga::default(),
            video_mode: 0,
            pic_mask: 0,
            port61: 0,
            sound: Vec::new(),
            disks: vec![game],
            drive: 0,
            keys: VecDeque::new(),
            text: [[b' '; 40]; 25],
            last_row: 0,
            font: None,
            cursor: (0, 0),
            next_tick: CYCLES_PER_TICK,
            calls: HashMap::new(),
            unknown_ports: HashMap::new(),
            notes: Vec::new(),
            idle_polls: 0,
            blocked_reads: 0,
            head: 0,
            last_disk: None,
            disk_cycles: 0,
            fetch_quarters: 6,
            fetch_rest: 0,
            cga_wait: 4,
            cga_accesses: 0,
            sectors_read: 0,
            checkpoint: 0,
            events: None,
        };
        // Interrupt vectors -> stubs; the diskette parameters; the BIOS data area.
        for n in 0..256u32 {
            let off = STUB_BASE + n as u16;
            m.w16(n * 4, off);
            m.w16(n * 4 + 2, STUB_SEG);
        }
        let p = cpu::lin(STUB_SEG, PARAM_TABLE) as usize;
        m.ram[p..p + 11].copy_from_slice(&[0xDF, 0x02, 0x25, 0x02, 0x09, 0x2A, 0xFF, 0x50, 0xF6, 0x0F, 0x02]);
        m.set_vector(0x1E, STUB_SEG, PARAM_TABLE);
        m.w16(0x410, 0x0021); // equipment: diskette, 80x25 colour
        m.w16(0x413, RAM_KB);
        m
    }

    /// A key into the BIOS keyboard buffer, logged.
    pub fn push_key(&mut self, k: u16) {
        if let Some(e) = self.events.as_mut() {
            e.push_str(&format!(
                "K {} {k:04X}
",
                self.checkpoint
            ));
        }
        self.keys.push_back(k);
    }

    /// Disk `n` (0: the Master; 1: the Play diskette) into the drive, logged.
    pub fn insert(&mut self, n: usize) {
        if let Some(e) = self.events.as_mut() {
            e.push_str(&format!(
                "D {} {n}
",
                self.checkpoint
            ));
        }
        while self.disks.len() <= n {
            self.disks.push(Disk::default());
        }
        self.drive = n;
    }

    fn w16(&mut self, a: u32, v: u16) {
        self.ram[a as usize] = v as u8;
        self.ram[a as usize + 1] = (v >> 8) as u8;
    }
    fn r16(&self, a: u32) -> u16 {
        self.ram[a as usize] as u16 | (self.ram[a as usize + 1] as u16) << 8
    }
    fn set_vector(&mut self, n: u8, seg: u16, off: u16) {
        self.w16(n as u32 * 4, off);
        self.w16(n as u32 * 4 + 2, seg);
    }
    pub fn vector(&self, n: u8) -> (u16, u16) {
        (self.r16(n as u32 * 4 + 2), self.r16(n as u32 * 4))
    }

    /// Start the engine as the loader does: `MAIN.EXE`'s program at segment `0080`
    /// (the file, header and all, at `0060`), its relocations applied, run from CS:IP
    /// `0080:0000` with its header's stack. (The original's header starts it at the
    /// protection's code in its last 773 bytes instead, which reads the disk through
    /// IBM's ROM and then jumps to the same place; the crack's starts it there.)
    pub fn load_engine(&mut self, cpu: &mut Cpu, exe: &[u8]) -> Result<(), String> {
        let u16_at = |o: usize| u16::from_le_bytes([exe[o], exe[o + 1]]);
        if exe.len() < 0x20 || &exe[..2] != b"MZ" {
            return Err("MAIN.EXE isn't a program".into());
        }
        const SEG: u16 = 0x0080;
        let header = u16_at(8) as usize * 16;
        let image = &exe[header..];
        let base = cpu::lin(SEG, 0) as usize;
        self.ram[base..base + image.len()].copy_from_slice(image);
        let (relocs, table) = (u16_at(6) as usize, u16_at(0x18) as usize);
        for k in 0..relocs {
            let (off, seg) = (u16_at(table + 4 * k), u16_at(table + 4 * k + 2));
            let a = cpu::lin(SEG.wrapping_add(seg), off);
            let v = self.r16(a).wrapping_add(SEG);
            self.w16(a, v);
        }
        cpu.s = [SEG - 0x10, SEG, SEG.wrapping_add(u16_at(0xE)), SEG - 0x10];
        cpu.ip = 0;
        cpu.r[SP] = u16_at(0x10);
        cpu.flags |= IF;
        Ok(())
    }

    /// One instruction, a BIOS service if the CPU has reached a stub, and the timer.
    pub fn step(&mut self, cpu: &mut Cpu) {
        if cpu.s[CS] == STUB_SEG && (STUB_BASE..STUB_BASE + 256).contains(&cpu.ip) {
            let n = (cpu.ip - STUB_BASE) as u8;
            self.service(cpu, n);
        } else if cpu.halted {
            cpu.cycles = cpu.cycles.max(self.next_tick);
            cpu.halted = false;
        } else {
            self.cga_accesses = 0;
            let fetched = cpu.fetched;
            cpu.step(self);
            // A charge for each instruction byte fetched, which Intel's counts leave out,
            // and the CGA's wait states (README.md, "Timing").
            let q = (cpu.fetched - fetched) * self.fetch_quarters + self.fetch_rest;
            self.fetch_rest = q % 4;
            cpu.cycles += q / 4 + self.cga_accesses * self.cga_wait;
        }
        if cpu.cycles >= self.next_tick {
            if cpu.flags & IF != 0 && self.pic_mask & 1 == 0 {
                self.next_tick += CYCLES_PER_TICK;
                if let Some(e) = self.events.as_mut() {
                    let ip = if cpu.s[CS] == 0x60 { cpu.ip } else { 0xFFFF };
                    e.push_str(&format!(
                        "T {} {ip:04X}
",
                        self.checkpoint
                    ));
                }
                cpu.interrupt(self, 8);
            } else if cpu.cycles > self.next_tick + CYCLES_PER_TICK {
                // Interrupts held off for a whole tick: the tick is lost, as on hardware.
                self.next_tick += CYCLES_PER_TICK;
            }
        }
    }

    /// Return from a service: IRET, with CF and ZF set in the flags it restores.
    fn ret(&mut self, cpu: &mut Cpu, cf: Option<bool>, zf: Option<bool>) {
        let at = cpu::lin(cpu.s[SS], cpu.r[SP].wrapping_add(4));
        let mut f = self.r16(at);
        if let Some(c) = cf {
            f = if c { f | CF } else { f & !CF };
        }
        if let Some(z) = zf {
            f = if z { f | ZF } else { f & !ZF };
        }
        self.w16(at, f);
        cpu.iret(self);
    }

    fn service(&mut self, cpu: &mut Cpu, n: u8) {
        let ah = (cpu.r[AX] >> 8) as u8;
        *self.calls.entry((n, ah)).or_default() += 1;
        // Rough times for the ROM routines this replaces (estimates, to calibrate).
        cpu.cycles += match (n, ah) {
            (0x08, _) => 250,
            (0x16, _) => 150,
            (0x10, 0x09) | (0x10, 0x0E) => 2_500,
            (0x10, 0x06) => 40_000,
            (0x10, _) => 300,
            _ => 200,
        };
        match n {
            0x08 => {
                // Timer: count the tick, then INT 1Ch (which IRETs to the interrupted code).
                let t = (self.r16(0x46C) as u32 | (self.r16(0x46E) as u32) << 16).wrapping_add(1);
                self.w16(0x46C, t as u16);
                self.w16(0x46E, (t >> 16) as u16);
                let (seg, off) = self.vector(0x1C);
                if seg == STUB_SEG {
                    cpu.iret(self);
                } else {
                    cpu.s[CS] = seg;
                    cpu.ip = off;
                    cpu.cycles += 60;
                }
            }
            0x10 => self.video(cpu),
            0x11 => {
                cpu.r[AX] = self.r16(0x410);
                self.ret(cpu, None, None);
            }
            0x12 => {
                cpu.r[AX] = self.r16(0x413);
                self.ret(cpu, None, None);
            }
            0x13 => self.disk(cpu),
            0x16 => self.keyboard(cpu),
            0x1A => {
                let t = self.r16(0x46C) as u32 | (self.r16(0x46E) as u32) << 16;
                cpu.r[CX] = (t >> 16) as u16;
                cpu.r[DX] = t as u16;
                cpu.r[AX] &= 0xFF00;
                self.ret(cpu, None, None);
            }
            0x1C | 0x09 | 0x1B | 0x23 | 0x24 => cpu.iret(self),
            _ => cpu.fault = Some(format!("unhandled BIOS call INT {n:02X}h AH={ah:02X}h")),
        }
    }

    // ---- INT 10h
    fn video(&mut self, cpu: &mut Cpu) {
        let (ah, al) = ((cpu.r[AX] >> 8) as u8, cpu.r[AX] as u8);
        let (bh, bl) = ((cpu.r[BX] >> 8) as u8, cpu.r[BX] as u8);
        let (dh, dl) = ((cpu.r[DX] >> 8) as u8, cpu.r[DX] as u8);
        match ah {
            0 => {
                self.video_mode = al & 0x7F;
                self.ram[0x449] = self.video_mode;
                // The mode register as IBM's BIOS sets it for the mode (its table at
                // F000:F0E4), and the colour register to 0 (30h in 320-dot modes).
                self.cga.mode = [0x2C, 0x28, 0x2D, 0x29, 0x2A, 0x2E, 0x1E, 0x29][self.video_mode as usize & 7];
                self.cga.colour = if self.video_mode == 6 { 0x3F } else { 0x30 };
                if al & 0x80 == 0 {
                    self.ram[CGA..CGA + 0x4000].fill(0);
                }
                self.text = [[b' '; 40]; 25];
                self.cursor = (0, 0);
            }
            1 => {}
            2 => self.cursor = (dh, dl),
            3 => {
                cpu.r[DX] = (self.cursor.0 as u16) << 8 | self.cursor.1 as u16;
                cpu.r[CX] = 0x0607;
            }
            // The shown page, in the text modes (the game's Play-diskette screens): the
            // characters are kept as text either way.
            5 => self.ram[0x462] = al,
            6 => {
                let (ch, cl) = ((cpu.r[CX] >> 8) as u8, cpu.r[CX] as u8);
                self.scroll(al, bh, (ch, cl), (dh, dl));
            }
            9 => {
                let n = cpu.r[CX];
                let (r, c) = self.cursor;
                for k in 0..n {
                    self.draw_char(r, c.wrapping_add(k as u8), al, bl);
                }
            }
            0x0B => self.cga.colour = bl & 0x3F,
            0x0E => self.teletype(al, bl),
            0x0F => cpu.r[AX] = 40 << 8 | self.video_mode as u16,
            _ => cpu.fault = Some(format!("unhandled INT 10h AH={ah:02X}h")),
        }
        self.ret(cpu, None, None);
    }

    /// A character cell's scan line in the CGA's memory: mode 4's layout (the BIOS's
    /// mode; the game shows it in 640 dots), 2 bytes per 8-pixel cell, 80 bytes a scan
    /// line, even lines from 0 and odd from 2000h.
    fn cell_row(&self, row: u8, col: u8, line: u8) -> Option<usize> {
        if row >= 25 || col >= 40 || !matches!(self.video_mode, 4 | 5) {
            return None;
        }
        let y = row as usize * 8 + line as usize;
        Some(CGA + (y & 1) * 0x2000 + (y >> 1) * 80 + col as usize * 2)
    }

    /// The font from a BIOS image: the IBM PC's 8 KB ROM (F000:E000-FFFF) or a 64 KB
    /// one (F000:0000-FFFF), the font at F000:FA6E; or a dump of the font itself (1,024
    /// bytes, or 2,048 with characters 80h-FFh).
    pub fn set_font(&mut self, rom: &[u8]) -> Result<(), String> {
        let at = match rom.len() {
            0x2000 => 0x1A6E,
            0x10000 => 0xFA6E,
            0x400 | 0x800 => 0,
            n => return Err(format!("not a BIOS image or a font dump ({n} bytes)")),
        };
        self.font = Some(rom[at..at + 0x400].to_vec());
        Ok(())
    }

    /// Draw a character in graphics mode, as the BIOS does in mode 4: each row of the
    /// glyph 8 pixels of 2 bits, the attribute's colour (its low 2 bits) where the glyph
    /// has a dot and 0 elsewhere, written over the cell; with attribute bit 7, XORed onto
    /// it instead. The character is recorded in `text` too. Without the font, the cell
    /// is cleared (left as it is for XOR).
    fn draw_char(&mut self, row: u8, col: u8, ch: u8, attr: u8) {
        if row < 25 && col < 40 {
            self.text[row as usize][col as usize] = ch;
            self.last_row = row;
        }
        let xor = attr & 0x80 != 0;
        for line in 0..8 {
            let Some(a) = self.cell_row(row, col, line) else { continue };
            let glyph = match &self.font {
                Some(f) if ch < 0x80 => f[ch as usize * 8 + line as usize],
                _ => 0,
            };
            let [hi, lo] = expand(glyph, attr & 3).to_be_bytes();
            if xor {
                self.ram[a] ^= hi;
                self.ram[a + 1] ^= lo;
            } else {
                self.ram[a] = hi;
                self.ram[a + 1] = lo;
            }
        }
    }

    fn teletype(&mut self, ch: u8, attr: u8) {
        let (mut r, mut c) = self.cursor;
        match ch {
            0x0D => c = 0,
            0x0A => r += 1,
            0x08 => c = c.saturating_sub(1),
            0x07 => self.notes.push("BIOS bell".into()),
            _ => {
                self.draw_char(r, c, ch, attr);
                c += 1;
                if c >= 40 {
                    c = 0;
                    r += 1;
                }
            }
        }
        if r >= 25 {
            self.scroll(1, 0, (0, 0), (24, 39));
            r = 24;
        }
        self.cursor = (r, c);
    }

    /// INT 10h AH=6: scroll a window up `lines` (0: clear it), the new rows in colour
    /// `fill` (BH: in mode 4 its low 2 bits, in every pixel).
    fn scroll(&mut self, lines: u8, fill: u8, (top, left): (u8, u8), (bottom, right): (u8, u8)) {
        let (bottom, right) = (bottom.min(24), right.min(39));
        if top > bottom || left > right {
            return;
        }
        let height = bottom - top + 1;
        let lines = if lines == 0 || lines > height { height } else { lines };
        for r in top..=bottom {
            let src = r + lines;
            for c in left..=right {
                for line in 0..8 {
                    let Some(d) = self.cell_row(r, c, line) else { continue };
                    if src <= bottom {
                        let s = self.cell_row(src, c, line).unwrap();
                        let tmp = [self.ram[s], self.ram[s + 1]];
                        self.ram[d..d + 2].copy_from_slice(&tmp);
                    } else {
                        self.ram[d..d + 2].fill((fill & 3) * 0x55);
                    }
                }
                self.text[r as usize][c as usize] =
                    if src <= bottom { self.text[src as usize][c as usize] } else { b' ' };
            }
        }
    }

    // ---- INT 13h
    fn disk(&mut self, cpu: &mut Cpu) {
        let (ah, al) = ((cpu.r[AX] >> 8) as u8, cpu.r[AX] as u8);
        let (ch, cl) = ((cpu.r[CX] >> 8) as u8, cpu.r[CX] as u8);
        let dh = (cpu.r[DX] >> 8) as u8;
        let (pseg, poff) = self.vector(0x1E);
        let pt = cpu::lin(pseg, poff) as usize;
        let (size, eot) = (self.ram[pt + 3], self.ram[pt + 4]);
        let len = 128usize << size;
        let es = cpu.s[ES];
        let mut bx = cpu.r[BX];
        let mut disk = std::mem::take(&mut self.disks[self.drive]);
        let protected = disk.write_protected && matches!(ah, 3 | 5);
        let (status, done) = match ah {
            0 => (0, 0),
            _ if protected => (3, 0),
            2 | 3 => {
                let (mut done, mut status) = (0u8, 0u8);
                for k in 0..al {
                    let rec = cl.wrapping_add(k);
                    if rec > eot {
                        break;
                    }
                    let key = (ch, dh, rec, size);
                    if ah == 2 {
                        let Some(data) = disk.sectors.get(&key) else {
                            status = 4;
                            break;
                        };
                        for (i, &v) in data.iter().enumerate() {
                            self.ram[cpu::lin(es, bx.wrapping_add(i as u16)) as usize] = v;
                        }
                    } else {
                        if !disk.sectors.contains_key(&key) {
                            status = 4;
                            break;
                        }
                        let data: Vec<u8> =
                            (0..len).map(|i| self.ram[cpu::lin(es, bx.wrapping_add(i as u16)) as usize]).collect();
                        disk.sectors.insert(key, data);
                        disk.written = true;
                    }
                    bx = bx.wrapping_add(len as u16);
                    done += 1;
                }
                let t = self.drive_time(cpu.cycles, ch, cl, done.max(1));
                cpu.cycles += t;
                self.disk_cycles += t;
                self.sectors_read += done as u64;
                (status, done)
            }
            5 => {
                // Format: ES:BX lists (C, H, R, N) for each sector of the track.
                let n = self.ram[pt + 4];
                disk.sectors.retain(|k, _| !(k.0 == ch && k.1 == dh));
                for k in 0..n as u16 {
                    let a = |i: u16| self.ram[cpu::lin(es, bx.wrapping_add(4 * k + i)) as usize];
                    let (c, h, r, s) = (a(0), a(1), a(2), a(3));
                    disk.sectors.insert((c, h, r, s), vec![0xF6; 128 << (s & 7)]);
                }
                disk.written = true;
                (0, al)
            }
            _ => {
                cpu.fault = Some(format!("unhandled INT 13h AH={ah:02X}h"));
                (1, 0)
            }
        };
        self.disks[self.drive] = disk;
        cpu.r[AX] = (status as u16) << 8 | done as u16;
        self.ram[0x441] = status;
        self.ret(cpu, Some(status != 0), None);
    }

    /// How long the drive takes to read or write `n` sectors from record `first` on
    /// cylinder `cyl`, starting at cycle `now`: the motor's start if it had stopped,
    /// the head's steps and settling, the wait for the first sector to come round, and
    /// the sectors passing the head. A 5.25-inch drive at 300 rpm, 250 kbit/s (6,250
    /// bytes a turn); the step and settle times are the game's own disk table's ($7B: 6
    /// ms a track, 15 ms), the motor's start the BIOS's (250 ms, after 2 s unused).
    fn drive_time(&mut self, now: u64, cyl: u8, first: u8, n: u8) -> u64 {
        let ms = |t: f64| (t * CPU_HZ / 1000.0) as u64;
        let turn = ms(200.0);
        let byte = turn as f64 / 6250.0;
        let mut t = 0;
        if self.last_disk.is_none_or(|l| now.saturating_sub(l) > ms(2000.0)) {
            t += ms(250.0);
        }
        let steps = (cyl as i32 - self.head as i32).unsigned_abs() as u64;
        if steps > 0 {
            t += steps * ms(6.0) + ms(15.0);
        }
        self.head = cyl;
        // The track: the index gap (146 bytes), then 9 sectors of 614 bytes with their
        // headers and gaps; a read starts at its first sector's ID.
        let start = (146.0 + (first.max(1) - 1) as f64 * 614.0) * byte;
        let angle = ((now + t) % turn) as f64;
        let wait = (start - angle).rem_euclid(turn as f64);
        t += (wait + n as f64 * 614.0 * byte) as u64;
        self.last_disk = Some(now + t);
        t
    }

    // ---- INT 16h
    fn keyboard(&mut self, cpu: &mut Cpu) {
        let ah = (cpu.r[AX] >> 8) as u8;
        match ah {
            0 => match self.keys.pop_front() {
                Some(k) => {
                    self.blocked_reads = 0;
                    cpu.r[AX] = k;
                    self.idle_polls = 0;
                    self.ret(cpu, None, None);
                }
                None => {
                    // Nothing typed: run the INT 16h again (the BIOS waits in a loop).
                    self.idle_polls += 1;
                    self.blocked_reads += 1;
                    cpu.iret(self);
                    cpu.ip = cpu.ip.wrapping_sub(2);
                    cpu.cycles += 200;
                }
            },
            1 => match self.keys.front() {
                Some(&k) => {
                    cpu.r[AX] = k;
                    self.idle_polls = 0;
                    self.ret(cpu, None, Some(false));
                }
                None => {
                    self.idle_polls += 1;
                    self.ret(cpu, None, Some(true));
                }
            },
            _ => self.ret(cpu, None, None),
        }
    }

    /// The screen's dots as the CGA shows them in its 640-dot mode: 200 lines of 640,
    /// each 0 or 1.
    pub fn screen_dots(&self) -> Vec<u8> {
        let mut dots = Vec::with_capacity(640 * 200);
        for y in 0..200 {
            let row = CGA + (y & 1) * 0x2000 + (y >> 1) * 80;
            for x in 0..80 {
                let b = self.ram[row + x];
                dots.extend((0..8).map(|k| b >> (7 - k) & 1));
            }
        }
        dots
    }
}

/// A glyph's row as mode 4 draws it: 8 pixels of 2 bits, the leftmost highest; `colour`
/// where the row's bit is set.
fn expand(row: u8, colour: u8) -> u16 {
    (0..8).fold(0u16, |w, k| w << 2 | if row >> (7 - k) & 1 != 0 { colour as u16 } else { 0 })
}

impl Bus for Machine {
    fn read(&mut self, a: u32) -> u8 {
        match a {
            0..=0x3FFFF => self.ram[a as usize],
            0xB8000..=0xBFFFF => {
                self.cga_accesses += 1;
                self.ram[CGA + (a as usize & 0x3FFF)]
            }
            0xF0000..=0xFFFFF => self.ram[a as usize],
            _ => 0xFF,
        }
    }
    fn write(&mut self, a: u32, v: u8) {
        match a {
            0..=0x3FFFF => self.ram[a as usize] = v,
            0xB8000..=0xBFFFF => {
                self.cga_accesses += 1;
                self.ram[CGA + (a as usize & 0x3FFF)] = v;
            }
            _ => {}
        }
    }
    fn input(&mut self, port: u16, cycles: u64) -> u8 {
        match port {
            0x21 => self.pic_mask,
            0x61 => self.port61,
            // No joystick: the timer bits never drop.
            0x201 => 0xFF,
            0x3D5 => self.cga.crtc.get(self.cga.crtc_index as usize).copied().unwrap_or(0),
            0x3DA => {
                // Status: display enable and vertical retrace, from the frame timing.
                let frame = cycles % 79_545; // 60 Hz
                if frame > 72_000 {
                    0x09
                } else {
                    (cycles / 304 % 2) as u8
                }
            }
            _ => {
                *self.unknown_ports.entry(port).or_default() += 1;
                0xFF
            }
        }
    }
    fn output(&mut self, port: u16, v: u8, cycles: u64) {
        match port {
            0x20 => {}
            0x21 => self.pic_mask = v,
            0x42 | 0x43 => self.sound.push((cycles, port, v)),
            0x61 => {
                self.port61 = v;
                self.sound.push((cycles, port, v));
            }
            0x3D4 => self.cga.crtc_index = v,
            0x3D5 => {
                if let Some(r) = self.cga.crtc.get_mut(self.cga.crtc_index as usize) {
                    *r = v;
                }
            }
            0x3D8 => self.cga.mode = v,
            0x3D9 => self.cga.colour = v,
            _ => *self.unknown_ports.entry(port | 0x8000).or_default() += 1,
        }
    }
}
