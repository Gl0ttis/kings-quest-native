//! The 8086 core, refemu-pcjr's (ports/pcjr/refemu/src/cpu.rs), copied: an 8086 core, with 8088 cycle counts (approximate: the base counts of the Intel
//! tables plus the effective-address time and 4 cycles for each word moved over the
//! 8088's 8-bit bus). Real mode only, the documented instructions and the 8086's
//! aliases (`60`-`6F` as `70`-`7F`, `C0`/`C1`/`C8`/`C9` as `C2`/`C3`/`CA`/`CB`).
//! Anything else stops the run with a report.

pub const CF: u16 = 0x0001;
pub const PF: u16 = 0x0004;
pub const AF: u16 = 0x0010;
pub const ZF: u16 = 0x0040;
pub const SF: u16 = 0x0080;
pub const TF: u16 = 0x0100;
pub const IF: u16 = 0x0200;
pub const DF: u16 = 0x0400;
pub const OF: u16 = 0x0800;

pub const AX: usize = 0;
pub const CX: usize = 1;
pub const DX: usize = 2;
pub const BX: usize = 3;
pub const SP: usize = 4;
pub const BP: usize = 5;
pub const SI: usize = 6;
pub const DI: usize = 7;

pub const ES: usize = 0;
pub const CS: usize = 1;
pub const SS: usize = 2;
pub const DS: usize = 3;

/// What the CPU reads and writes: 1 MB of memory and the I/O ports.
pub trait Bus {
    fn read(&mut self, addr: u32) -> u8;
    fn write(&mut self, addr: u32, v: u8);
    fn input(&mut self, port: u16, cycles: u64) -> u8;
    fn output(&mut self, port: u16, v: u8, cycles: u64);
}

#[derive(Debug, Clone, Default)]
pub struct Cpu {
    pub r: [u16; 8],
    pub s: [u16; 4],
    pub ip: u16,
    pub flags: u16,
    pub cycles: u64,
    pub halted: bool,
    /// Instruction bytes fetched (for the 8088 bus time the counts leave out).
    pub fetched: u64,
    /// Set when an instruction can't be run; the run stops.
    pub fault: Option<String>,
    seg_override: Option<usize>,
}

pub fn lin(seg: u16, off: u16) -> u32 {
    ((seg as u32) << 4).wrapping_add(off as u32) & 0xFFFFF
}

fn parity(v: u8) -> bool {
    v.count_ones().is_multiple_of(2)
}

/// A ModRM operand: a register, or a memory address (segment, offset).
#[derive(Clone, Copy)]
enum Rm {
    Reg(usize),
    Mem(usize, u16),
}

impl Cpu {
    pub fn new() -> Self {
        Cpu { flags: 0xF002, ..Default::default() }
    }

    fn get(&self, f: u16) -> bool {
        self.flags & f != 0
    }
    fn set(&mut self, f: u16, on: bool) {
        if on {
            self.flags |= f
        } else {
            self.flags &= !f
        }
    }

    // ---- registers
    fn r8(&self, n: usize) -> u8 {
        let w = self.r[n & 3];
        if n < 4 {
            w as u8
        } else {
            (w >> 8) as u8
        }
    }
    fn set_r8(&mut self, n: usize, v: u8) {
        let w = &mut self.r[n & 3];
        if n < 4 {
            *w = (*w & 0xFF00) | v as u16
        } else {
            *w = (*w & 0x00FF) | (v as u16) << 8
        }
    }

    // ---- memory
    fn rd8<B: Bus>(&mut self, b: &mut B, seg: usize, off: u16) -> u8 {
        b.read(lin(self.s[seg], off))
    }
    fn rd16<B: Bus>(&mut self, b: &mut B, seg: usize, off: u16) -> u16 {
        let lo = b.read(lin(self.s[seg], off)) as u16;
        let hi = b.read(lin(self.s[seg], off.wrapping_add(1))) as u16;
        lo | hi << 8
    }
    fn wr8<B: Bus>(&mut self, b: &mut B, seg: usize, off: u16, v: u8) {
        b.write(lin(self.s[seg], off), v)
    }
    fn wr16<B: Bus>(&mut self, b: &mut B, seg: usize, off: u16, v: u16) {
        b.write(lin(self.s[seg], off), v as u8);
        b.write(lin(self.s[seg], off.wrapping_add(1)), (v >> 8) as u8);
    }
    fn fetch8<B: Bus>(&mut self, b: &mut B) -> u8 {
        let v = b.read(lin(self.s[CS], self.ip));
        self.ip = self.ip.wrapping_add(1);
        self.fetched += 1;
        v
    }
    fn fetch16<B: Bus>(&mut self, b: &mut B) -> u16 {
        let lo = self.fetch8(b) as u16;
        lo | (self.fetch8(b) as u16) << 8
    }
    pub fn push<B: Bus>(&mut self, b: &mut B, v: u16) {
        self.r[SP] = self.r[SP].wrapping_sub(2);
        let sp = self.r[SP];
        self.wr16(b, SS, sp, v);
    }
    pub fn pop<B: Bus>(&mut self, b: &mut B) -> u16 {
        let sp = self.r[SP];
        let v = self.rd16(b, SS, sp);
        self.r[SP] = sp.wrapping_add(2);
        v
    }

