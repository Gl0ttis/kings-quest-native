//! King's Quest (Apple II, 1984) resource layout, as established from the engine's
//! own loader code. Engine addresses in comments refer to the 6502 engine image
//! that boot stage 1 loads from disk 1 T11 S0..T14 S1 into $4000-$71FF.
//!
//! Resource addressing (engine $6037): a 3-byte record
//!   byte 0 = track
//!   byte 1 = (disk << 4) | sector   -- disk is matched against the RWTS volume
//!   byte 2 = byte offset within that sector
//! Disk 0 means "any disk": tracks 1-4 are byte-identical on disks 1-3.
//!
//! Resource body (engine $5E3F/$5EC7): a little-endian u16 length at the address,
//! followed by `length` bytes, streamed through consecutive DOS-logical sectors
//! (sector 0..15, then next track; engine $605E).

/// Byte offset of the engine image inside disk1.do (T11 S0), and its load address.
pub const ENGINE_DISK_OFFSET: usize = 11 * 16 * 256;
pub const ENGINE_LEN: usize = 50 * 256;
pub const ENGINE_BASE: u16 = 0x4000;

/// Fixed resources loaded at startup (engine $44F6-$45B8): (record address, destination).
pub const FIXED: [(u16, &str); 7] = [
    (0x6781, "aux_09C0_code"),
    (0x6784, "aux_1D00_font"),
    (0x6787, "main_0800_vocab"),
    (0x678A, "main_1500_table"),
    (0x678D, "main_1700_dirs"),
    (0x6790, "main_1A80_records"),
    (0x6793, "lc_FF00_code"),
];

/// Indexed directories inside the `main_1700_dirs` resource (engine $4318 lookups).
/// Names reflect probable roles; see docs/disk-layout.md for the evidence.
pub const DIRS: [(u16, &str); 3] = [(0x1700, "pic"), (0x1800, "logic"), (0x1900, "view")];

/// Entries per directory. The lookup is plain `base + 3n`, so a directory can run past
/// its page: the view directory continues into $1A00 and holds 120 entries, matching
/// the 120 view sizes in the $1500 table. Pictures and scripts use 85.
pub fn dir_entries(page: u16) -> usize {
    if page == 0x1900 {
        120
    } else {
        85
    }
}

/// Directory slots whose records decode but are not real resources.
/// pic 0: its span on disk 2 overlaps pic 71 and logic 70, and its data does not
/// begin with a picture opcode ($F0-$FF) like every other picture.
/// view 101: its record is identical to view 36's (same disk address), while the $1500
/// size table gives it a different size (2775 vs 3705). No script loads view 101.
pub const UNUSED: [(&str, usize); 2] = [("pic", 0), ("view", 101)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskAddr {
    pub disk: u8,
    pub track: u8,
    pub sector: u8,
    pub offset: u8,
}

impl DiskAddr {
    pub fn from_record(r: &[u8]) -> Self {
        DiskAddr { disk: r[1] >> 4, track: r[0], sector: r[1] & 0x0F, offset: r[2] }
    }
    pub fn linear(&self) -> usize {
        (self.track as usize * 16 + self.sector as usize) * 256 + self.offset as usize
    }
}

pub struct Disks {
    /// DOS-ordered 140K images for game disks 1..=3.
    pub images: Vec<Vec<u8>>,
}

#[derive(Debug)]
pub struct Resource<'a> {
    pub addr: DiskAddr,
    /// Disk the bytes were actually read from (resolves disk 0).
    pub disk: u8,
    /// Linear byte range on that disk, including the 2-byte length header.
    pub span: std::ops::Range<usize>,
    pub data: &'a [u8],
}

impl Disks {
    pub fn new(images: Vec<Vec<u8>>) -> Self {
        Disks { images }
    }

    /// The three game disks from image files: `.nib` (232,960 bytes, decoded here) or
    /// DOS-order `.do`/`.dsk` (143,360 bytes). Each `.nib` carries its disk number as
    /// its volume (1-3), so those may come in any order; the save disk (volume 10) is
    /// refused with a pointer to `--savedisk`.
    pub fn from_files<P: AsRef<std::path::Path>>(paths: &[P]) -> Result<Self, String> {
        let mut disks: Vec<(Option<u8>, Vec<u8>)> = Vec::new();
        for p in paths {
            let p = p.as_ref();
            let raw = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
            disks.push(Self::image(&raw).map_err(|e| format!("{}: {e}", p.display()))?);
        }
        if disks.len() != 3 {
            return Err(format!("expected the 3 game disks, got {}", disks.len()));
        }
        if disks.iter().all(|d| d.0.is_some()) {
            disks.sort_by_key(|d| d.0);
            let vols: Vec<u8> = disks.iter().map(|d| d.0.unwrap()).collect();
            if vols != [1, 2, 3] {
                return Err(format!("expected game disks 1, 2 and 3 (volumes), got volumes {vols:?}"));
            }
        }
        Ok(Disks { images: disks.into_iter().map(|d| d.1).collect() })
    }

