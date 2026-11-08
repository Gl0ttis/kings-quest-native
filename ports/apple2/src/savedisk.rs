//! The original's save disk (disk 4, volume 10): reading and writing saved games on a
//! disk image, `.nib` or DOS-order `.do`/`.dsk`, so that saves move between the port
//! and the original (on a real Apple or an emulator).
//!
//! Each letter is one track (A = track 1 .. Z = track 26). The game writes through a
//! 256-byte buffer at `$1600` ($5941, $5E63):
//!
//! - sector 0: the header, with the signature "ame  " (from "game  " at `$7039`) in
//!   bytes 1-5;
//! - then each block as a stream starting at offset 0 of its first sector: a 2-byte
//!   length, then the data, carried on into the following sectors. Objects ($500 bytes)
//!   start in sector 1, actors ($7C) in sector 6, zero page ($90) in sector 7 and
//!   variables ($DF) in sector 8.
//!
//! The object stream runs 2 bytes into sector 6, which the actor block then overwrites,
//! so the original restores `$1F7E-$1F7F` as the actor block's length ($7C, $00). The
//! buffer is not cleared between sectors, so bytes after a block's end hold whatever was
//! there before; nothing reads them.

use crate::nib;

const SIGNATURE: &[u8; 5] = b"ame  ";
/// (first sector, length) of each block, in the port's save image order.
const BLOCKS: [(usize, usize); 4] = [(1, 0x500), (6, 0x7C), (7, 0x90), (8, 0xDF)];
const MAGIC: &[u8; 8] = b"KQSAVE01";

enum Format {
    Nib,
    Dos,
}

pub struct SaveDisk {
    raw: Vec<u8>,
    format: Format,
}

impl SaveDisk {
    pub fn open(raw: Vec<u8>) -> Result<Self, String> {
        let format = match raw.len() {
            nib::NIB_LEN => Format::Nib,
            143_360 => Format::Dos,
            n => return Err(format!("not a 5.25-inch disk image ({n} bytes)")),
        };
        if let Format::Nib = format {
            // The game finds its save disk by volume number.
            let t = nib::decode_track(0, &raw[..nib::TRACK_LEN]);
            match t.sectors.first().map(|s| s.volume) {
                Some(10) => {}
                Some(v) => return Err(format!("not a King's Quest save disk: volume {v}, not 10")),
                None => return Err("the disk is not formatted".into()),
            }
        }
        Ok(SaveDisk { raw, format })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.raw
    }

    /// A sector by DOS 3.3 logical number (as the game's RWTS calls ask for it).
    pub fn read_sector(&self, track: usize, sector: usize) -> Option<[u8; 256]> {
        match self.format {
            Format::Dos => {
                let o = (track * 16 + sector) * 256;
                self.raw.get(o..o + 256)?.try_into().ok()
            }
            Format::Nib => {
                let t = &self.raw[track * nib::TRACK_LEN..(track + 1) * nib::TRACK_LEN];
                let phys = nib::DOS_TO_PHYS[sector] as u8;
                nib::decode_track(track, t)
                    .sectors
                    .into_iter()
                    .find(|s| s.sector == phys && s.data_checksum_ok)
                    .map(|s| s.data)
            }
        }
    }

    pub fn write_sector(&mut self, track: usize, sector: usize, data: &[u8; 256]) -> Result<(), String> {
        match self.format {
            Format::Dos => {
                let o = (track * 16 + sector) * 256;
                self.raw[o..o + 256].copy_from_slice(data);
            }
            Format::Nib => {
                let t = &mut self.raw[track * nib::TRACK_LEN..(track + 1) * nib::TRACK_LEN];
                let phys = nib::DOS_TO_PHYS[sector] as u8;
                let start =
                    nib::data_field(t, phys).ok_or(format!("track {track} sector {sector} is not formatted"))?;
                let n = t.len();
                for (k, b) in nib::encode(data).into_iter().enumerate() {
                    t[(start + k) % n] = b;
                }
            }
        }
        Ok(())
    }

    /// The game saved under `slot` (1-26), as a port save image, if the track holds one.
    pub fn load_game(&self, slot: u8) -> Option<Vec<u8>> {
        let track = slot as usize;
        let header = self.read_sector(track, 0)?;
        if &header[1..6] != SIGNATURE {
            return None;
        }
        let mut img = MAGIC.to_vec();
        for (first, len) in BLOCKS {
            // $5EC7: the stored length, then the data, across sectors.
            let mut sector = first;
            let mut buf = self.read_sector(track, sector)?;
            let stored = buf[0] as usize | (buf[1] as usize) << 8;
            let mut x = 2;
            let mut data = Vec::with_capacity(stored);
            while data.len() < stored {
                if x == 256 {
                    sector += 1;
                    buf = self.read_sector(track, sector)?;
                    x = 0;
                }
                data.push(buf[x]);
                x += 1;
            }
            data.resize(len, 0);
            img.extend_from_slice(&data);
        }
        Some(img)
    }

    /// Write a port save image under `slot` (1-26) as the original does.
    pub fn save_game(&mut self, slot: u8, img: &[u8]) -> Result<(), String> {
        if img.len() < 8 || &img[..8] != MAGIC {
            return Err("not a King's Quest save".into());
        }
        let track = slot as usize;
        // The $1600 buffer, carried from sector to sector as in the original.
        let mut buf = [0u8; 256];
        buf[1..6].copy_from_slice(SIGNATURE);
        self.write_sector(track, 0, &buf)?;
        let mut p = 8;
        for (first, len) in BLOCKS {
            let data = img.get(p..p + len).ok_or("save image too short")?;
            p += len;
            let mut sector = first;
            buf[0] = len as u8;
            buf[1] = (len >> 8) as u8;
            let mut x = 2;
            let mut done = 0;
            loop {
                let n = (256 - x).min(len - done);
                buf[x..x + n].copy_from_slice(&data[done..done + n]);
                done += n;
                self.write_sector(track, sector, &buf)?;
                if done == len {
                    break;
                }
                sector += 1;
                x = 0;
            }
        }
        Ok(())
    }
}
