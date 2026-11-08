//! The game disk: 40 tracks, 2 sides, 9 sectors of 512 bytes, with no DOS filesystem;
//! except that on the original, tracks 0 side 1 to 3 side 0 hold 5 sectors of 1,024
//! bytes each: the engine, which the boot sector loads from there (the copy
//! protection). The engine finds every resource through a table at the start of its
//! code segment. See docs/disk-layout.md.
//!
//! Two kinds of image are read: the original as dumped (an HxC `.mfm`, see mfm.rs),
//! and the cracked 360K sector image (`.img`), which has the engine copied to sectors
//! 648-707 instead (byte for byte the same) and a boot sector patched to load it there.

/// Bytes in a 360K image.
pub const IMAGE_LEN: usize = 368_640;
pub const SECTOR: usize = 512;

/// The engine: 30 KB, loaded to 0060:0200. The original's boot sector reads it as 30
/// sectors of 1,024 bytes from track 0 side 1 (`$90`, through its read routine `$9D`
/// set for 5 sectors a track); the crack's reads 60 sectors of 512 from sector 648.
pub const ENGINE_LEN: usize = 30 * 1024;
/// Where the crack keeps the engine.
pub const ENGINE_SECTOR: usize = 648;
/// Where the engine starts in its code segment (segment 0x60).
pub const ENGINE_ADDR: usize = 0x200;

/// Room `r`'s directory entry is at CS:`0x100 + 4r` (engine `$4AFC` with base 0). Room
/// 0 is the global script. The room resource is the room's script followed by its
/// picture.
pub const ROOM_TABLE: usize = 0x100;
/// Rooms with an entry: 0-84. (85-89 are unused, filled with `*`.)
pub const ROOMS: usize = 85;
/// Ten more entries in the same table, numbers 90-99, hold sound-chip data
/// (sounds; not yet confirmed from the engine).
pub const SOUND_FIRST: usize = 90;
pub const SOUNDS: usize = 10;
/// View `v`'s entry is at CS:`0x300 + 4v` (engine `$4BBF`, base 0x200).
pub const VIEW_TABLE: usize = 0x300;
/// Views with an entry: 0-110.
pub const VIEWS: usize = 111;

/// A directory entry: the resource starts `offset` bytes into `sector` and is read as
/// `sectors` sectors from there (engine `$4B35`, `$4BBF`, `$4D06`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub sector: usize,
    pub offset: usize,
    pub sectors: usize,
}

impl Entry {
    /// Decode the 4 bytes `b0 b1 b2 b3`: offset = b0 | (b1 bit 7) << 8; sectors =
    /// (b1:b2 >> 4) & 0x3FF; sector = (b2:b3) & 0x3FF (big-endian pairs). `None` for
    /// the filler of unused entries (`****`).
    pub fn decode(b: [u8; 4]) -> Option<Entry> {
        if b == [b'*'; 4] {
            return None;
        }
        Some(Entry {
            offset: b[0] as usize | ((b[1] >> 7) as usize) << 8,
            sectors: (u16::from_be_bytes([b[1], b[2]]) >> 4) as usize & 0x3FF,
            sector: u16::from_be_bytes([b[2], b[3]]) as usize & 0x3FF,
        })
    }

    /// Where the resource starts in the image.
    pub fn start(&self) -> usize {
        self.sector * SECTOR + self.offset
    }
}

pub struct Disk {
    /// The 512-byte sectors, as a 360K image (the original's 1 KB-sector tracks are
    /// left as the format filler there).
    pub image: Vec<u8>,
    /// The engine, as loaded to 0060:0200.
    pub engine: Vec<u8>,
    /// Read from the original (an MFM dump) rather than the crack.
    pub original: bool,
}

impl Disk {
    /// A disk image file: an HxC MFM dump of the original, or a 360K sector image.
    pub fn from_file(raw: Vec<u8>) -> Result<Disk, String> {
        if raw.starts_with(b"HXCMFM\0") {
            Disk::from_tracks(&crate::mfm::decode(&raw)?)
        } else {
            Disk::new(raw)
        }
    }

    /// A 360K sector image (the crack): the engine is in sectors 648-707.
    pub fn new(image: Vec<u8>) -> Result<Disk, String> {
        if image.len() != IMAGE_LEN {
            return Err(format!(
                "not a disk image of this game ({} bytes; expected an HxC .mfm dump or a {IMAGE_LEN}-byte .img)",
                image.len()
            ));
        }
        let engine = image[ENGINE_SECTOR * SECTOR..ENGINE_SECTOR * SECTOR + ENGINE_LEN].to_vec();
        Disk::checked(Disk { image, engine, original: false })
    }

