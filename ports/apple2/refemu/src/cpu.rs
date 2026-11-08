//! NMOS 6502 core with documented opcodes and exact cycle counts (including page-crossing
//! and branch penalties). Undocumented opcodes are reported, not emulated.

pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, v: u8);
    /// Called before each instruction; return true if the bus handled this PC itself
    /// (a firmware substitute), in which case the instruction is not executed.
    fn trap(&mut self, cpu: &mut Cpu) -> bool;
}

#[derive(Default, Clone)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub p: u8,
    pub pc: u16,
    pub cycles: u64,
    pub bad_opcode: Option<(u16, u8)>,
}

pub const C: u8 = 0x01;
pub const Z: u8 = 0x02;
pub const I: u8 = 0x04;
pub const D: u8 = 0x08;
pub const B: u8 = 0x10;
pub const V: u8 = 0x40;
pub const N: u8 = 0x80;

#[derive(Clone, Copy)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Izx,
    Izy,
    Ind,
    Rel,
}

impl Cpu {
    pub fn flag(&self, f: u8) -> bool {
        self.p & f != 0
    }
    pub fn set(&mut self, f: u8, on: bool) {
        if on {
            self.p |= f
        } else {
            self.p &= !f
        }
    }
    fn nz(&mut self, v: u8) {
        self.set(Z, v == 0);
        self.set(N, v & 0x80 != 0);
    }

    pub fn push<Bs: Bus>(&mut self, bus: &mut Bs, v: u8) {
        bus.write(0x100 | self.s as u16, v);
        self.s = self.s.wrapping_sub(1);
    }
    pub fn pull<Bs: Bus>(&mut self, bus: &mut Bs) -> u8 {
        self.s = self.s.wrapping_add(1);
        bus.read(0x100 | self.s as u16)
    }
    /// RTS as performed by firmware substitutes.
    pub fn rts<Bs: Bus>(&mut self, bus: &mut Bs) {
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        self.pc = (lo | hi << 8).wrapping_add(1);
        self.cycles += 6;
    }

