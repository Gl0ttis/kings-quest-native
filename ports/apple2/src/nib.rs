//! Decoder for 232,960-byte `.nib` images (35 tracks x 6,656 raw nibbles).

pub const TRACKS: usize = 35;
pub const TRACK_LEN: usize = 6656;
pub const NIB_LEN: usize = TRACKS * TRACK_LEN;
pub const SECTORS: usize = 16;

const WRITE_TABLE: [u8; 64] = [
    0x96, 0x97, 0x9A, 0x9B, 0x9D, 0x9E, 0x9F, 0xA6, 0xA7, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
    0xB7, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xCB, 0xCD, 0xCE, 0xCF, 0xD3, 0xD6, 0xD7, 0xD9, 0xDA, 0xDB, 0xDC,
    0xDD, 0xDE, 0xDF, 0xE5, 0xE6, 0xE7, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
    0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

fn read_table() -> [i16; 256] {
    let mut t = [-1i16; 256];
    for (i, &b) in WRITE_TABLE.iter().enumerate() {
        t[b as usize] = i as i16;
    }
    t
}

#[derive(Debug, Clone)]
pub struct Sector {
    pub volume: u8,
    pub track: u8,
    /// Physical sector number as written in the address field.
    pub sector: u8,
    pub addr_checksum_ok: bool,
    pub data_checksum_ok: bool,
    pub data: [u8; 256],
}

#[derive(Debug)]
pub struct TrackDecode {
    pub index: usize,
    pub sectors: Vec<Sector>,
    pub problems: Vec<String>,
}

/// Circular byte reader over one raw track.
struct Ring<'a> {
    t: &'a [u8],
}
impl Ring<'_> {
    fn at(&self, i: usize) -> u8 {
        self.t[i % self.t.len()]
    }
}

fn odd_even(hi: u8, lo: u8) -> u8 {
    ((hi << 1) | 1) & lo
}

pub fn decode_track(index: usize, raw: &[u8]) -> TrackDecode {
    let rt = read_table();
    let ring = Ring { t: raw };
    let n = raw.len();
    let mut sectors = Vec::new();
    let mut problems = Vec::new();
    let mut seen = [false; SECTORS];

    let mut i = 0;
    while i < n {
        if !(ring.at(i) == 0xD5 && ring.at(i + 1) == 0xAA && ring.at(i + 2) == 0x96) {
            i += 1;
            continue;
        }
        let a = i + 3;
        let f = |k: usize| odd_even(ring.at(a + 2 * k), ring.at(a + 2 * k + 1));
        let (vol, trk, sec, chk) = (f(0), f(1), f(2), f(3));
        let addr_ok = vol ^ trk ^ sec == chk;
        // Data prologue must follow within a short gap.
        let mut d = a + 8;
        let limit = d + 64;
        while d < limit && !(ring.at(d) == 0xD5 && ring.at(d + 1) == 0xAA && ring.at(d + 2) == 0xAD) {
            d += 1;
        }
        if d >= limit {
            problems.push(format!("T{index:02} S{sec:X}: no data field after address field"));
            i = a + 8;
            continue;
        }
        let d0 = d + 3;
        let mut buf = [0u8; 343];
        let mut bad_nibble = false;
        let mut prev = 0u8;
        for k in 0..343 {
            let v = rt[ring.at(d0 + k) as usize];
            if v < 0 {
                bad_nibble = true;
                break;
            }
            prev ^= v as u8;
            buf[k] = prev;
        }
        // After XOR chaining, buf[342] equals the running checksum which must be 0.
        let data_ok = !bad_nibble && buf[342] == 0;
        let mut out = [0u8; 256];
        for k in 0..256 {
            let six = buf[86 + k] << 2;
            let aux = buf[k % 86] >> (2 * (k / 86));
            let two = ((aux & 1) << 1) | ((aux >> 1) & 1);
            out[k] = six | two;
        }
        if (sec as usize) < SECTORS {
            if seen[sec as usize] {
                // Wrapped around to sectors already decoded.
                break;
            }
            seen[sec as usize] = true;
        }
        if trk as usize != index {
            problems.push(format!("T{index:02} S{sec:X}: address field claims track {trk}"));
        }
        if !addr_ok {
            problems.push(format!("T{index:02} S{sec:X}: address checksum mismatch"));
        }
        if !data_ok {
            problems.push(format!("T{index:02} S{sec:X}: data checksum mismatch"));
        }
        sectors.push(Sector {
            volume: vol,
            track: trk,
            sector: sec,
            addr_checksum_ok: addr_ok,
            data_checksum_ok: data_ok,
            data: out,
        });
        i = d0 + 343;
        if i >= n + 3 {
            break;
        }
    }
    for (s, ok) in seen.iter().enumerate() {
        if !ok {
            problems.push(format!("T{index:02} S{s:X}: physical sector missing"));
        }
    }
    TrackDecode { index, sectors, problems }
}

