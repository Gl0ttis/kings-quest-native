//! HxC MFM images (`.mfm`): each track's raw MFM bitstream, as a preservation dump of
//! the original disk keeps it. Decoded here to IBM sectors, with their CRCs checked,
//! the way a PC's or PCjr's disk controller reads them. A sector whose CRC fails isn't
//! returned: [`decode`] refuses the image, and [`decode_with_errors`] lists them (a
//! copy protection can be a sector made unreadable on purpose).
//!
//! File layout: "HXCMFM\0", u16 tracks, u8 sides, u16 rpm, u16 bit rate (kbit/s), u8
//! interface type, u32 offset of the track list; then per track: u16 track, u8 side,
//! u32 size and u32 offset of its bitstream (bits most significant first).

/// A sector as read from a track: its ID field (cylinder, head, record, size code) and
/// its data.
#[derive(Debug, Clone)]
pub struct Sector {
    pub cylinder: u8,
    pub head: u8,
    pub record: u8,
    /// Size code: the data is `128 << size` bytes.
    pub size: u8,
    pub data: Vec<u8>,
}

/// One track side's sectors, in the order they pass the head.
pub struct Track {
    pub cylinder: u8,
    pub side: u8,
    pub sectors: Vec<Sector>,
}

/// A sector the controller can't read: its ID field's CRC fails (`record` and `size`
/// are then as recorded, unchecked), or its data's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadSector {
    pub cylinder: u8,
    pub side: u8,
    pub record: u8,
    pub size: u8,
    /// The data field's CRC failed, not the ID's.
    pub data: bool,
}

/// Decode every track of an HxC MFM image. Sectors whose ID or data CRC fails are left
/// out, and counted in the error if any track has one.
pub fn decode(file: &[u8]) -> Result<Vec<Track>, String> {
    let (tracks, bad) = decode_with_errors(file)?;
    if !bad.is_empty() {
        return Err(format!("{} sectors have a bad CRC (a damaged dump?)", bad.len()));
    }
    Ok(tracks)
}

/// Decode every track of an HxC MFM image, with the sectors whose ID or data CRC fails
/// left out of the tracks and listed apart.
pub fn decode_with_errors(file: &[u8]) -> Result<(Vec<Track>, Vec<BadSector>), String> {
    if file.len() < 19 || &file[..7] != b"HXCMFM\0" {
        return Err("not an HxC MFM image".into());
    }
    let u16_at = |o: usize| u16::from_le_bytes([file[o], file[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap()) as usize;
    let (tracks, sides, list) = (u16_at(7), file[9] as usize, u32_at(15));
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for i in 0..tracks * sides {
        let e = list + 11 * i;
        let entry = file.get(e..e + 11).ok_or("track list runs past the end")?;
        let (cyl, side) = (entry[0], entry[2]);
        let (size, off) = (
            u32::from_le_bytes(entry[3..7].try_into().unwrap()) as usize,
            u32::from_le_bytes(entry[7..11].try_into().unwrap()) as usize,
        );
        let raw = file.get(off..off + size).ok_or(format!("track {cyl} side {side} runs past the end"))?;
        let (sectors, b) = track_sectors(raw);
        bad.extend(b.into_iter().map(|(record, size, data)| BadSector { cylinder: cyl, side, record, size, data }));
        out.push(Track { cylinder: cyl, side, sectors });
    }
    Ok((out, bad))
}

/// The sectors on one track's bitstream, and those that failed their CRC (record, size
/// code, and whether it was the data's CRC), each once.
fn track_sectors(raw: &[u8]) -> (Vec<Sector>, Vec<(u8, u8, bool)>) {
    let n = raw.len() * 8;
    // Read past the end into the start again: a sector can straddle the index.
    let bit = |i: usize| raw[(i % n) / 8] >> (7 - (i % n) % 8) & 1;
    // A data byte from 16 cells starting at bit i: the data bits are the odd cells.
    let byte = |i: usize| (0..8).fold(0u8, |v, k| v << 1 | bit(i + 2 * k + 1));
    // A1 with a missing clock bit, three times: the address mark sync.
    const SYNC: u16 = 0x4489;
    let mut sectors = Vec::new();
    let mut bad = Vec::new();
    let mut id: Option<[u8; 4]> = None;
    let mut window = 0u64;
    let mut i = 0;
    while i < n {
        window = window << 1 | bit(i) as u64;
        i += 1;
        if window & 0xFFFF_FFFF_FFFF != (SYNC as u64) << 32 | (SYNC as u64) << 16 | SYNC as u64 {
            continue;
        }
        let mark = byte(i);
        let read = |at: usize, len: usize| -> Vec<u8> { (0..len).map(|k| byte(at + 16 * k)).collect() };
        match mark {
            0xFE => {
                let f = read(i, 7);
                id = if crc(&f[..5]) == u16::from_be_bytes([f[5], f[6]]) {
                    Some([f[1], f[2], f[3], f[4]])
                } else {
                    bad.push((f[3], f[4], false));
                    None
                };
                i += 16 * 7;
            }
            0xFB | 0xF8 => {
                if let Some([c, h, r, s]) = id.take() {
                    let len = 128usize << (s & 7);
                    let f = read(i, 1 + len + 2);
                    if crc(&f[..1 + len]) == u16::from_be_bytes([f[1 + len], f[2 + len]]) {
                        sectors.push(Sector { cylinder: c, head: h, record: r, size: s, data: f[1..1 + len].to_vec() });
                    } else {
                        bad.push((r, s, true));
                    }
                    i += 16 * (3 + len);
                }
            }
            _ => {}
        }
        window = 0;
    }
    // Sectors found again after the wrap-around are duplicates.
    let mut seen = std::collections::HashSet::new();
    sectors.retain(|s| seen.insert((s.record, s.size)));
    let mut seen = std::collections::HashSet::new();
    bad.retain(|b| seen.insert(*b));
    (sectors, bad)
}

/// CRC-CCITT over the three sync bytes (`A1 A1 A1`) and `data`, as the controller
/// computes it.
fn crc(data: &[u8]) -> u16 {
    let mut c = 0xFFFFu16;
    for &b in [0xA1, 0xA1, 0xA1].iter().chain(data) {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { c << 1 ^ 0x1021 } else { c << 1 };
        }
    }
    c
}
