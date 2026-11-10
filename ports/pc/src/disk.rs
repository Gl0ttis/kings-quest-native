//! The game disk: a 360K DOS-format disk (40 tracks, 2 sides, 9 sectors of 512 bytes,
//! FAT12), self-booting. Its files are the loader (`LOAD`), the engine (`MAIN.EXE`) and
//! `290K`, a file that spans the whole data area; the engine reads the resources from
//! there by sector number, through a table on sectors 10-11. On the original, track 6
//! side 0 holds an unreadable sector: the copy protection. See docs/disk-layout.md.
//!
//! Two kinds of image are read: the original as dumped (an HxC `.mfm`, see
//! kq1-formats' mfm), and the cracked 360K sector image (`.img`), whose loader and
//! engine are patched not to look for that sector. The resources and their table are
//! the same on both.

/// Bytes in a 360K image.
pub const IMAGE_LEN: usize = 368_640;
pub const SECTOR: usize = 512;

/// The boot sector's banner: this release's loader. (The CGA+RGB edition's says
/// `BOOT v1.2`.)
pub const BOOT_BANNER: &[u8] = b"BOOT v1.1 (c) Copyright Sierra On-Line, Inc. 1984";

/// The FAT12 layout of a 360K disk: two FATs of 2 sectors from sector 1, the root
/// directory's 7 sectors from 5 (112 entries), clusters of 2 sectors from sector 12.
const FAT: usize = 1;
const ROOT: usize = 5;
const ROOT_ENTRIES: usize = 112;
const DATA: usize = 12;
const CLUSTER: usize = 2;

/// The protection: on the original, track 6 side 0's first sector has an ID that says
/// 256 bytes and fails its CRC, so it can't be read (sector 108 in disk order).
pub const PROTECTION: (u8, u8, u8) = (6, 0, 1);

/// The resource table: 2 sectors from sector 10 (in the root directory's area, after
/// its used entries), which the engine reads to CS:`$88` at start-up (`$5A`, through
/// its read routine `$1FB6`). It holds 4-byte entries in the PCjr's format.
pub const TABLE_SECTOR: usize = 10;
pub const TABLE_ADDR: usize = 0x88;
pub const TABLE_LEN: usize = 2 * SECTOR;
/// Room `r`'s entry is at CS:`$88 + 4r`. Room 0 is the global script. The room
/// resource is the room's script followed by its picture.
pub const ROOM_TABLE: usize = TABLE_ADDR;
/// Rooms with an entry: 0-84.
pub const ROOMS: usize = 85;
/// Entries 90-99 of the same table hold the sounds, four to an entry.
pub const SOUND_FIRST: usize = 90;
pub const SOUNDS: usize = 10;
/// View `v`'s entry is at CS:`$288 + 4v` (entry 128 + v).
pub const VIEW_TABLE: usize = TABLE_ADDR + 0x200;
/// Views with an entry: 0-110.
pub const VIEWS: usize = 111;

/// `MAIN.EXE`'s header: 32 paragraphs. The engine runs where its first instruction
/// copies it, at 0060:0000, so engine address *a* is byte *a* of the image after the
/// header.
const EXE_HEADER: usize = 0x200;
/// The original engine's image ends with 773 bytes the crack's lacks: the code its
/// header starts at (CS:IP `071F:0000`), which then runs the rest from 0.
pub const ORIGINAL_ENGINE_LEN: usize = 0x74F0;
pub const CRACK_ENGINE_LEN: usize = 0x71EB;

/// The resource table's entries (crates/kq1-ibm, the PCjr's format).
pub use kq1_ibm::entry::Entry;

/// What this disk fills an unused table entry with (zeros).
pub const UNUSED_ENTRY: [u8; 4] = [0; 4];

/// A file in the root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub attributes: u8,
    pub cluster: usize,
    pub size: usize,
}

impl File {
    /// The file's first sector.
    pub fn sector(&self) -> usize {
        DATA + (self.cluster - 2) * CLUSTER
    }
}

