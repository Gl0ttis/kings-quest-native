//! A PCjr for measuring the original: 128 KB of RAM shared with the video (the Video
//! Gate Array shows a "CRT page" of it), the 18.2 Hz timer, the sound chip and the
//! disk. No IBM ROM: the BIOS services the game calls are answered here, natively
//! (see `bios`). Every interrupt vector starts at a stub in segment F000; running into
//! a stub calls the matching service.

use crate::cpu::{self, Bus, Cpu, AX, BX, CF, CS, CX, DX, ES, IF, SP, SS, ZF};
use std::collections::{HashMap, VecDeque};

/// CPU clocks per BIOS timer tick: the PIT runs at a quarter of the 4.77 MHz CPU clock
/// and counts 65,536 per tick.
pub const CYCLES_PER_TICK: u64 = 4 * 65_536;
pub const CPU_HZ: f64 = 14_318_180.0 / 3.0;

/// Where the vector stubs are: F000:0100 + n.
const STUB_SEG: u16 = 0xF000;
const STUB_BASE: u16 = 0x0100;
/// A standard 360K diskette-parameter table, at F000:0010.
const PARAM_TABLE: u16 = 0x0010;

/// A disk as the controller sees it: sectors by (cylinder, head, record, size code).
#[derive(Default, Clone)]
pub struct Disk {
    pub sectors: HashMap<(u8, u8, u8, u8), Vec<u8>>,
    pub written: bool,
    /// A write-protected disk (the Master): writes and formats fail with status 3.
    pub write_protected: bool,
}

impl Disk {
    pub fn from_tracks(tracks: &[kq1_pcjr::mfm::Track]) -> Disk {
        let mut d = Disk::default();
        for t in tracks {
            for s in &t.sectors {
                d.sectors.insert((t.cylinder, t.side, s.record, s.size), s.data.clone());
            }
        }
        d
    }
}

#[derive(Clone)]
pub struct Machine {
    pub ram: Vec<u8>,
    /// The Video Gate Array's page registers: the CRT page shown, and the CPU page the
    /// B800 window reaches (16 KB pages).
    pub crt_page: u8,
    pub cpu_page: u8,
    pub video_mode: u8,
    pub pic_mask: u8,
    pub port61: u8,
    /// Sound chip writes: (cycle, byte).
    pub sound: Vec<(u64, u8)>,
    pub disks: Vec<Disk>,
    pub drive: usize,
    pub keys: VecDeque<u16>,
    /// The character in each text cell (25 rows of 40), as the BIOS last wrote it.
    pub text: [[u8; 40]; 25],
    pub cursor: (u8, u8),
    pub next_tick: u64,
    /// BIOS calls made: (vector, AH) -> count.
    pub calls: HashMap<(u8, u8), u64>,
    /// Ports read or written that nothing here models.
    pub unknown_ports: HashMap<u16, u64>,
    pub notes: Vec<String>,
    /// Keyboard polls since the last key (INT 16h AH=0/1 with nothing waiting).
    pub idle_polls: u64,
    /// Blocking keyboard reads (INT 16h AH=0) found nothing typed, since this was reset:
    /// the game is waiting at a prompt.
    pub blocked_reads: u64,
    /// The last arrow key a plan pressed (for `S`, which presses it again).
    pub last_arrow: u16,
    /// Cycle of the last joystick trigger (port 201h write).
    joy_trigger: u64,
    /// Memory accesses to the shared RAM (the low 128 KB) during the current instruction.
    shared_accesses: u64,
    /// Wait states added for the Video Gate Array per access to the shared RAM.
    pub wait_states: u64,
    /// Cycles added for wait states so far.
    pub wait_cycles: u64,
    /// CPU cycles a sector read or write takes (the drive's time, not modelled in
    /// detail: one figure, to calibrate).
    pub sector_cycles: u64,
    /// Cycles added because the bus, not the execution unit, set the pace.
    pub bus_cycles: u64,
    /// Clocks per instruction byte fetched (4: one 8088 bus cycle).
    pub fetch_cycles: u64,
    /// Totals for calibration: execution-unit cycles of the instructions, memory
    /// accesses (fetches included), disk cycles.
    pub eu_total: u64,
    pub access_total: u64,
    pub disk_total: u64,
    /// Accesses through the B800 window (where 86Box applies the PCjr's wait states).
    pub b800_total: u64,
    pub sectors_read: u64,
    /// Checkpoints passed so far (kq1_pcjr::engine::clock::CHECKPOINTS), counted by
    /// the main loop.
    pub checkpoint: u64,
    /// The event log (`--events`): ticks, keys and disk swaps against the checkpoint
    /// count, for replaying in the port.
    pub events: Option<String>,
}

