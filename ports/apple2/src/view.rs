//! View (sprite) resources: the `$1900` directory. Layout from the aux routines that
//! select loops and cels ($0A55) and draw them ($10EF/$1124/$1194).
//!
//! View:  [0] loop count (copied to actor field 6), then a u16 offset per loop (from view start)
//! Loop:  [0..4] header (bytes 0-2 copied to actor $0C/$0D/$09; byte 3 = cel count),
//!        then a u16 offset per cel (from loop start)
//! Cel:   [0] height, [1] width, then a nibble stream (high nibble first, continuous
//!        across rows):
//!          1-7  run of n pixels in the colour given by the next nibble
//!          8    end of row
//!          9-15 skip (n & 7) transparent pixels
//! Colour nibbles are written directly as the buffer's visual value (Apple DHGR colour,
//! the same space as picture colours after the picture colour table). The bottom row is
//! drawn at the actor's y; the top row is y - height + 1 ($162B).

pub struct Cel {
    pub width: u8,
    pub height: u8,
    /// Row-major, width * height; None = transparent.
    pub pixels: Vec<Option<u8>>,
    /// Nibbles in the encoded stream: the work the engine's decoder does per draw.
    pub nibbles: u32,
}

pub struct Loop {
    pub header: [u8; 4],
    pub cels: Vec<Cel>,
}

pub struct View {
    pub loops: Vec<Loop>,
}

fn u16_at(b: &[u8], o: usize) -> Result<usize, String> {
    Ok(*b.get(o).ok_or("offset out of range")? as usize | (*b.get(o + 1).ok_or("offset out of range")? as usize) << 8)
}

pub fn decode_cel(b: &[u8], at: usize) -> Result<Cel, String> {
    let height = *b.get(at).ok_or("cel header out of range")?;
    let width = *b.get(at + 1).ok_or("cel header out of range")?;
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![None; w * h];
    let start = (at + 2) * 2;
    let mut pos = start; // nibble index
    let mut next = || -> Result<u8, String> {
        let byte = *b.get(pos / 2).ok_or("cel data runs past end of view")?;
        let n = if pos.is_multiple_of(2) { byte >> 4 } else { byte & 0x0F };
        pos += 1;
        Ok(n)
    };
    for row in 0..h {
        let mut x = 0usize;
        loop {
            let n = next()?;
            match n {
                8 => break,
                9..=15 => x += (n & 7) as usize,
                0 => return Err(format!("zero-length run in row {row}")),
                _ => {
                    let c = next()?;
                    for _ in 0..n {
                        if x >= w {
                            return Err(format!("row {row} wider than cel width {w}"));
                        }
                        pixels[row * w + x] = Some(c);
                        x += 1;
                    }
                }
            }
        }
    }
    drop(next);
    Ok(Cel { width, height, pixels, nibbles: (pos - start) as u32 })
}

pub fn decode(b: &[u8]) -> Result<View, String> {
    let nloops = *b.first().ok_or("empty view")? as usize;
    let mut loops = Vec::with_capacity(nloops);
    for l in 0..nloops {
        let lo = u16_at(b, 1 + 2 * l)?;
        let header: [u8; 4] = b.get(lo..lo + 4).ok_or("loop header out of range")?.try_into().unwrap();
        let mut cels = Vec::new();
        for c in 0..header[3] as usize {
            let co = lo + u16_at(b, lo + 4 + 2 * c)?;
            cels.push(decode_cel(b, co).map_err(|e| format!("loop {l} cel {c}: {e}"))?);
        }
        loops.push(Loop { header, cels });
    }
    Ok(View { loops })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cel_nibble_stream() {
        // 2x3 cel: row0 "skip1, 1px colour 4", row1 "2px colour 15", row2 empty.
        // nibbles: 9 1 4 8 | 2 F 8 | 8 (pad 0)
        let b = [3, 2, 0x91, 0x48, 0x2F, 0x88];
        let c = decode_cel(&b, 0).unwrap();
        assert_eq!(c.pixels, vec![None, Some(4), Some(15), Some(15), None, None]);
    }
}
