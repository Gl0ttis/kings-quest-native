//! The game's resources as the engine uses them: scripts (four sections each), room
//! pictures, views and sounds, read from the player's disk and decoded when the game
//! first asks for them; and the tables and strings in the engine's own data.

use super::{Object, OBJECTS};
use crate::disk::Disk;
use std::cell::RefCell;
use std::rc::Rc;

/// A script: the actions, the cycle rules, the parser rules and the messages, one after
/// another as the loader leaves them; the sections' starts are offsets in `data`.
pub struct Script {
    pub data: Rc<Vec<u8>>,
    pub actions: usize,
    pub cycle: usize,
    pub parser: usize,
    pub messages: usize,
}

impl Script {
    /// Byte `i` (FF past the end, which no well-formed rule reaches).
    pub fn at(&self, i: usize) -> u8 {
        self.data.get(i).copied().unwrap_or(0xFF)
    }
    pub fn word(&self, i: usize) -> u16 {
        u16::from_le_bytes([self.at(i), self.at(i + 1)])
    }
}

/// A view, its number, its length (the word before its bytes), and its bytes as the
/// engine holds them (after the length).
pub struct View {
    pub num: u8,
    pub len: u16,
    pub view: crate::view::View,
    pub raw: Vec<u8>,
}

impl View {
    fn word(&self, at: usize) -> usize {
        u16::from_le_bytes([self.raw.get(at).copied().unwrap_or(0), self.raw.get(at + 1).copied().unwrap_or(0)])
            as usize
    }

    /// Where loop `lp`'s cel `cel` is, from the view's start (`$1E2B`): past the loop
    /// count and the 8 loop sizes, the loops before, the loop's 4-byte header, its cel
    /// size table and the cels before.
    pub fn cel_offset(&self, lp: u8, cel: u8) -> usize {
        let header = 0x11 + (0..lp as usize).map(|l| self.word(1 + 2 * l)).sum::<usize>();
        let count = self.raw.get(header + 3).copied().unwrap_or(0) as usize;
        header + 4 + 2 * count + (0..cel as usize).map(|c| self.word(header + 4 + 2 * c)).sum::<usize>()
    }

    /// The (loop, cel) at `offset` from the view's start, if one is.
    pub fn cel_at(&self, offset: usize) -> Option<(u8, u8)> {
        for (l, lp) in self.view.loops.iter().enumerate() {
            for c in 0..lp.cels.len().max(1) {
                if self.cel_offset(l as u8, c as u8) == offset {
                    return Some((l as u8, c as u8));
                }
            }
        }
        None
    }
}

pub struct Resources {
    disk: Disk,
    /// The Master's sectors (the restart reads its initial state from them).
    pub disk_image: Vec<u8>,
    /// The engine's data segment as loaded (64 KB, zeros past the image): its tables
    /// and strings, at the disk's edition's own addresses. The engine names them by the
    /// first edition's ([`Resources::d`] maps them); a pointer read from it is the
    /// edition's own, and indexes it as it is.
    pub ds: Rc<Vec<u8>>,
    /// The engine's code segment as loaded.
    pub code: Vec<u8>,
    /// The vocabulary (DS:0004), as the parser reads it.
    pub vocab: Vec<u8>,
    scripts: RefCell<Vec<Option<Rc<Script>>>>,
    views: RefCell<Vec<Option<Rc<View>>>>,
    pub edition: crate::disk::Edition,
}

impl Resources {
    pub fn new(disk: &Disk) -> Result<Resources, String> {
        let code = disk.code_segment();
        let e = disk.edition;
        let mut ds = code.get(e.data_segment()..).ok_or("engine too short")?.to_vec();
        ds.resize(0x10000, 0);
        let vocab = decode_vocab(&ds[e.ds(0).ok_or("no data")?..]);
        Ok(Resources {
            disk: Disk {
                image: disk.image.clone(),
                engine: disk.engine.clone(),
                original: disk.original,
                edition: disk.edition,
            },
            disk_image: disk.image.clone(),
            ds: Rc::new(ds),
            code,
            vocab,
            scripts: RefCell::new(vec![None; 256]),
            views: RefCell::new(vec![None; 256]),
            edition: e,
        })
    }

    /// The first edition's data address `a` on the disk's edition.
    pub fn d(&self, a: usize) -> usize {
        self.edition.ds(a).unwrap_or_else(|| panic!("data address {a:04X} isn't mapped to {}", self.edition.name()))
    }

    /// The first edition's code address `a` on the disk's edition.
    pub fn c(&self, a: u16) -> u16 {
        self.edition.code(a).unwrap_or_else(|| panic!("code address {a:04X} isn't in {}", self.edition.name()))
    }

    /// A word of the engine's initial data, at the first edition's address `a`.
    pub fn ds_word(&self, a: usize) -> u16 {
        let a = self.d(a);
        u16::from_le_bytes([self.ds[a], self.ds[a + 1]])
    }

    /// A string in the engine's data at the first edition's address `a`, up to `end`
    /// (FF or 0).
    pub fn ds_bytes(&self, a: usize, end: u8) -> Vec<u8> {
        self.ds[self.d(a)..].iter().take_while(|&&b| b != end).copied().collect()
    }