    /// Decode a ModRM byte: (reg field, operand, effective-address cycles).
    fn modrm<B: Bus>(&mut self, b: &mut B) -> (usize, Rm, u64) {
        let m = self.fetch8(b);
        let (md, reg, rm) = ((m >> 6) as usize, ((m >> 3) & 7) as usize, (m & 7) as usize);
        if md == 3 {
            return (reg, Rm::Reg(rm), 0);
        }
        let (base, seg, ea): (u16, usize, u64) = match rm {
            0 => (self.r[BX].wrapping_add(self.r[SI]), DS, 7),
            1 => (self.r[BX].wrapping_add(self.r[DI]), DS, 8),
            2 => (self.r[BP].wrapping_add(self.r[SI]), SS, 8),
            3 => (self.r[BP].wrapping_add(self.r[DI]), SS, 7),
            4 => (self.r[SI], DS, 5),
            5 => (self.r[DI], DS, 5),
            6 => {
                if md == 0 {
                    let d = self.fetch16(b);
                    return (reg, Rm::Mem(self.seg_override.unwrap_or(DS), d), 6);
                }
                (self.r[BP], SS, 5)
            }
            _ => (self.r[BX], DS, 5),
        };
        let (off, extra) = match md {
            0 => (base, 0),
            1 => (base.wrapping_add(self.fetch8(b) as i8 as u16), 4),
            _ => (base.wrapping_add(self.fetch16(b)), 4),
        };
        (reg, Rm::Mem(self.seg_override.unwrap_or(seg), off), ea + extra)
    }

    fn rm8<B: Bus>(&mut self, b: &mut B, rm: Rm) -> u8 {
        match rm {
            Rm::Reg(n) => self.r8(n),
            Rm::Mem(s, o) => self.rd8(b, s, o),
        }
    }
    fn rm16<B: Bus>(&mut self, b: &mut B, rm: Rm) -> u16 {
        match rm {
            Rm::Reg(n) => self.r[n],
            Rm::Mem(s, o) => self.rd16(b, s, o),
        }
    }
    fn set_rm8<B: Bus>(&mut self, b: &mut B, rm: Rm, v: u8) {
        match rm {
            Rm::Reg(n) => self.set_r8(n, v),
            Rm::Mem(s, o) => self.wr8(b, s, o, v),
        }
    }
    fn set_rm16<B: Bus>(&mut self, b: &mut B, rm: Rm, v: u16) {
        match rm {
            Rm::Reg(n) => self.r[n] = v,
            Rm::Mem(s, o) => self.wr16(b, s, o, v),
        }
    }

    // ---- flags for results
    fn szp(&mut self, v: u32, w: bool) {
        let (mask, sign) = if w { (0xFFFF, 0x8000) } else { (0xFF, 0x80) };
        let v = v & mask;
        self.set(ZF, v == 0);
        self.set(SF, v & sign != 0);
        self.set(PF, parity(v as u8));
    }

    /// The eight ALU operations (ADD OR ADC SBB AND SUB XOR CMP); returns the result.
    fn alu(&mut self, op: usize, a: u32, c: u32, w: bool) -> u32 {
        let (mask, sign) = if w { (0xFFFFu32, 0x8000u32) } else { (0xFF, 0x80) };
        let carry = self.get(CF) as u32;
        let r = match op {
            0 | 2 => {
                let cin = if op == 2 { carry } else { 0 };
                let r = a + c + cin;
                self.set(CF, r > mask);
                self.set(AF, (a ^ c ^ r) & 0x10 != 0);
                self.set(OF, (!(a ^ c) & (a ^ r)) & sign != 0);
                r
            }
            3 | 5 | 7 => {
                let bin = if op == 3 { carry } else { 0 };
                let r = a.wrapping_sub(c).wrapping_sub(bin);
                self.set(CF, c + bin > a);
                self.set(AF, (a ^ c ^ r) & 0x10 != 0);
                self.set(OF, ((a ^ c) & (a ^ r)) & sign != 0);
                r
            }
            1 | 4 | 6 => {
                let r = match op {
                    1 => a | c,
                    4 => a & c,
                    _ => a ^ c,
                };
                self.set(CF, false);
                self.set(OF, false);
                self.set(AF, false);
                r
            }
            _ => unreachable!(),
        } & mask;
        self.szp(r, w);
        r
    }

    fn inc_dec(&mut self, v: u32, dec: bool, w: bool) -> u32 {
        let cf = self.get(CF);
        let r = self.alu(if dec { 5 } else { 0 }, v, 1, w);
        self.set(CF, cf);
        r
    }

    /// Shifts and rotates (group 2): ROL ROR RCL RCR SHL SHR (SAL) SAR.
    fn shift(&mut self, op: usize, v: u32, n: u32, w: bool) -> u32 {
        let bits = if w { 16 } else { 8 };
        let (mask, sign) = if w { (0xFFFFu32, 0x8000u32) } else { (0xFF, 0x80) };
        if n == 0 {
            return v;
        }
        let mut r = v & mask;
        for _ in 0..n {
            let cf = self.get(CF) as u32;
            match op {
                0 => {
                    let out = r & sign != 0;
                    r = ((r << 1) | out as u32) & mask;
                    self.set(CF, out);
                }
                1 => {
                    let out = r & 1;
                    r = (r >> 1) | (out << (bits - 1));
                    self.set(CF, out != 0);
                }
                2 => {
                    let out = r & sign != 0;
                    r = ((r << 1) | cf) & mask;
                    self.set(CF, out);
                }
                3 => {
                    let out = r & 1;
                    r = (r >> 1) | (cf << (bits - 1));
                    self.set(CF, out != 0);
                }
                4 | 6 => {
                    self.set(CF, r & sign != 0);
                    r = (r << 1) & mask;
                }
                5 => {
                    self.set(CF, r & 1 != 0);
                    r >>= 1;
                }
                _ => {
                    self.set(CF, r & 1 != 0);
                    r = (r >> 1) | (r & sign);
                }
            }
        }
        // OF as the 8086 leaves it after the last step.
        let of = match op {
            0 | 2 | 4 | 6 => ((r & sign) != 0) != self.get(CF),
            1 | 3 => ((r ^ (r << 1)) & sign) != 0,
            5 => v & sign != 0 && n == 1,
            _ => false,
        };
        self.set(OF, of);
        if op >= 4 {
            self.szp(r, w);
        }
        r
    }