    fn fetch<Bs: Bus>(&mut self, bus: &mut Bs) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }
    fn fetch16<Bs: Bus>(&mut self, bus: &mut Bs) -> u16 {
        let lo = self.fetch(bus) as u16;
        lo | (self.fetch(bus) as u16) << 8
    }

    /// Effective address; adds the page-crossing cycle where the instruction pays it.
    fn addr<Bs: Bus>(&mut self, bus: &mut Bs, m: Mode, cross_penalty: bool) -> u16 {
        match m {
            Mode::Zp => self.fetch(bus) as u16,
            Mode::Zpx => self.fetch(bus).wrapping_add(self.x) as u16,
            Mode::Zpy => self.fetch(bus).wrapping_add(self.y) as u16,
            Mode::Abs => self.fetch16(bus),
            Mode::Abx | Mode::Aby => {
                let base = self.fetch16(bus);
                let r = if matches!(m, Mode::Abx) { self.x } else { self.y };
                let a = base.wrapping_add(r as u16);
                if cross_penalty && (a & 0xFF00) != (base & 0xFF00) {
                    self.cycles += 1;
                }
                a
            }
            Mode::Izx => {
                let z = self.fetch(bus).wrapping_add(self.x);
                bus.read(z as u16) as u16 | (bus.read(z.wrapping_add(1) as u16) as u16) << 8
            }
            Mode::Izy => {
                let z = self.fetch(bus);
                let base = bus.read(z as u16) as u16 | (bus.read(z.wrapping_add(1) as u16) as u16) << 8;
                let a = base.wrapping_add(self.y as u16);
                if cross_penalty && (a & 0xFF00) != (base & 0xFF00) {
                    self.cycles += 1;
                }
                a
            }
            Mode::Ind => {
                let p = self.fetch16(bus);
                // NMOS bug: the high byte comes from the same page.
                let hi = (p & 0xFF00) | (p.wrapping_add(1) & 0x00FF);
                bus.read(p) as u16 | (bus.read(hi) as u16) << 8
            }
            Mode::Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                a
            }
            _ => 0,
        }
    }

    fn adc(&mut self, v: u8) {
        let c = self.flag(C) as u16;
        if self.flag(D) {
            let mut lo = (self.a & 0x0F) as u16 + (v & 0x0F) as u16 + c;
            let mut hi = (self.a >> 4) as u16 + (v >> 4) as u16;
            if lo > 9 {
                lo += 6;
                hi += 1;
            }
            let bin = self.a as u16 + v as u16 + c;
            self.set(Z, bin & 0xFF == 0);
            self.set(N, hi & 8 != 0);
            self.set(V, (!(self.a ^ v) & (self.a ^ ((hi << 4) as u8)) & 0x80) != 0);
            if hi > 9 {
                hi += 6;
            }
            self.set(C, hi > 15);
            self.a = (((hi << 4) | (lo & 0x0F)) & 0xFF) as u8;
        } else {
            let r = self.a as u16 + v as u16 + c;
            self.set(V, (!(self.a ^ v) & (self.a ^ r as u8) & 0x80) != 0);
            self.set(C, r > 0xFF);
            self.a = r as u8;
            self.nz(self.a);
        }
    }

    fn sbc(&mut self, v: u8) {
        if self.flag(D) {
            let c = 1 - self.flag(C) as i16;
            let mut lo = (self.a & 0x0F) as i16 - (v & 0x0F) as i16 - c;
            let mut hi = (self.a >> 4) as i16 - (v >> 4) as i16;
            if lo < 0 {
                lo -= 6;
                hi -= 1;
            }
            if hi < 0 {
                hi -= 6;
            }
            let bin = self.a as i16 - v as i16 - c;
            self.set(C, bin >= 0);
            self.set(V, ((self.a ^ v) & (self.a ^ bin as u8) & 0x80) != 0);
            self.nz(bin as u8);
            self.a = (((hi << 4) | (lo & 0x0F)) & 0xFF) as u8;
        } else {
            self.adc(!v);
        }
    }

    fn cmp(&mut self, r: u8, v: u8) {
        self.set(C, r >= v);
        self.nz(r.wrapping_sub(v));
    }

    fn branch<Bs: Bus>(&mut self, bus: &mut Bs, cond: bool) {
        let off = self.fetch(bus) as i8;
        if cond {
            let t = self.pc.wrapping_add(off as i16 as u16);
            self.cycles += if (t & 0xFF00) != (self.pc & 0xFF00) { 2 } else { 1 };
            self.pc = t;
        }
    }

    /// Execute one instruction (or one firmware substitute).
    pub fn step<Bs: Bus>(&mut self, bus: &mut Bs) {
        if bus.trap(self) {
            return;
        }
        let at = self.pc;
        let op = self.fetch(bus);
        use Mode::*;
        // (mnemonic group, mode, base cycles)
        let (m, cyc): (Mode, u64) =
            match op {
                0x69 | 0x29 | 0xC9 | 0xE0 | 0xC0 | 0x49 | 0xA9 | 0xA2 | 0xA0 | 0x09 | 0xE9 => (Imm, 2),
                0x65 | 0x25 | 0x24 | 0xC5 | 0xE4 | 0xC4 | 0x45 | 0xA5 | 0xA6 | 0xA4 | 0x05 | 0xE5 | 0x85 | 0x86
                | 0x84 => (Zp, 3),
                0x06 | 0xC6 | 0xE6 | 0x46 | 0x26 | 0x66 => (Zp, 5),
                0x75 | 0x35 | 0xD5 | 0x55 | 0xB5 | 0xB4 | 0x15 | 0xF5 | 0x95 | 0x94 => (Zpx, 4),
                0x16 | 0xD6 | 0xF6 | 0x56 | 0x36 | 0x76 => (Zpx, 6),
                0xB6 | 0x96 => (Zpy, 4),
                0x6D | 0x2D | 0x2C | 0xCD | 0xEC | 0xCC | 0x4D | 0xAD | 0xAE | 0xAC | 0x0D | 0xED | 0x8D | 0x8E
                | 0x8C => (Abs, 4),
                0x0E | 0xCE | 0xEE | 0x4E | 0x2E | 0x6E => (Abs, 6),
                0x7D | 0x3D | 0xDD | 0x5D | 0xBD | 0xBC | 0x1D | 0xFD => (Abx, 4),
                0x9D => (Abx, 5),
                0x1E | 0xDE | 0xFE | 0x5E | 0x3E | 0x7E => (Abx, 7),
                0x79 | 0x39 | 0xD9 | 0x59 | 0xB9 | 0xBE | 0x19 | 0xF9 => (Aby, 4),
                0x99 => (Aby, 5),
                0x61 | 0x21 | 0xC1 | 0x41 | 0xA1 | 0x01 | 0xE1 | 0x81 => (Izx, 6),
                0x71 | 0x31 | 0xD1 | 0x51 | 0xB1 | 0x11 | 0xF1 => (Izy, 5),
                0x91 => (Izy, 6),
                0x0A | 0x4A | 0x2A | 0x6A => (Acc, 2),
                0x4C => (Abs, 3),
                0x6C => (Ind, 5),
                0x20 => (Abs, 6),
                0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => (Rel, 2),
                0x00 => (Imp, 7),
                0x40 | 0x60 => (Imp, 6),
                0x48 | 0x08 => (Imp, 3),
                0x68 | 0x28 => (Imp, 4),
                0x18 | 0x38 | 0x58 | 0x78 | 0xB8 | 0xD8 | 0xF8 | 0xAA | 0xA8 | 0xBA | 0x8A | 0x9A | 0x98 | 0xE8
                | 0xC8 | 0xCA | 0x88 | 0xEA => (Imp, 2),
                _ => {
                    self.bad_opcode = Some((at, op));
                    self.cycles += 2;
                    return;
                }
            };
        self.cycles += cyc;
        // Read-type instructions pay the page-crossing cycle; stores and RMW do not.
        let penalty =
            matches!(m, Abx | Aby | Izy) && !matches!(op, 0x9D | 0x99 | 0x91 | 0x1E | 0xDE | 0xFE | 0x5E | 0x3E | 0x7E);
        let ea = match m {
            Imp | Acc | Rel => 0,
            _ => self.addr(bus, m, penalty),
        };
        let rd = |cpu: &mut Cpu, bus: &mut Bs| -> u8 {
            let _ = cpu;
            bus.read(ea)
        };
        macro_rules! rmw {
            ($f:expr) => {{
                if matches!(m, Acc) {
                    let v = self.a;
                    let r = $f(self, v);
                    self.a = r;
                    self.nz(r);
                } else {
                    let v = bus.read(ea);
                    let r = $f(self, v);
                    bus.write(ea, r);
                    self.nz(r);
                }
            }};
        }
        match op {
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => {
                let v = rd(self, bus);
                self.adc(v)
            }
            0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 => {
                let v = rd(self, bus);
                self.sbc(v)
            }
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => {
                self.a &= rd(self, bus);
                self.nz(self.a)
            }
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => {
                self.a |= rd(self, bus);
                self.nz(self.a)
            }
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => {
                self.a ^= rd(self, bus);
                self.nz(self.a)
            }
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                let v = rd(self, bus);
                self.cmp(self.a, v)
            }
            0xE0 | 0xE4 | 0xEC => {
                let v = rd(self, bus);
                self.cmp(self.x, v)
            }
            0xC0 | 0xC4 | 0xCC => {
                let v = rd(self, bus);
                self.cmp(self.y, v)
            }
            0x24 | 0x2C => {
                let v = rd(self, bus);
                self.set(Z, self.a & v == 0);
                self.set(N, v & 0x80 != 0);
                self.set(V, v & 0x40 != 0);
            }
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => {
                self.a = rd(self, bus);
                self.nz(self.a)
            }
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                self.x = rd(self, bus);
                self.nz(self.x)
            }
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                self.y = rd(self, bus);
                self.nz(self.y)
            }
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => bus.write(ea, self.a),
            0x86 | 0x96 | 0x8E => bus.write(ea, self.x),
            0x84 | 0x94 | 0x8C => bus.write(ea, self.y),
            0x0A | 0x06 | 0x16 | 0x0E | 0x1E => rmw!(|c: &mut Cpu, v: u8| {
                c.set(C, v & 0x80 != 0);
                v << 1
            }),
            0x4A | 0x46 | 0x56 | 0x4E | 0x5E => rmw!(|c: &mut Cpu, v: u8| {
                c.set(C, v & 1 != 0);
                v >> 1
            }),
            0x2A | 0x26 | 0x36 | 0x2E | 0x3E => rmw!(|c: &mut Cpu, v: u8| {
                let r = v << 1 | c.flag(C) as u8;
                c.set(C, v & 0x80 != 0);
                r
            }),
            0x6A | 0x66 | 0x76 | 0x6E | 0x7E => rmw!(|c: &mut Cpu, v: u8| {
                let r = v >> 1 | (c.flag(C) as u8) << 7;
                c.set(C, v & 1 != 0);
                r
            }),
            0xE6 | 0xF6 | 0xEE | 0xFE => rmw!(|_c: &mut Cpu, v: u8| v.wrapping_add(1)),
            0xC6 | 0xD6 | 0xCE | 0xDE => rmw!(|_c: &mut Cpu, v: u8| v.wrapping_sub(1)),
            0x4C | 0x6C => self.pc = ea,
            0x20 => {
                let ret = self.pc.wrapping_sub(1);
                self.push(bus, (ret >> 8) as u8);
                self.push(bus, ret as u8);
                self.pc = ea;
            }
            0x60 => {
                let lo = self.pull(bus) as u16;
                let hi = self.pull(bus) as u16;
                self.pc = (lo | hi << 8).wrapping_add(1);
            }
            0x40 => {
                self.p = self.pull(bus) | 0x20;
                let lo = self.pull(bus) as u16;
                let hi = self.pull(bus) as u16;
                self.pc = lo | hi << 8;
            }
            0x00 => {
                let ret = self.pc.wrapping_add(1);
                self.push(bus, (ret >> 8) as u8);
                self.push(bus, ret as u8);
                let p = self.p | B | 0x20;
                self.push(bus, p);
                self.set(I, true);
                self.pc = bus.read(0xFFFE) as u16 | (bus.read(0xFFFF) as u16) << 8;
            }
            0x10 => self.branch(bus, !self.flag(N)),
            0x30 => self.branch(bus, self.flag(N)),
            0x50 => self.branch(bus, !self.flag(V)),
            0x70 => self.branch(bus, self.flag(V)),
            0x90 => self.branch(bus, !self.flag(C)),
            0xB0 => self.branch(bus, self.flag(C)),
            0xD0 => self.branch(bus, !self.flag(Z)),
            0xF0 => self.branch(bus, self.flag(Z)),
            0x48 => self.push(bus, self.a),
            0x08 => {
                let p = self.p | B | 0x20;
                self.push(bus, p)
            }
            0x68 => {
                self.a = self.pull(bus);
                self.nz(self.a)
            }
            0x28 => self.p = self.pull(bus) | 0x20,
            0x18 => self.set(C, false),
            0x38 => self.set(C, true),
            0x58 => self.set(I, false),
            0x78 => self.set(I, true),
            0xB8 => self.set(V, false),
            0xD8 => self.set(D, false),
            0xF8 => self.set(D, true),
            0xAA => {
                self.x = self.a;
                self.nz(self.x)
            }
            0xA8 => {
                self.y = self.a;
                self.nz(self.y)
            }
            0xBA => {
                self.x = self.s;
                self.nz(self.x)
            }
            0x8A => {
                self.a = self.x;
                self.nz(self.a)
            }
            0x9A => self.s = self.x,
            0x98 => {
                self.a = self.y;
                self.nz(self.a)
            }
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.nz(self.x)
            }
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.nz(self.y)
            }
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.nz(self.x)
            }
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.nz(self.y)
            }
            0xEA => {}
            _ => unreachable!(),
        }
    }
}