pub struct Disk {
    /// The 512-byte sectors, as a 360K image (the protection's sector is zeros there).
    pub image: Vec<u8>,
    /// The engine: `MAIN.EXE`'s image, as it runs at 0060:0000.
    pub engine: Vec<u8>,
    /// Read from the original (an MFM dump) rather than the crack.
    pub original: bool,
}

impl Disk {
    /// A disk image file: an HxC MFM dump of the original, or a 360K sector image.
    pub fn from_file(raw: Vec<u8>) -> Result<Disk, String> {
        if raw.starts_with(b"HXCMFM\0") {
            let (tracks, bad) = kq1_formats::mfm::decode_with_errors(&raw)?;
            let protection = |b: &kq1_formats::mfm::BadSector| (b.cylinder, b.side, b.record) == PROTECTION;
            if let Some(b) = bad.iter().find(|b| !protection(b)) {
                return Err(format!(
                    "track {} side {} sector {} has a bad CRC (a damaged dump?)",
                    b.cylinder, b.side, b.record
                ));
            }
            if bad.is_empty() {
                return Err("no unreadable sector on track 6: not the original disk (a copy?)".into());
            }
            Disk::from_tracks(&tracks)
        } else {
            Disk::new(raw, false)
        }
    }

    /// The original's tracks: their 512-byte sectors into an image.
    pub fn from_tracks(tracks: &[kq1_formats::mfm::Track]) -> Result<Disk, String> {
        let mut image = vec![0u8; IMAGE_LEN];
        for t in tracks {
            for s in &t.sectors {
                if s.size == 2 && (1..=9).contains(&s.record) && t.cylinder < 40 && t.side < 2 {
                    let lin = (t.cylinder as usize * 2 + t.side as usize) * 9 + s.record as usize - 1;
                    image[lin * SECTOR..(lin + 1) * SECTOR].copy_from_slice(&s.data);
                }
            }
        }
        Disk::new(image, true)
    }

    /// A 360K sector image: the original's sectors, or the crack.
    pub fn new(image: Vec<u8>, original: bool) -> Result<Disk, String> {
        if image.len() != IMAGE_LEN {
            return Err(format!(
                "not a disk image of this game ({} bytes; expected an HxC .mfm dump or a {IMAGE_LEN}-byte .img)",
                image.len()
            ));
        }
        if image.get(3..3 + BOOT_BANNER.len()) != Some(BOOT_BANNER) {
            let edition = image.windows(9).take(SECTOR).any(|w| w == b"BOOT v1.2");
            return Err(if edition {
                "this is the CGA+RGB edition (BOOT v1.2), not the 1984 CGA release this port plays".into()
            } else {
                "not the King's Quest PC disk (its boot sector isn't Sierra's BOOT v1.1)".into()
            });
        }
        let mut disk = Disk { image, engine: Vec::new(), original };
        let main = disk.file("MAIN.EXE").ok_or("no MAIN.EXE on the disk")?;
        let exe = disk.read_file(&main)?;
        if exe.get(..2) != Some(b"MZ") || exe.len() <= EXE_HEADER {
            return Err("MAIN.EXE isn't a program".into());
        }
        disk.engine = exe[EXE_HEADER..].to_vec();
        let expected = if original { ORIGINAL_ENGINE_LEN } else { CRACK_ENGINE_LEN };
        if disk.engine.len() != expected {
            return Err(format!(
                "MAIN.EXE's program is {} bytes, not the {expected} of this release's {}",
                disk.engine.len(),
                if original { "original" } else { "crack" }
            ));
        }
        for (what, n, e) in disk.entries() {
            if e.start() >= IMAGE_LEN || (e.sector + e.sectors.max(1)) * SECTOR > IMAGE_LEN {
                return Err(format!("{what} {n}: entry {e:?} is outside the disk; not this game's disk?"));
            }
        }
        Ok(disk)
    }