    /// The original's tracks: 512-byte sectors into the image, and the engine from the
    /// 1 KB sectors in disk order.
    pub fn from_tracks(tracks: &[crate::mfm::Track]) -> Result<Disk, String> {
        let mut image = vec![0xF6u8; IMAGE_LEN];
        let mut big: Vec<(u8, u8, u8, &[u8])> = Vec::new();
        for t in tracks {
            for s in &t.sectors {
                match s.size {
                    2 if (1..=9).contains(&s.record) && t.cylinder < 40 && t.side < 2 => {
                        let lin = (t.cylinder as usize * 2 + t.side as usize) * 9 + s.record as usize - 1;
                        image[lin * SECTOR..(lin + 1) * SECTOR].copy_from_slice(&s.data);
                    }
                    3 => big.push((t.cylinder, t.side, s.record, &s.data)),
                    _ => {}
                }
            }
        }
        big.sort_by_key(|b| (b.0, b.1, b.2));
        let engine: Vec<u8> = big.iter().flat_map(|b| b.3.iter().copied()).collect();
        if engine.len() != ENGINE_LEN {
            return Err(format!(
                "expected the engine as 30 sectors of 1,024 bytes, found {} bytes; not the original disk?",
                engine.len()
            ));
        }
        Disk::checked(Disk { image, engine, original: true })
    }

    fn checked(disk: Disk) -> Result<Disk, String> {
        for (what, n, e) in disk.entries() {
            let end = e.sector * SECTOR + e.sectors.max(1) * SECTOR;
            if end > IMAGE_LEN || e.start() >= IMAGE_LEN {
                return Err(format!("{what} {n}: entry {e:?} is outside the disk; not this game's disk?"));
            }
        }
        Ok(disk)
    }

    /// The engine's code segment as loaded: the boot sector at 0, the engine from 0x200.
    /// (The crack's boot sector differs from the original's in its loading code, before
    /// `$100`; the resource table after it is the same.)
    pub fn code_segment(&self) -> Vec<u8> {
        let mut cs = self.image[..SECTOR].to_vec();
        cs.extend_from_slice(&self.engine);
        cs
    }

    fn entry_at(&self, cs_addr: usize) -> Option<Entry> {
        let cs = self.code_segment();
        Entry::decode(cs[cs_addr..cs_addr + 4].try_into().unwrap())
    }

    pub fn room_entry(&self, r: usize) -> Option<Entry> {
        (r < ROOMS).then(|| self.entry_at(ROOM_TABLE + 4 * r)).flatten()
    }

    pub fn sound_entry(&self, s: usize) -> Option<Entry> {
        (s < SOUNDS).then(|| self.entry_at(ROOM_TABLE + 4 * (SOUND_FIRST + s))).flatten()
    }

    pub fn view_entry(&self, v: usize) -> Option<Entry> {
        (v < VIEWS).then(|| self.entry_at(VIEW_TABLE + 4 * v)).flatten()
    }

    /// Every present entry: (kind, number, entry).
    pub fn entries(&self) -> Vec<(&'static str, usize, Entry)> {
        let mut v = Vec::new();
        v.extend((0..ROOMS).filter_map(|n| Some(("room", n, self.room_entry(n)?))));
        v.extend((0..SOUNDS).filter_map(|n| Some(("sound", n, self.sound_entry(n)?))));
        v.extend((0..VIEWS).filter_map(|n| Some(("view", n, self.view_entry(n)?))));
        v
    }

    /// A resource's bytes: from its start to the start of the next resource on the disk,
    /// or the end of the sectors its entry reads, whichever comes first. (Resources
    /// carry their own lengths; this is an upper bound.)
    pub fn resource(&self, e: Entry) -> &[u8] {
        let start = e.start();
        let limit = (e.sector + e.sectors) * SECTOR;
        let next = self.entries().iter().map(|x| x.2.start()).filter(|&s| s > start).min().unwrap_or(limit);
        &self.image[start..next.min(limit).min(IMAGE_LEN)]
    }

    /// Room `r`: its script and the picture after it. The script is an 8-byte header
    /// of four section lengths and the sections; a length of 1 is an empty section
    /// (engine `$4D74`). Room 0 (the global script) has no picture.
    pub fn room(&self, r: usize) -> Option<Room<'_>> {
        let data = self.resource(self.room_entry(r)?);
        let len = (0..4)
            .map(|i| match u16::from_le_bytes([data[2 * i], data[2 * i + 1]]) as usize {
                1 => 0,
                n => n,
            })
            .sum::<usize>()
            + 8;
        let len = len.min(data.len());
        Some(Room { script: &data[..len], picture: &data[len..] })
    }

    /// View `v`: a u16 length (of what follows it), then the view; with the length.
    pub fn view(&self, v: usize) -> Option<&[u8]> {
        let data = self.resource(self.view_entry(v)?);
        let len = 2 + u16::from_le_bytes([data[0], data[1]]) as usize;
        Some(&data[..len.min(data.len())])
    }

    pub fn sound(&self, s: usize) -> Option<&[u8]> {
        Some(self.resource(self.sound_entry(s)?))
    }
}

pub struct Room<'a> {
    pub script: &'a [u8],
    /// The picture, up to the next resource on the disk (it ends at its own `$FF`).
    pub picture: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_decoding() {
        // Room 4's entry: offset 293 in sector 106, 4 sectors.
        assert_eq!(Entry::decode([0x25, 0x80, 0x40, 0x6a]), Some(Entry { sector: 106, offset: 293, sectors: 4 }));
        assert_eq!(Entry::decode(*b"****"), None);
    }
}