    /// Find the three game disk images in a folder: `.nib` files by the disk number they
    /// record (volume 1-3; the save disk and damaged images are passed over), `.do`/`.dsk`
    /// files by the disk number in their name ("disk 1", "side A", or the first digit
    /// 1-3). Returns the paths in disk order, or why they couldn't all be found.
    pub fn find(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut found: [Option<std::path::PathBuf>; 3] = Default::default();
        let mut files: Vec<std::path::PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
        files.sort();
        // .nib first: their disk numbers are certain.
        for nib in [true, false] {
            for p in &files {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                let n = match ext.as_str() {
                    "nib" if nib => std::fs::read(p).ok().and_then(|raw| Self::image(&raw).ok()?.0),
                    "do" | "dsk" if !nib => p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(disk_number)
                        .filter(|_| std::fs::metadata(p).is_ok_and(|m| m.len() == 143_360)),
                    _ => None,
                };
                if let Some(slot) = n.filter(|n| (1..=3).contains(n)).map(|n| &mut found[n as usize - 1]) {
                    slot.get_or_insert_with(|| p.clone());
                }
            }
        }
        let missing: Vec<String> = (1..=3).filter(|&i| found[i - 1].is_none()).map(|i| i.to_string()).collect();
        match missing.len() {
            0 => {}
            3 => return Err(format!("no game disks in {}", dir.display())),
            _ => return Err(format!("game disk {} not found in {}", missing.join(" and "), dir.display())),
        }
        Ok(found.into_iter().map(|p| p.unwrap()).collect())
    }

    /// One image as DOS order, with its volume number when the image records it.
    fn image(raw: &[u8]) -> Result<(Option<u8>, Vec<u8>), String> {
        match raw.len() {
            crate::nib::NIB_LEN => {
                let tracks = crate::nib::decode_image(raw)?;
                let bad: usize = tracks
                    .iter()
                    .map(|t| t.sectors.iter().filter(|s| !s.data_checksum_ok).count() + (16 - t.sectors.len().min(16)))
                    .sum();
                if bad > 0 {
                    return Err(format!("{bad} sectors are missing or damaged"));
                }
                let vol = tracks[0].sectors.first().map(|s| s.volume);
                if vol == Some(10) {
                    return Err("this is the save disk (volume 10); give it with --savedisk".into());
                }
                Ok((vol, crate::nib::to_ordered(&tracks, &crate::nib::DOS_TO_PHYS)))
            }
            143_360 => Ok((None, raw.to_vec())),
            n => Err(format!(
                "not a 5.25-inch disk image ({n} bytes; expected a .nib of 232,960 or a .do/.dsk of 143,360)"
            )),
        }
    }

    pub fn engine(&self) -> &[u8] {
        &self.images[0][ENGINE_DISK_OFFSET..ENGINE_DISK_OFFSET + ENGINE_LEN]
    }

    pub fn engine_bytes(&self, addr: u16, n: usize) -> &[u8] {
        let o = (addr - ENGINE_BASE) as usize;
        &self.engine()[o..o + n]
    }

    /// Read a resource. Disk 0 is resolved to `current` (any disk works: shared tracks).
    pub fn read(&self, addr: DiskAddr, current: u8) -> Result<Resource<'_>, String> {
        let disk = if addr.disk == 0 { current } else { addr.disk };
        if disk == 0 || disk as usize > self.images.len() {
            return Err(format!("disk {} out of range", addr.disk));
        }
        if addr.track >= 35 {
            return Err(format!("track {} out of range", addr.track));
        }
        let img = &self.images[disk as usize - 1];
        let p = addr.linear();
        let len = u16::from_le_bytes([img[p], img[p + 1]]) as usize;
        let end = p + 2 + len;
        if len == 0 || end > img.len() {
            return Err(format!("length {len} invalid at {addr:?}"));
        }
        Ok(Resource { addr, disk, span: p..end, data: &img[p + 2..end] })
    }

    pub fn fixed(&self, record_addr: u16) -> Result<Resource<'_>, String> {
        self.read(DiskAddr::from_record(self.engine_bytes(record_addr, 3)), 1)
    }

    /// Raw 3-byte records of one directory ($1700/$1800/$1900).
    pub fn directory(&self, page: u16) -> Result<Vec<[u8; 3]>, String> {
        let dirs = self.fixed(0x678D)?;
        let o = (page - 0x1700) as usize;
        let n = dir_entries(page);
        let tb = dirs.data.get(o..o + n * 3).ok_or("directory resource too short")?;
        Ok((0..n).map(|i| [tb[i * 3], tb[i * 3 + 1], tb[i * 3 + 2]]).collect())
    }
}

/// The disk number in an image's file name: after "disk" or "side" (1-3 or A-C), else
/// the first digit 1-3 in the name.
fn disk_number(name: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    for word in ["disk", "side"] {
        if let Some(i) = name.find(word) {
            let c = name[i + word.len()..].chars().find(|c| c.is_ascii_alphanumeric())?;
            return match c {
                '1'..='3' => Some(c as u8 - b'0'),
                'a'..='c' => Some(c as u8 - b'a' + 1),
                _ => None,
            };
        }
    }
    name.chars().find(|c| c.is_ascii_digit()).filter(|c| ('1'..='3').contains(c)).map(|c| c as u8 - b'0')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn record_decoding() {
        let a = DiskAddr::from_record(&[18, 0x16, 0x00]);
        assert_eq!(a, DiskAddr { disk: 1, track: 18, sector: 6, offset: 0 });
        assert_eq!(a.linear(), (18 * 16 + 6) * 256);
    }

    #[test]
    fn disk_numbers_from_names() {
        assert_eq!(disk_number("King's Quest - Disk 2 of 3"), Some(2));
        assert_eq!(disk_number("kq1 side C"), Some(3));
        assert_eq!(disk_number("kq-3"), Some(3));
        assert_eq!(disk_number("disk4"), None);
        assert_eq!(disk_number("saves"), None);
    }
}