    /// Software or hardware interrupt `n`: push flags, CS, IP; clear IF and TF; jump.
    pub fn interrupt<B: Bus>(&mut self, b: &mut B, n: u8) {
        let f = self.flags;
        self.push(b, f);
        let cs = self.s[CS];
        self.push(b, cs);
        let ip = self.ip;
        self.push(b, ip);
        self.set(IF, false);
        self.set(TF, false);
        let v = (n as u32) * 4;
        self.ip = b.read(v) as u16 | (b.read(v + 1) as u16) << 8;
        self.s[CS] = b.read(v + 2) as u16 | (b.read(v + 3) as u16) << 8;
        self.cycles += 71;
    }

    /// IRET.
    pub fn iret<B: Bus>(&mut self, b: &mut B) {
        self.ip = self.pop(b);
        self.s[CS] = self.pop(b);
        self.flags = (self.pop(b) & 0x0FD5) | 0xF002;
        self.cycles += 44;
    }

    fn cond(&self, c: u8) -> bool {
        let r = match c >> 1 {
            0 => self.get(OF),
            1 => self.get(CF),
            2 => self.get(ZF),
            3 => self.get(CF) || self.get(ZF),
            4 => self.get(SF),
            5 => self.get(PF),
            6 => self.get(SF) != self.get(OF),
            _ => self.get(ZF) || (self.get(SF) != self.get(OF)),
        };
        r != (c & 1 != 0)
    }