pub fn decode_image(raw: &[u8]) -> Result<Vec<TrackDecode>, String> {
    if raw.len() != NIB_LEN {
        return Err(format!("expected {NIB_LEN} bytes, got {}", raw.len()));
    }
    Ok((0..TRACKS).map(|t| decode_track(t, &raw[t * TRACK_LEN..(t + 1) * TRACK_LEN])).collect())
}

/// DOS 3.3 logical sector -> physical sector.
pub const DOS_TO_PHYS: [usize; 16] = [0, 13, 11, 9, 7, 5, 3, 1, 14, 12, 10, 8, 6, 4, 2, 15];
/// ProDOS block-half order -> physical sector.
pub const PRODOS_TO_PHYS: [usize; 16] = [0, 2, 4, 6, 8, 10, 12, 14, 1, 3, 5, 7, 9, 11, 13, 15];

/// Build a 143,360-byte image in the given logical order.
pub fn to_ordered(tracks: &[TrackDecode], map: &[usize; 16]) -> Vec<u8> {
    let mut img = vec![0u8; TRACKS * SECTORS * 256];
    for t in tracks {
        for s in &t.sectors {
            if let Some(logical) = map.iter().position(|&p| p == s.sector as usize) {
                let off = (t.index * SECTORS + logical) * 256;
                img[off..off + 256].copy_from_slice(&s.data);
            }
        }
    }
    img
}

/// 6-and-2 encode a sector's data field: 342 nibbles and the checksum (343).
pub fn encode(data: &[u8; 256]) -> Vec<u8> {
    let mut buf = [0u8; 342];
    for k in 0..256 {
        let b = data[k];
        let two = ((b & 1) << 1) | ((b >> 1) & 1);
        buf[k % 86] |= two << (2 * (k / 86));
        buf[86 + k] = b >> 2;
    }
    let mut out = Vec::with_capacity(343);
    let mut prev = 0u8;
    for &v in &buf {
        out.push(WRITE_TABLE[(v ^ prev) as usize]);
        prev = v;
    }
    out.push(WRITE_TABLE[prev as usize]);
    out
}

/// Where the data field of physical sector `sector` starts in a raw track (the first
/// nibble after D5 AA AD; the track is circular, so later nibbles may wrap).
pub fn data_field(raw: &[u8], sector: u8) -> Option<usize> {
    let ring = Ring { t: raw };
    let n = raw.len();
    for i in 0..n {
        if !(ring.at(i) == 0xD5 && ring.at(i + 1) == 0xAA && ring.at(i + 2) == 0x96) {
            continue;
        }
        let a = i + 3;
        if odd_even(ring.at(a + 4), ring.at(a + 5)) != sector {
            continue;
        }
        let mut d = a + 8;
        while d < a + 72 {
            if ring.at(d) == 0xD5 && ring.at(d + 1) == 0xAA && ring.at(d + 2) == 0xAD {
                return Some((d + 3) % n);
            }
            d += 1;
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_6and2() {
        let mut data = [0u8; 256];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let mut track = vec![0xFFu8; 40];
        for s in 0..16u8 {
            let enc44 = |v: u8| [(v >> 1) | 0xAA, v | 0xAA];
            track.extend_from_slice(&[0xD5, 0xAA, 0x96]);
            for v in [254u8, 3, s, 254 ^ 3 ^ s] {
                track.extend_from_slice(&enc44(v));
            }
            track.extend_from_slice(&[0xDE, 0xAA, 0xEB, 0xFF, 0xFF, 0xFF, 0xD5, 0xAA, 0xAD]);
            track.extend(encode(&data));
            track.extend_from_slice(&[0xDE, 0xAA, 0xEB]);
            track.extend(std::iter::repeat_n(0xFF, 20));
        }
        let td = decode_track(3, &track);
        assert!(td.problems.is_empty(), "{:?}", td.problems);
        assert_eq!(td.sectors.len(), 16);
        for s in &td.sectors {
            assert_eq!(s.volume, 254);
            assert_eq!(s.data, data);
        }
    }
}