    /// The root directory's files (deleted entries, volume labels and empty files left
    /// out).
    pub fn files(&self) -> Vec<File> {
        let dir = &self.image[ROOT * SECTOR..ROOT * SECTOR + ROOT_ENTRIES * 32];
        dir.chunks(32)
            .take_while(|e| e[0] != 0)
            .filter(|e| e[0] != 0xE5 && e[11] & 0x08 == 0)
            .filter_map(|e| {
                let cluster = u16::from_le_bytes([e[26], e[27]]) as usize;
                let size = u32::from_le_bytes(e[28..32].try_into().unwrap()) as usize;
                let part = |b: &[u8]| String::from_utf8_lossy(b).trim_end().to_string();
                let (stem, ext) = (part(&e[..8]), part(&e[8..11]));
                let name = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
                (cluster >= 2).then_some(File { name, attributes: e[11], cluster, size })
            })
            .collect()
    }

    pub fn file(&self, name: &str) -> Option<File> {
        self.files().into_iter().find(|f| f.name == name)
    }

    /// A file's bytes, through its FAT chain.
    pub fn read_file(&self, f: &File) -> Result<Vec<u8>, String> {
        let fat = &self.image[FAT * SECTOR..(FAT + 2) * SECTOR];
        let next = |c: usize| {
            let v = u16::from_le_bytes([fat[c * 3 / 2], fat[c * 3 / 2 + 1]]) as usize;
            if c & 1 == 1 {
                v >> 4
            } else {
                v & 0xFFF
            }
        };
        let mut out = Vec::with_capacity(f.size);
        let mut c = f.cluster;
        while out.len() < f.size {
            if !(2..0xFF0).contains(&c) {
                return Err(format!("{}: its clusters end early", f.name));
            }
            let s = DATA + (c - 2) * CLUSTER;
            out.extend_from_slice(self.image.get(s * SECTOR..(s + CLUSTER) * SECTOR).ok_or("cluster off the disk")?);
            c = next(c);
        }
        out.truncate(f.size);
        Ok(out)
    }

    /// The engine's code segment as it runs: the engine, with the resource table read
    /// over it at `$88`.
    pub fn code_segment(&self) -> Vec<u8> {
        let mut cs = self.engine.clone();
        let t = &self.image[TABLE_SECTOR * SECTOR..TABLE_SECTOR * SECTOR + TABLE_LEN];
        cs[TABLE_ADDR..TABLE_ADDR + TABLE_LEN].copy_from_slice(t);
        cs
    }

    fn entry_at(&self, n: usize) -> Option<Entry> {
        let at = TABLE_SECTOR * SECTOR + 4 * n;
        Entry::decode(self.image[at..at + 4].try_into().unwrap(), UNUSED_ENTRY)
    }

    /// Entry `n` of the table (rooms from 0, sounds from 90, views from 128).
    pub fn table_entry(&self, n: usize) -> Option<Entry> {
        (4 * n + 4 <= TABLE_LEN).then(|| self.entry_at(n)).flatten()
    }

    pub fn room_entry(&self, r: usize) -> Option<Entry> {
        (r < ROOMS).then(|| self.entry_at(r)).flatten()
    }

    pub fn sound_entry(&self, s: usize) -> Option<Entry> {
        (s < SOUNDS).then(|| self.entry_at(SOUND_FIRST + s)).flatten()
    }

    pub fn view_entry(&self, v: usize) -> Option<Entry> {
        (v < VIEWS).then(|| self.entry_at(128 + v)).flatten()
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
    /// of four section lengths and the sections; a length of 1 is an empty section.
    /// Room 0 (the global script) has no picture.
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
        // Room 1's entry: offset 280 in sector 166, 8 sectors.
        assert_eq!(
            Entry::decode([0x18, 0x80, 0x80, 0xa6], UNUSED_ENTRY),
            Some(Entry { sector: 166, offset: 280, sectors: 8 })
        );
        assert_eq!(Entry::decode([0; 4], UNUSED_ENTRY), None);
    }
}