    /// Run one instruction (with its prefixes).
    pub fn step<B: Bus>(&mut self, b: &mut B) {
        self.seg_override = None;
        let mut rep: Option<bool> = None; // Some(true) = REPE/REP, Some(false) = REPNE
        let start_ip = self.ip;
        let mut op = self.fetch8(b);
        loop {
            match op {
                0x26 | 0x2E | 0x36 | 0x3E => {
                    self.seg_override = Some(((op >> 3) & 3) as usize);
                    self.cycles += 2;
                }
                0xF2 => rep = Some(false),
                0xF3 => rep = Some(true),
                0xF0 | 0xF1 => {}
                _ => break,
            }
            op = self.fetch8(b);
        }
        let cs = self.s[CS];
        let fault = |s: &mut Cpu, what: &str| {
            s.fault = Some(format!("{what} ${op:02X} at {cs:04X}:{start_ip:04X}"));
        };
        match op {
            // ALU r/m,r ; r,r/m ; AL/AX,imm
            0x00..=0x3F if op & 7 < 6 => {
                let alu = (op >> 3) as usize;
                let w = op & 1 != 0;
                match op & 7 {
                    0..=3 => {
                        let (reg, rm, ea) = self.modrm(b);
                        let to_reg = op & 2 != 0;
                        let (a, c) = if w {
                            let m = self.rm16(b, rm) as u32;
                            if to_reg {
                                (self.r[reg] as u32, m)
                            } else {
                                (m, self.r[reg] as u32)
                            }
                        } else {
                            let m = self.rm8(b, rm) as u32;
                            if to_reg {
                                (self.r8(reg) as u32, m)
                            } else {
                                (m, self.r8(reg) as u32)
                            }
                        };
                        let r = self.alu(alu, a, c, w);
                        if alu != 7 {
                            if to_reg {
                                if w {
                                    self.r[reg] = r as u16
                                } else {
                                    self.set_r8(reg, r as u8)
                                }
                            } else if w {
                                self.set_rm16(b, rm, r as u16)
                            } else {
                                self.set_rm8(b, rm, r as u8)
                            }
                        }
                        self.cycles += match rm {
                            Rm::Reg(_) => 3,
                            Rm::Mem(..) => {
                                let base = if to_reg || alu == 7 { 9 } else { 16 };
                                base + ea
                                    + if w {
                                        if to_reg || alu == 7 {
                                            4
                                        } else {
                                            8
                                        }
                                    } else {
                                        0
                                    }
                            }
                        };
                    }
                    4 => {
                        let c = self.fetch8(b) as u32;
                        let r = self.alu(alu, self.r8(0) as u32, c, false);
                        if alu != 7 {
                            self.set_r8(0, r as u8);
                        }
                        self.cycles += 4;
                    }
                    _ => {
                        let c = self.fetch16(b) as u32;
                        let r = self.alu(alu, self.r[AX] as u32, c, true);
                        if alu != 7 {
                            self.r[AX] = r as u16;
                        }
                        self.cycles += 4;
                    }
                }
            }
            // PUSH/POP segment
            0x06 | 0x0E | 0x16 | 0x1E => {
                let v = self.s[(op >> 3) as usize];
                self.push(b, v);
                self.cycles += 14;
            }
            0x07 | 0x17 | 0x1F => {
                self.s[(op >> 3) as usize] = self.pop(b);
                self.cycles += 12;
            }
            0x27 => {
                // DAA
                let (mut al, old) = (self.r8(0), self.r8(0));
                let cf = self.get(CF);
                if al & 0x0F > 9 || self.get(AF) {
                    al = al.wrapping_add(6);
                    self.set(AF, true);
                }
                if old > 0x99 || cf {
                    al = al.wrapping_add(0x60);
                    self.set(CF, true);
                }
                self.set_r8(0, al);
                self.szp(al as u32, false);
                self.cycles += 4;
            }
            0x2F => {
                // DAS
                let (mut al, old) = (self.r8(0), self.r8(0));
                let cf = self.get(CF);
                if al & 0x0F > 9 || self.get(AF) {
                    al = al.wrapping_sub(6);
                    self.set(AF, true);
                }
                if old > 0x99 || cf {
                    al = al.wrapping_sub(0x60);
                    self.set(CF, true);
                }
                self.set_r8(0, al);
                self.szp(al as u32, false);
                self.cycles += 4;
            }
            0x37 | 0x3F => {
                // AAA / AAS
                if self.r8(0) & 0x0F > 9 || self.get(AF) {
                    let (al, ah) = (self.r8(0), self.r8(4));
                    if op == 0x37 {
                        self.set_r8(0, al.wrapping_add(6));
                        self.set_r8(4, ah.wrapping_add(1));
                    } else {
                        self.set_r8(0, al.wrapping_sub(6));
                        self.set_r8(4, ah.wrapping_sub(1));
                    }
                    self.set(AF, true);
                    self.set(CF, true);
                } else {
                    self.set(AF, false);
                    self.set(CF, false);
                }
                let al = self.r8(0) & 0x0F;
                self.set_r8(0, al);
                self.cycles += 8;
            }
            0x40..=0x4F => {
                let n = (op & 7) as usize;
                self.r[n] = self.inc_dec(self.r[n] as u32, op >= 0x48, true) as u16;
                self.cycles += 3;
            }
            0x50..=0x57 => {
                let n = (op & 7) as usize;
                let v = if n == SP { self.r[SP].wrapping_sub(2) } else { self.r[n] };
                self.push(b, v);
                self.cycles += 15;
            }
            0x58..=0x5F => {
                let v = self.pop(b);
                self.r[(op & 7) as usize] = v;
                self.cycles += 12;
            }
            0x60..=0x7F => {
                let d = self.fetch8(b) as i8 as u16;
                if self.cond(op & 0x0F) {
                    self.ip = self.ip.wrapping_add(d);
                    self.cycles += 16;
                } else {
                    self.cycles += 4;
                }
            }
            0x80..=0x83 => {
                let w = op & 1 != 0;
                let (alu, rm, ea) = self.modrm(b);
                let a = if w { self.rm16(b, rm) as u32 } else { self.rm8(b, rm) as u32 };
                let c = match op {
                    0x81 => self.fetch16(b) as u32,
                    0x83 => self.fetch8(b) as i8 as i16 as u16 as u32,
                    _ => self.fetch8(b) as u32,
                };
                let r = self.alu(alu, a, c, w);
                if alu != 7 {
                    if w {
                        self.set_rm16(b, rm, r as u16)
                    } else {
                        self.set_rm8(b, rm, r as u8)
                    }
                }
                self.cycles += match rm {
                    Rm::Reg(_) => 4,
                    Rm::Mem(..) => (if alu == 7 { 10 } else { 17 }) + ea + if w { 8 } else { 0 },
                };
            }
            0x84 | 0x85 => {
                let w = op & 1 != 0;
                let (reg, rm, ea) = self.modrm(b);
                let (a, c) = if w {
                    (self.rm16(b, rm) as u32, self.r[reg] as u32)
                } else {
                    (self.rm8(b, rm) as u32, self.r8(reg) as u32)
                };
                self.alu(4, a, c, w);
                self.cycles += if matches!(rm, Rm::Reg(_)) { 3 } else { 9 + ea };
            }
            0x86 | 0x87 => {
                let (reg, rm, ea) = self.modrm(b);
                if op == 0x87 {
                    let (a, c) = (self.rm16(b, rm), self.r[reg]);
                    self.set_rm16(b, rm, c);
                    self.r[reg] = a;
                } else {
                    let (a, c) = (self.rm8(b, rm), self.r8(reg));
                    self.set_rm8(b, rm, c);
                    self.set_r8(reg, a);
                }
                self.cycles += if matches!(rm, Rm::Reg(_)) { 4 } else { 17 + ea };
            }
            0x88..=0x8B => {
                let (reg, rm, ea) = self.modrm(b);
                match op {
                    0x88 => {
                        let v = self.r8(reg);
                        self.set_rm8(b, rm, v)
                    }
                    0x89 => {
                        let v = self.r[reg];
                        self.set_rm16(b, rm, v)
                    }
                    0x8A => {
                        let v = self.rm8(b, rm);
                        self.set_r8(reg, v)
                    }
                    _ => self.r[reg] = self.rm16(b, rm),
                }
                self.cycles += match rm {
                    Rm::Reg(_) => 2,
                    Rm::Mem(..) => (if op < 0x8A { 9 } else { 8 }) + ea + if op & 1 != 0 { 4 } else { 0 },
                };
            }
            0x8C => {
                let (reg, rm, ea) = self.modrm(b);
                let v = self.s[reg & 3];
                self.set_rm16(b, rm, v);
                self.cycles += if matches!(rm, Rm::Reg(_)) { 2 } else { 13 + ea };
            }
            0x8D => {
                let (reg, rm, ea) = self.modrm(b);
                if let Rm::Mem(_, o) = rm {
                    self.r[reg] = o;
                }
                self.cycles += 2 + ea;
            }
            0x8E => {
                let (reg, rm, ea) = self.modrm(b);
                self.s[reg & 3] = self.rm16(b, rm);
                self.cycles += if matches!(rm, Rm::Reg(_)) { 2 } else { 12 + ea };
            }
            0x8F => {
                let (_, rm, ea) = self.modrm(b);
                let v = self.pop(b);
                self.set_rm16(b, rm, v);
                self.cycles += 25 + ea;
            }
            0x90..=0x97 => {
                let n = (op & 7) as usize;
                self.r.swap(AX, n);
                self.cycles += 3;
            }
            0x98 => {
                self.r[AX] = self.r8(0) as i8 as i16 as u16;
                self.cycles += 2;
            }
            0x99 => {
                self.r[DX] = if self.r[AX] & 0x8000 != 0 { 0xFFFF } else { 0 };
                self.cycles += 5;
            }
            0x9A => {
                let ip = self.fetch16(b);
                let seg = self.fetch16(b);
                let (cs, ret) = (self.s[CS], self.ip);
                self.push(b, cs);
                self.push(b, ret);
                self.ip = ip;
                self.s[CS] = seg;
                self.cycles += 36;
            }
            0x9B => self.cycles += 4,
            0x9C => {
                let f = self.flags | 0xF002;
                self.push(b, f);
                self.cycles += 14;
            }
            0x9D => {
                self.flags = (self.pop(b) & 0x0FD5) | 0xF002;
                self.cycles += 12;
            }
            0x9E => {
                self.flags = (self.flags & 0xFF00) | (self.r8(4) as u16 & 0xD5) | 2;
                self.cycles += 4;
            }
            0x9F => {
                self.set_r8(4, self.flags as u8);
                self.cycles += 4;
            }
            0xA0..=0xA3 => {
                let o = self.fetch16(b);
                let seg = self.seg_override.unwrap_or(DS);
                match op {
                    0xA0 => {
                        let v = self.rd8(b, seg, o);
                        self.set_r8(0, v)
                    }
                    0xA1 => self.r[AX] = self.rd16(b, seg, o),
                    0xA2 => {
                        let v = self.r8(0);
                        self.wr8(b, seg, o, v)
                    }
                    _ => {
                        let v = self.r[AX];
                        self.wr16(b, seg, o, v)
                    }
                }
                self.cycles += 10 + if op & 1 != 0 { 4 } else { 0 };
            }
            0xA4..=0xA7 | 0xAA..=0xAF => self.string(b, op, rep),
            0xA8 => {
                let c = self.fetch8(b) as u32;
                self.alu(4, self.r8(0) as u32, c, false);
                self.cycles += 4;
            }
            0xA9 => {
                let c = self.fetch16(b) as u32;
                self.alu(4, self.r[AX] as u32, c, true);
                self.cycles += 4;
            }
            0xB0..=0xB7 => {
                let v = self.fetch8(b);
                self.set_r8((op & 7) as usize, v);
                self.cycles += 4;
            }
            0xB8..=0xBF => {
                self.r[(op & 7) as usize] = self.fetch16(b);
                self.cycles += 4;
            }
            0xC0 | 0xC2 => {
                let n = self.fetch16(b);
                self.ip = self.pop(b);
                self.r[SP] = self.r[SP].wrapping_add(n);
                self.cycles += 24;
            }
            0xC1 | 0xC3 => {
                self.ip = self.pop(b);
                self.cycles += 20;
            }
            0xC4 | 0xC5 => {
                let (reg, rm, ea) = self.modrm(b);
                if let Rm::Mem(s, o) = rm {
                    self.r[reg] = self.rd16(b, s, o);
                    self.s[if op == 0xC4 { ES } else { DS }] = self.rd16(b, s, o.wrapping_add(2));
                }
                self.cycles += 24 + ea;
            }
            0xC6 | 0xC7 => {
                let (_, rm, ea) = self.modrm(b);
                if op == 0xC7 {
                    let v = self.fetch16(b);
                    self.set_rm16(b, rm, v)
                } else {
                    let v = self.fetch8(b);
                    self.set_rm8(b, rm, v)
                }
                self.cycles += if matches!(rm, Rm::Reg(_)) { 4 } else { 10 + ea + if op & 1 != 0 { 4 } else { 0 } };
            }
            0xC8 | 0xCA => {
                let n = self.fetch16(b);
                self.ip = self.pop(b);
                self.s[CS] = self.pop(b);
                self.r[SP] = self.r[SP].wrapping_add(n);
                self.cycles += 33;
            }
            0xC9 | 0xCB => {
                self.ip = self.pop(b);
                self.s[CS] = self.pop(b);
                self.cycles += 34;
            }
            0xCC => self.interrupt(b, 3),
            0xCD => {
                let n = self.fetch8(b);
                self.interrupt(b, n);
            }
            0xCE => {
                if self.get(OF) {
                    self.interrupt(b, 4)
                } else {
                    self.cycles += 4
                }
            }
            0xCF => self.iret(b),
            0xD0..=0xD3 => {
                let w = op & 1 != 0;
                let (sub, rm, ea) = self.modrm(b);
                let n = if op >= 0xD2 { self.r8(1) as u32 } else { 1 };
                let v = if w { self.rm16(b, rm) as u32 } else { self.rm8(b, rm) as u32 };
                let r = self.shift(sub, v, n, w);
                if w {
                    self.set_rm16(b, rm, r as u16)
                } else {
                    self.set_rm8(b, rm, r as u8)
                }
                self.cycles += match (rm, op >= 0xD2) {
                    (Rm::Reg(_), false) => 2,
                    (Rm::Reg(_), true) => 8 + 4 * n as u64,
                    (Rm::Mem(..), false) => 15 + ea,
                    (Rm::Mem(..), true) => 20 + ea + 4 * n as u64,
                };
            }
            0xD4 => {
                let base = self.fetch8(b);
                let al = self.r8(0);
                match (al.checked_div(base), al.checked_rem(base)) {
                    (Some(q), Some(r)) => {
                        self.set_r8(4, q);
                        self.set_r8(0, r);
                        self.szp(r as u32, false);
                        self.cycles += 83;
                    }
                    _ => self.interrupt(b, 0),
                }
            }
            0xD5 => {
                let base = self.fetch8(b);
                let al = self.r8(4).wrapping_mul(base).wrapping_add(self.r8(0));
                self.r[AX] = al as u16;
                self.szp(al as u32, false);
                self.cycles += 60;
            }
            0xD6 => {
                let v = if self.get(CF) { 0xFF } else { 0 };
                self.set_r8(0, v);
                self.cycles += 4;
            }
            0xD7 => {
                let seg = self.seg_override.unwrap_or(DS);
                let o = self.r[BX].wrapping_add(self.r8(0) as u16);
                let v = self.rd8(b, seg, o);
                self.set_r8(0, v);
                self.cycles += 11;
            }
            0xD8..=0xDF => {
                // ESC (no coprocessor): decode and skip the operand.
                let _ = self.modrm(b);
                self.cycles += 8;
            }
            0xE0..=0xE3 => {
                let d = self.fetch8(b) as i8 as u16;
                let take = if op == 0xE3 {
                    self.r[CX] == 0
                } else {
                    self.r[CX] = self.r[CX].wrapping_sub(1);
                    self.r[CX] != 0
                        && match op {
                            0xE0 => !self.get(ZF),
                            0xE1 => self.get(ZF),
                            _ => true,
                        }
                };
                if take {
                    self.ip = self.ip.wrapping_add(d);
                    self.cycles += 17;
                } else {
                    self.cycles += 5;
                }
            }
            0xE4 | 0xE5 | 0xEC | 0xED => {
                let port = if op < 0xEC { self.fetch8(b) as u16 } else { self.r[DX] };
                let lo = b.input(port, self.cycles);
                if op & 1 != 0 {
                    let hi = b.input(port.wrapping_add(1), self.cycles);
                    self.r[AX] = lo as u16 | (hi as u16) << 8;
                } else {
                    self.set_r8(0, lo);
                }
                self.cycles += if op < 0xEC { 10 } else { 8 } + if op & 1 != 0 { 4 } else { 0 };
            }
            0xE6 | 0xE7 | 0xEE | 0xEF => {
                let port = if op < 0xEE { self.fetch8(b) as u16 } else { self.r[DX] };
                b.output(port, self.r8(0), self.cycles);
                if op & 1 != 0 {
                    b.output(port.wrapping_add(1), self.r8(4), self.cycles);
                }
                self.cycles += if op < 0xEE { 10 } else { 8 } + if op & 1 != 0 { 4 } else { 0 };
            }
            0xE8 => {
                let d = self.fetch16(b);
                let ret = self.ip;
                self.push(b, ret);
                self.ip = self.ip.wrapping_add(d);
                self.cycles += 23;
            }
            0xE9 => {
                let d = self.fetch16(b);
                self.ip = self.ip.wrapping_add(d);
                self.cycles += 15;
            }
            0xEA => {
                let ip = self.fetch16(b);
                let seg = self.fetch16(b);
                self.ip = ip;
                self.s[CS] = seg;
                self.cycles += 15;
            }
            0xEB => {
                let d = self.fetch8(b) as i8 as u16;
                self.ip = self.ip.wrapping_add(d);
                self.cycles += 15;
            }
            0xF4 => {
                self.halted = true;
                self.cycles += 2;
            }
            0xF5 => {
                self.flags ^= CF;
                self.cycles += 2;
            }
            0xF6 | 0xF7 => self.group3(b, op),
            0xF8 => self.set(CF, false),
            0xF9 => self.set(CF, true),
            0xFA => self.set(IF, false),
            0xFB => self.set(IF, true),
            0xFC => self.set(DF, false),
            0xFD => self.set(DF, true),
            0xFE | 0xFF => {
                let w = op == 0xFF;
                let (sub, rm, ea) = self.modrm(b);
                let mem = matches!(rm, Rm::Mem(..));
                match sub {
                    0 | 1 => {
                        if w {
                            let v = self.rm16(b, rm) as u32;
                            let r = self.inc_dec(v, sub == 1, true);
                            self.set_rm16(b, rm, r as u16);
                        } else {
                            let v = self.rm8(b, rm) as u32;
                            let r = self.inc_dec(v, sub == 1, false);
                            self.set_rm8(b, rm, r as u8);
                        }
                        self.cycles += if mem { 15 + ea + if w { 8 } else { 0 } } else { 3 };
                    }
                    2 if w => {
                        let t = self.rm16(b, rm);
                        let ret = self.ip;
                        self.push(b, ret);
                        self.ip = t;
                        self.cycles += if mem { 29 + ea } else { 20 };
                    }
                    3 if w => {
                        if let Rm::Mem(s, o) = rm {
                            let ip = self.rd16(b, s, o);
                            let seg = self.rd16(b, s, o.wrapping_add(2));
                            let (cs, ret) = (self.s[CS], self.ip);
                            self.push(b, cs);
                            self.push(b, ret);
                            self.ip = ip;
                            self.s[CS] = seg;
                        }
                        self.cycles += 53 + ea;
                    }
                    4 if w => {
                        self.ip = self.rm16(b, rm);
                        self.cycles += if mem { 18 + ea } else { 11 };
                    }
                    5 if w => {
                        if let Rm::Mem(s, o) = rm {
                            self.ip = self.rd16(b, s, o);
                            self.s[CS] = self.rd16(b, s, o.wrapping_add(2));
                        }
                        self.cycles += 24 + ea;
                    }
                    6 if w => {
                        let v = self.rm16(b, rm);
                        self.push(b, v);
                        self.cycles += 24 + ea;
                    }
                    _ => fault(self, &format!("group {op:02X}/{sub}")),
                }
            }
            _ => fault(self, "unknown opcode"),
        }
        // Instructions with a fixed small cost not counted above.
        if matches!(op, 0xF8..=0xFD) {
            self.cycles += 2;
        }
    }

