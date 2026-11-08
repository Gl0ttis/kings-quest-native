//! Minimal DOS 3.3 VTOC / catalog reader over a DOS-ordered 140K image.

pub fn sector(img: &[u8], t: u8, s: u8) -> &[u8] {
    let o = (t as usize * 16 + s as usize) * 256;
    &img[o..o + 256]
}

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub kind: char,
    pub locked: bool,
    pub ts_track: u8,
    pub ts_sector: u8,
    pub sectors: u16,
}

/// Returns None unless T17 S0 is a DOS 3.3 VTOC *and* the catalog chain is well formed.
pub fn catalog(img: &[u8]) -> Option<Vec<Entry>> {
    let vtoc = sector(img, 17, 0);
    if vtoc[0x27] != 122 || vtoc[0x34] != 35 || vtoc[0x35] != 16 || vtoc[1] >= 35 {
        return None;
    }
    let (mut t, mut s) = (vtoc[1], vtoc[2]);
    let mut out = Vec::new();
    let mut guard = 0;
    while t != 0 && t < 35 && s < 16 && guard < 64 {
        guard += 1;
        let c = sector(img, t, s);
        if c[1] >= 35 || c[2] >= 16 {
            return None;
        }
        for e in 0..7 {
            let b = &c[0x0B + e * 35..0x0B + (e + 1) * 35];
            if b[0] == 0 || b[0] == 0xFF {
                continue;
            }
            // A real entry has an in-range T/S list and a high-bit ASCII name.
            if b[0] >= 35 || b[1] >= 16 || !b[3..33].iter().all(|&c| (0xA0..0xFF).contains(&c)) {
                return None;
            }
            let ty = b[2];
            let kind = match ty & 0x7F {
                0x00 => 'T',
                0x01 => 'I',
                0x02 => 'A',
                0x04 => 'B',
                0x08 => 'S',
                0x10 => 'R',
                0x20 => 'a',
                0x40 => 'b',
                _ => '?',
            };
            let name: String = b[3..33].iter().map(|&c| (c & 0x7F) as char).collect::<String>().trim_end().to_string();
            out.push(Entry {
                name,
                kind,
                locked: ty & 0x80 != 0,
                ts_track: b[0],
                ts_sector: b[1],
                sectors: u16::from_le_bytes([b[33], b[34]]),
            });
        }
        t = c[1];
        s = c[2];
    }
    Some(out)
}

/// Follow a file's track/sector list and concatenate its data sectors.
pub fn read_file(img: &[u8], e: &Entry) -> Vec<u8> {
    let mut data = Vec::new();
    let (mut t, mut s) = (e.ts_track, e.ts_sector);
    let mut guard = 0;
    while t != 0 && t < 35 && s < 16 && guard < 128 {
        guard += 1;
        let ts = sector(img, t, s);
        for p in 0..122 {
            let (dt, ds) = (ts[0x0C + 2 * p], ts[0x0D + 2 * p]);
            if dt == 0 && ds == 0 {
                data.extend_from_slice(&[0u8; 256]);
            } else if dt < 35 && ds < 16 {
                data.extend_from_slice(sector(img, dt, ds));
            }
        }
        t = ts[1];
        s = ts[2];
    }
    // Trim trailing sparse padding.
    while data.len() >= 256 && data[data.len() - 256..].iter().all(|&b| b == 0) {
        data.truncate(data.len() - 256);
    }
    data
}