impl Machine {
    /// Disk `n` (0: the Master; 1: the Play diskette) into the drive.
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

    pub fn new(game: Disk) -> Machine {
        let mut m = Machine {
            ram: vec![0; 0x100000],
            crt_page: 6,
            cpu_page: 6,
            video_mode: 0,
            pic_mask: 0,
            port61: 0,
            sound: Vec::new(),
            disks: vec![game],
            drive: 0,
            keys: VecDeque::new(),
            text: [[b' '; 40]; 25],
            cursor: (0, 0),
            next_tick: CYCLES_PER_TICK,
            calls: HashMap::new(),
            unknown_ports: HashMap::new(),
            notes: Vec::new(),
            idle_polls: 0,
            blocked_reads: 0,
            last_arrow: 0x4D00,
            joy_trigger: 0,
            shared_accesses: 0,
            wait_states: 0,
            wait_cycles: 0,
            sector_cycles: 100_000,
            bus_cycles: 0,
            fetch_cycles: 4,
            eu_total: 0,
            access_total: 0,
            disk_total: 0,
            b800_total: 0,
            sectors_read: 0,
            checkpoint: 0,
            events: None,
        };
        // Interrupt vectors -> stubs; the diskette parameters; the BIOS data area.
        for n in 0..256u32 {
            let off = STUB_BASE + n as u16;
            m.ram[(n * 4) as usize] = off as u8;
            m.ram[(n * 4 + 1) as usize] = (off >> 8) as u8;
            m.ram[(n * 4 + 2) as usize] = STUB_SEG as u8;
            m.ram[(n * 4 + 3) as usize] = (STUB_SEG >> 8) as u8;
        }
        let p = cpu::lin(STUB_SEG, PARAM_TABLE) as usize;
        m.ram[p..p + 11].copy_from_slice(&[0xDF, 0x02, 0x25, 0x02, 0x09, 0x2A, 0xFF, 0x50, 0xF6, 0x0F, 0x02]);
        m.set_vector(0x1E, STUB_SEG, PARAM_TABLE);
        m.w16(0x410, 0x0021); // equipment: diskette, 80x25 colour
        m.w16(0x413, 128); // memory size, KB
        m
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

    /// Load the boot sector to 0000:7C00 and point the CPU at it, as the BIOS does.
    pub fn boot(&mut self, cpu: &mut Cpu) -> Result<(), String> {
        let boot = self.disks[0].sectors.get(&(0, 0, 1, 2)).ok_or("no boot sector")?.clone();
        self.ram[0x7C00..0x7E00].copy_from_slice(&boot);
        // The IBM BIOS boots with its stack at 0030:0100, below everything a boot
        // sector loads.
        cpu.s = [0, 0, 0x30, 0];
        cpu.ip = 0x7C00;
        cpu.r[SP] = 0x100;
        cpu.r[DX] = 0;
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
            self.shared_accesses = 0;
            let (before, fetched) = (cpu.cycles, cpu.fetched);
            cpu.step(self);
            // The 8088 fetches instructions a byte at a time, 4 clocks each. Intel's
            // counts assume the prefetch queue is full, so they leave this out (data
            // transfers are in them): the fetch time is added (README.md, "Timing").
            let bus = (cpu.fetched - fetched) * self.fetch_cycles;
            let ran = cpu.cycles - before;
            self.eu_total += ran;
            self.access_total += self.shared_accesses;
            cpu.cycles += bus;
            self.bus_cycles += bus;
            // The Video Gate Array's wait states on shared-RAM accesses (0 by default,
            // as 86Box has none outside the B800 window; IBM gives 2 on average).
            let w = self.shared_accesses * self.wait_states;
            cpu.cycles += w;
            self.wait_cycles += w;
        }
        if cpu.cycles >= self.next_tick {
            if cpu.flags & IF != 0 && self.pic_mask & 1 == 0 {
                self.next_tick += CYCLES_PER_TICK;
                if let Some(e) = self.events.as_mut() {
                    let ip = if cpu.s[cpu::CS] == 0x60 { cpu.ip } else { 0xFFFF };
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
        // Rough times for the ROM routines this replaces (estimates, to calibrate): the
        // timer tick, a keyboard check, a character drawn in graphics mode, a scroll.
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
            _ => {
                cpu.fault = Some(format!("unhandled BIOS call INT {n:02X}h AH={ah:02X}h"));
            }
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
                // 0040:0089, the horizontal screen position: register 2 of the mode's
                // CRTC table, as IBM's BIOS sets it (its tables at F000:F0A4, 16 bytes
                // each: modes 0-1, 2-3, 4-8, 9 and up).
                self.ram[0x489] = match self.video_mode {
                    0 | 1 => 0x2C,
                    2 | 3 => 0x5A,
                    4..=8 => 0x2B,
                    _ => 0x56,
                };
                // 32 KB modes use pages 6-7 on a 128 KB PCjr, 16 KB modes page 7.
                let page = if matches!(self.video_mode, 9 | 10) { 6 } else { 7 };
                self.crt_page = page;
                self.cpu_page = page;
                if al & 0x80 == 0 {
                    let base = page as usize * 0x4000;
                    let len = if page == 6 { 0x8000 } else { 0x4000 };
                    self.ram[base..base + len].fill(0);
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
            5 => match al {
                0x80 => cpu.r[BX] = (self.crt_page as u16) << 8 | self.cpu_page as u16,
                0x81 => self.cpu_page = bl,
                0x82 => self.crt_page = bh,
                0x83 => {
                    self.crt_page = bh;
                    self.cpu_page = bl;
                }
                _ => {}
            },
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
            0x0E => self.teletype(al, bl),
            _ => cpu.fault = Some(format!("unhandled INT 10h AH={ah:02X}h")),
        }
        self.ret(cpu, None, None);
    }

    /// Bytes per character cell row and the cell's address in the CRT page: mode 9 has
    /// 4 bits a pixel (4 bytes per 8-pixel cell), 160 bytes a scan line, four banks.
    fn cell_row(&self, row: u8, col: u8, line: u8) -> Option<usize> {
        if row >= 25 || col >= 40 {
            return None;
        }
        let y = row as usize * 8 + line as usize;
        let base = self.crt_page as usize * 0x4000;
        match self.video_mode {
            9 => Some(base + (y & 3) * 0x2000 + (y >> 2) * 160 + col as usize * 4),
            4 | 5 => Some(base + (y & 1) * 0x2000 + (y >> 1) * 80 + col as usize * 2),
            _ => None,
        }
    }

    /// Draw a character in graphics mode. The glyph would come from the BIOS ROM's
    /// font, which isn't here: the cell is cleared to the background (the BIOS writes
    /// the whole cell, the glyph in the colour and the rest 0) and the character
    /// recorded in `text`, where the comparison reads it. With attribute bit 7 the BIOS
    /// XORs the glyph onto the cell instead (the title's credits), and the cell is left
    /// as it is.
    fn draw_char(&mut self, row: u8, col: u8, ch: u8, attr: u8) {
        if row < 25 && col < 40 {
            self.text[row as usize][col as usize] = ch;
        }
        if attr & 0x80 != 0 {
            return;
        }
        let width = if self.video_mode == 9 { 4 } else { 2 };
        for line in 0..8 {
            if let Some(a) = self.cell_row(row, col, line) {
                self.ram[a..a + width].fill(0);
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

    /// INT 10h AH=6: scroll a window up `lines` (0: clear it), filling with `fill`.
    fn scroll(&mut self, lines: u8, fill: u8, (top, left): (u8, u8), (bottom, right): (u8, u8)) {
        let (bottom, right) = (bottom.min(24), right.min(39));
        if top > bottom || left > right {
            return;
        }
        let height = bottom - top + 1;
        let lines = if lines == 0 || lines > height { height } else { lines };
        let width = if self.video_mode == 9 { 4 } else { 2 };
        let fill_byte = if self.video_mode == 9 { (fill & 0x0F) * 0x11 } else { 0 };
        for r in top..=bottom {
            let src = r + lines;
            for c in left..=right {
                for line in 0..8 {
                    let Some(d) = self.cell_row(r, c, line) else { continue };
                    if src <= bottom {
                        let s = self.cell_row(src, c, line).unwrap();
                        let tmp: Vec<u8> = self.ram[s..s + width].to_vec();
                        self.ram[d..d + width].copy_from_slice(&tmp);
                    } else {
                        self.ram[d..d + width].fill(fill_byte);
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
        // The sector size and the last sector on a track come from the parameter table.
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
                let mut done = 0u8;
                let mut status = 0u8;
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
                cpu.cycles += done as u64 * self.sector_cycles;
                self.disk_total += done as u64 * self.sector_cycles;
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

    // ---- INT 16h
    fn keyboard(&mut self, cpu: &mut Cpu) {
        let ah = (cpu.r[AX] >> 8) as u8;
        match ah {
            0 => match self.keys.pop_front() {
                Some(k) => {
                    self.blocked_reads = 0;
                    if std::env::var_os("REFEMU_KEYLOG").is_some() {
                        eprintln!("key {k:04X} read at cycle {}", cpu.cycles);
                    }
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

    /// The CRT page as shown, for mode 9: 200 rows of 320 4-bit pixels.
    pub fn screen_mode9(&self) -> Vec<u8> {
        let base = self.crt_page as usize * 0x4000;
        let mut px = Vec::with_capacity(320 * 200);
        for y in 0..200 {
            let row = base + (y & 3) * 0x2000 + (y >> 2) * 160;
            for x in 0..160 {
                let b = self.ram[row + x];
                px.push(b >> 4);
                px.push(b & 0x0F);
            }
        }
        px
    }
}

impl Bus for Machine {
    fn read(&mut self, a: u32) -> u8 {
        match a {
            0..=0x1FFFF => {
                self.shared_accesses += 1;
                self.ram[a as usize]
            }
            // The B800 window: the CPU page (32 KB in the 32 KB modes).
            0xB8000..=0xBFFFF => {
                self.b800_total += 1;
                let page = (self.cpu_page & !1) as usize * 0x4000;
                self.ram[(page + (a as usize - 0xB8000)) & 0x1FFFF]
            }
            0xF0000..=0xFFFFF => self.ram[a as usize],
            _ => 0xFF,
        }
    }
    fn write(&mut self, a: u32, v: u8) {
        match a {
            0..=0x1FFFF => {
                self.shared_accesses += 1;
                self.ram[a as usize] = v
            }
            0xB8000..=0xBFFFF => {
                self.b800_total += 1;
                let page = (self.cpu_page & !1) as usize * 0x4000;
                self.ram[(page + (a as usize - 0xB8000)) & 0x1FFFF] = v;
            }
            _ => {}
        }
    }
    fn input(&mut self, port: u16, cycles: u64) -> u8 {
        match port {
            0x21 => self.pic_mask,
            0x61 => self.port61,
            // No joystick: the timer bits never drop.
            0x201 => {
                let _ = cycles.saturating_sub(self.joy_trigger);
                0xFF
            }
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
            0x61 => self.port61 = v,
            0xC0 => self.sound.push((cycles, v)),
            0x201 => self.joy_trigger = cycles,
            _ => *self.unknown_ports.entry(port | 0x8000).or_default() += 1,
        }
    }
}