    fn group3<B: Bus>(&mut self, b: &mut B, op: u8) {
        let w = op == 0xF7;
        let (sub, rm, ea) = self.modrm(b);
        let mem = if matches!(rm, Rm::Mem(..)) { ea + 6 } else { 0 };
        match sub {
            0 | 1 => {
                let (a, c) = if w {
                    (self.rm16(b, rm) as u32, self.fetch16(b) as u32)
                } else {
                    (self.rm8(b, rm) as u32, self.fetch8(b) as u32)
                };
                self.alu(4, a, c, w);
                self.cycles += 5 + mem;
            }
            2 => {
                if w {
                    let v = !self.rm16(b, rm);
                    self.set_rm16(b, rm, v)
                } else {
                    let v = !self.rm8(b, rm);
                    self.set_rm8(b, rm, v)
                }
                self.cycles += 3 + mem;
            }
            3 => {
                if w {
                    let v = self.rm16(b, rm) as u32;
                    let r = self.alu(5, 0, v, true);
                    self.set_rm16(b, rm, r as u16);
                } else {
                    let v = self.rm8(b, rm) as u32;
                    let r = self.alu(5, 0, v, false);
                    self.set_rm8(b, rm, r as u8);
                }
                self.cycles += 3 + mem;
            }
            4 | 5 => {
                let signed = sub == 5;
                if w {
                    let v = self.rm16(b, rm);
                    let r: u32 = if signed {
                        (self.r[AX] as i16 as i32 * v as i16 as i32) as u32
                    } else {
                        self.r[AX] as u32 * v as u32
                    };
                    self.r[AX] = r as u16;
                    self.r[DX] = (r >> 16) as u16;
                    let over = if signed { (r as i32) != (r as u16 as i16 as i32) } else { r >> 16 != 0 };
                    self.set(CF, over);
                    self.set(OF, over);
                    self.cycles += 128 + mem;
                } else {
                    let v = self.rm8(b, rm);
                    let r: u16 = if signed {
                        (self.r8(0) as i8 as i16 * v as i8 as i16) as u16
                    } else {
                        self.r8(0) as u16 * v as u16
                    };
                    self.r[AX] = r;
                    let over = if signed { (r as i16) != (r as u8 as i8 as i16) } else { r >> 8 != 0 };
                    self.set(CF, over);
                    self.set(OF, over);
                    self.cycles += 73 + mem;
                }
            }
            _ => {
                let signed = sub == 7;
                if w {
                    let v = self.rm16(b, rm);
                    let n = (self.r[DX] as u32) << 16 | self.r[AX] as u32;
                    let res = if v == 0 {
                        None
                    } else if signed {
                        let (n, d) = (n as i32, v as i16 as i32);
                        let q = n.wrapping_div(d);
                        (q == q as i16 as i32).then(|| (q as u16, n.wrapping_rem(d) as u16))
                    } else {
                        let q = n / v as u32;
                        (q <= 0xFFFF).then(|| (q as u16, (n % v as u32) as u16))
                    };
                    match res {
                        Some((q, r)) => {
                            self.r[AX] = q;
                            self.r[DX] = r;
                            self.cycles += 150 + mem;
                        }
                        None => self.interrupt(b, 0),
                    }
                } else {
                    let v = self.rm8(b, rm);
                    let n = self.r[AX];
                    let res = if v == 0 {
                        None
                    } else if signed {
                        let (n, d) = (n as i16 as i32, v as i8 as i32);
                        let q = n / d;
                        (q == q as i8 as i32).then(|| (q as u8, (n % d) as u8))
                    } else {
                        let q = n / v as u16;
                        (q <= 0xFF).then(|| (q as u8, (n % v as u16) as u8))
                    };
                    match res {
                        Some((q, r)) => {
                            self.set_r8(0, q);
                            self.set_r8(4, r);
                            self.cycles += 85 + mem;
                        }
                        None => self.interrupt(b, 0),
                    }
                }
            }
        }
    }