    /// The object table as the engine starts with it (DS:1967).
    pub fn initial_objects(&self) -> [Object; OBJECTS] {
        let mut o = [Object::default(); OBJECTS];
        for (i, r) in o.iter_mut().enumerate() {
            let at = self.d(0x1967) + 8 * i;
            let b = &self.ds[at..at + 8];
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

    /// Room-table entry `n` read as a script (`$1F29`): the rooms' scripts (0 the
    /// global script) and, from 5Ah, the sound groups. A section length of 1 counts as
    /// 0 (`$1F97`).
    pub fn script(&self, n: u8) -> Rc<Script> {
        if let Some(s) = &self.scripts.borrow()[n as usize] {
            return s.clone();
        }
        let raw = self.disk.table_entry(n as usize).map(|e| self.disk.resource(e)).unwrap_or(&[]);
        let len = |i: usize| match u16::from_le_bytes([
            raw.get(2 * i).copied().unwrap_or(0),
            raw.get(2 * i + 1).copied().unwrap_or(0),
        ]) {
            1 => 0,
            n => n as usize,
        };
        let (a, c, p, m) = (len(0), len(1), len(2), len(3));
        let total = a + c + p + m;
        let data = raw.get(8..(8 + total).min(raw.len())).unwrap_or(&[]).to_vec();
        let s = Rc::new(Script { data: Rc::new(data), actions: 0, cycle: a, parser: a + c, messages: a + c + p });
        self.scripts.borrow_mut()[n as usize] = Some(s.clone());
        s
    }

    /// Room `n`'s picture (`$1D55`): after the script, whose length here is the four
    /// raw section lengths (a 1 is not counted as 0, so room 64's picture starts a
    /// byte early, on a stray byte that ends it at once).
    pub fn picture(&self, n: u8) -> Rc<Vec<u8>> {
        let raw = self.disk.table_entry(n as usize).map(|e| self.disk.resource(e)).unwrap_or(&[]);
        let w = |i: usize| {
            u16::from_le_bytes([raw.get(2 * i).copied().unwrap_or(0), raw.get(2 * i + 1).copied().unwrap_or(0)])
                as usize
        };
        let start = 8 + w(0) + w(1) + w(2) + w(3);
        Rc::new(raw.get(start..).unwrap_or(&[]).to_vec())
    }

    /// The sectors read by each `$1FB6` call loading room-table entry `n`'s script
    /// (`$1F29`): two, then the rest if the script with its header and its offset in
    /// the first sector passes 400h.
    pub fn script_reads(&self, n: u8) -> Vec<(u16, u16)> {
        let Some(e) = self.disk.table_entry(n as usize) else { return vec![(0, 2)] };
        let at = e.sector as u16;
        let raw = self.disk.resource(e);
        let len = |i: usize| match u16::from_le_bytes([
            raw.get(2 * i).copied().unwrap_or(0),
            raw.get(2 * i + 1).copied().unwrap_or(0),
        ]) {
            1 => 0,
            n => n,
        };
        let size = (0..4).fold(8u16.wrapping_add(e.offset as u16), |a, i| a.wrapping_add(len(i)));
        if size > 0x400 {
            vec![(at, 2), (at + 2, (size >> 9).wrapping_sub(1))]
        } else {
            vec![(at, 2)]
        }
    }

    /// The same for room `n`'s picture (`$1D55`): two sectors, then from the one the
    /// picture starts in.
    pub fn picture_reads(&self, n: u8) -> Vec<(u16, u16)> {
        let Some(e) = self.disk.table_entry(n as usize) else { return vec![(0, 2), (0, 0)] };
        let raw = self.disk.resource(e);
        let w = |i: usize| {
            u16::from_le_bytes([raw.get(2 * i).copied().unwrap_or(0), raw.get(2 * i + 1).copied().unwrap_or(0)])
        };
        let at = (0..4).fold((e.offset as u16).wrapping_add(8), |a, i| a.wrapping_add(w(i)));
        let first = e.sector as u16;
        vec![(first, 2), (first + (at >> 9), (e.sectors as u16).wrapping_sub(at >> 9))]
    }

    /// The same for view `v` (`$1DDE`): all its sectors at once.
    pub fn view_reads(&self, v: u8) -> Vec<(u16, u16)> {
        vec![self.disk.view_entry(v as usize).map_or((0, 0), |e| (e.sector as u16, e.sectors as u16))]
    }

    /// View `v` (`$1DDE`).
    pub fn view(&self, v: u8) -> Option<Rc<View>> {
        if let Some(x) = &self.views.borrow()[v as usize] {
            return Some(x.clone());
        }
        let raw = self.disk.view(v as usize)?;
        let view = crate::view::decode(raw).ok()?;
        let len = u16::from_le_bytes([raw.first().copied().unwrap_or(0), raw.get(1).copied().unwrap_or(0)]);
        let x = Rc::new(View { num: v, len, view, raw: raw.get(2..).unwrap_or(&[]).to_vec() });
        self.views.borrow_mut()[v as usize] = Some(x.clone());
        Some(x)
    }

    /// A byte of the engine's code segment, as the random numbers read it (they walk the
    /// code from 0B00h up to 4000h or 5000h, `$0F0A`, `$4150`; the data segment, from
    /// 5110h, is beyond either).
    pub fn code_byte(&self, a: u16) -> u8 {
        self.code.get(a as usize).copied().unwrap_or(0)
    }
}

/// The vocabulary, as the parser reads it: on the PC stored plainly at DS:0004, up to its
/// end marker `{` (the PCjr's, the same 3,259 bytes, is rotated a bit and decoded at
/// start).
fn decode_vocab(ds: &[u8]) -> Vec<u8> {
    let start = crate::vocab::VOCAB;
    let end = (start..ds.len() - 1).find(|&i| ds[i] == b'{' && ds[i + 1] == 0).unwrap_or(start);
    ds[start..=end].to_vec()
}