    /// MOVS CMPS STOS LODS SCAS, with REP/REPE/REPNE.
    fn string<B: Bus>(&mut self, b: &mut B, op: u8, rep: Option<bool>) {
        let w = op & 1 != 0;
        let step: u16 = if w { 2 } else { 1 };
        let delta = if self.get(DF) { step.wrapping_neg() } else { step };
        let src = self.seg_override.unwrap_or(DS);
        let per = match op & 0xFE {
            0xA4 => 17,
            0xA6 => 22,
            0xAA => 10,
            0xAC => 13,
            _ => 15,
        } + if w { 4 } else { 0 };
        if rep.is_some() {
            self.cycles += 9;
        }
        loop {
            if rep.is_some() && self.r[CX] == 0 {
                break;
            }
            let (si, di) = (self.r[SI], self.r[DI]);
            match op {
                0xA4 => {
                    let v = self.rd8(b, src, si);
                    self.wr8(b, ES, di, v);
                }
                0xA5 => {
                    let v = self.rd16(b, src, si);
                    self.wr16(b, ES, di, v);
                }
                0xA6 => {
                    let (x, y) = (self.rd8(b, src, si) as u32, self.rd8(b, ES, di) as u32);
                    self.alu(7, x, y, false);
                }
                0xA7 => {
                    let (x, y) = (self.rd16(b, src, si) as u32, self.rd16(b, ES, di) as u32);
                    self.alu(7, x, y, true);
                }
                0xAA => {
                    let v = self.r8(0);
                    self.wr8(b, ES, di, v);
                }
                0xAB => {
                    let v = self.r[AX];
                    self.wr16(b, ES, di, v);
                }
                0xAC => {
                    let v = self.rd8(b, src, si);
                    self.set_r8(0, v);
                }
                0xAD => self.r[AX] = self.rd16(b, src, si),
                0xAE => {
                    let y = self.rd8(b, ES, di) as u32;
                    self.alu(7, self.r8(0) as u32, y, false);
                }
                _ => {
                    let y = self.rd16(b, ES, di) as u32;
                    self.alu(7, self.r[AX] as u32, y, true);
                }
            }
            if matches!(op, 0xA4 | 0xA5 | 0xA6 | 0xA7 | 0xAC | 0xAD) {
                self.r[SI] = si.wrapping_add(delta);
            }
            if !matches!(op, 0xAC | 0xAD) {
                self.r[DI] = di.wrapping_add(delta);
            }
            self.cycles += per;
            let Some(eq) = rep else { break };
            self.r[CX] = self.r[CX].wrapping_sub(1);
            if matches!(op, 0xA6 | 0xA7 | 0xAE | 0xAF) && self.get(ZF) != eq {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);
    impl Bus for Ram {
        fn read(&mut self, a: u32) -> u8 {
            self.0[a as usize]
        }
        fn write(&mut self, a: u32, v: u8) {
            self.0[a as usize] = v
        }
        fn input(&mut self, _: u16, _: u64) -> u8 {
            0xFF
        }
        fn output(&mut self, _: u16, _: u8, _: u64) {}
    }

    /// Run `code` from 0000:0100 for `n` instructions.
    fn run(code: &[u8], n: usize, setup: impl FnOnce(&mut Cpu)) -> (Cpu, Ram) {
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x100..0x100 + code.len()].copy_from_slice(code);
        let mut cpu = Cpu::new();
        cpu.ip = 0x100;
        cpu.r[SP] = 0xFFFE;
        setup(&mut cpu);
        for _ in 0..n {
            cpu.step(&mut ram);
        }
        assert!(cpu.fault.is_none(), "{:?}", cpu.fault);
        (cpu, ram)
    }

    #[test]
    fn add_and_sub_flags() {
        // MOV AL,0xFF ; ADD AL,1 -> 0, CF ZF AF set, OF clear.
        let (c, _) = run(&[0xB0, 0xFF, 0x04, 0x01], 2, |_| {});
        assert_eq!(c.r[AX] & 0xFF, 0);
        assert!(c.get(CF) && c.get(ZF) && c.get(AF) && !c.get(OF));
        // MOV AL,0x7F ; ADD AL,1 -> 0x80: OF and SF set.
        let (c, _) = run(&[0xB0, 0x7F, 0x04, 0x01], 2, |_| {});
        assert!(c.get(OF) && c.get(SF) && !c.get(CF));
        // MOV AX,1 ; SUB AX,2 -> 0xFFFF with borrow.
        let (c, _) = run(&[0xB8, 1, 0, 0x2D, 2, 0], 2, |_| {});
        assert_eq!(c.r[AX], 0xFFFF);
        assert!(c.get(CF) && c.get(SF));
        // INC leaves CF alone.
        let (c, _) = run(&[0xF9, 0x40], 2, |_| {});
        assert!(c.get(CF));
    }

    #[test]
    fn multiply_divide_and_shifts() {
        // MOV AX,1000 ; MOV BX,300 ; MUL BX -> DX:AX = 300000.
        let (c, _) = run(&[0xB8, 0xE8, 0x03, 0xBB, 0x2C, 0x01, 0xF7, 0xE3], 3, |_| {});
        assert_eq!((c.r[DX] as u32) << 16 | c.r[AX] as u32, 300_000);
        assert!(c.get(CF));
        // DIV BX back: AX = 1000, DX = 0.
        let (c, _) = run(&[0xF7, 0xF3], 1, |c| {
            c.r[DX] = 4;
            c.r[AX] = 0x93E0;
            c.r[BX] = 300;
        });
        assert_eq!((c.r[AX], c.r[DX]), (1000, 0));
        // MOV AL,0x81 ; ROL AL,1 -> 0x03, CF set; SAR keeps the sign.
        let (c, _) = run(&[0xB0, 0x81, 0xD0, 0xC0], 2, |_| {});
        assert_eq!(c.r[AX] & 0xFF, 0x03);
        assert!(c.get(CF));
        let (c, _) = run(&[0xB0, 0x80, 0xD0, 0xF8], 2, |_| {});
        assert_eq!(c.r[AX] & 0xFF, 0xC0);
    }

    #[test]
    fn rep_movsb_and_repne_scasb() {
        // MOV SI,0x200 ; MOV DI,0x300 ; MOV CX,3 ; REP MOVSB
        let (_, ram) = run(&[0xBE, 0, 2, 0xBF, 0, 3, 0xB9, 3, 0, 0xF3, 0xA4], 4, |_| {});
        let _ = ram;
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x200..0x203].copy_from_slice(b"abc");
        ram.0[0x100..0x10B].copy_from_slice(&[0xBE, 0, 2, 0xBF, 0, 3, 0xB9, 3, 0, 0xF3, 0xA4]);
        let mut cpu = Cpu::new();
        cpu.ip = 0x100;
        for _ in 0..4 {
            cpu.step(&mut ram);
        }
        assert_eq!(&ram.0[0x300..0x303], b"abc");
        assert_eq!(cpu.r[CX], 0);
        // REPNE SCASB for 'c' from 0x200: stops past it, CX counts down.
        ram.0[0x100..0x10B].copy_from_slice(&[0xBF, 0, 2, 0xB0, b'c', 0xB9, 9, 0, 0xF2, 0xAE, 0x90]);
        let mut cpu = Cpu::new();
        cpu.ip = 0x100;
        for _ in 0..4 {
            cpu.step(&mut ram);
        }
        assert_eq!(cpu.r[DI], 0x203);
        assert_eq!(cpu.r[CX], 6);
        assert!(cpu.get(ZF));
    }

    #[test]
    fn call_ret_and_interrupts() {
        // CALL +1 ; NOP ; (at 0x104) RET -> back at 0x103.
        let (c, _) = run(&[0xE8, 0x01, 0x00, 0x90, 0xC3], 2, |_| {});
        assert_eq!(c.ip, 0x103);
        // INT 21h through a vector at 0:84 pointing to 0000:0200, where IRET is.
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x84..0x88].copy_from_slice(&[0x00, 0x02, 0x00, 0x00]);
        ram.0[0x200] = 0xCF;
        ram.0[0x100..0x102].copy_from_slice(&[0xCD, 0x21]);
        let mut cpu = Cpu::new();
        cpu.ip = 0x100;
        cpu.r[SP] = 0xFFFE;
        cpu.flags |= IF;
        cpu.step(&mut ram);
        assert_eq!(cpu.ip, 0x200);
        assert!(!cpu.get(IF));
        cpu.step(&mut ram);
        assert_eq!(cpu.ip, 0x102);
        assert!(cpu.get(IF));
    }
}
