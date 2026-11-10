//! The IBM PC and PCjr views (the PC release's are byte for byte the PCjr's). The cels
//! are encoded as in the Apple version (ports/apple2/src/view.rs,
//! `decode_cel`), but the tables hold sizes, not offsets:
//!
//! | Part | Layout |
//! |---|---|
//! | View | `u16` total length; loop count; 8 `u16` loop sizes (unused slots 0) |
//! | Loop | 4-byte header (x step, y step, delay, cel count); a `u16` size per cel |
//! | Cel | height, width, then the nibble stream |
//!
//! Each loop and cel starts where the one before it ends.

pub use kq1_formats::view::{Cel, Loop, View};

/// Loop slots in the table, used or not.
pub const LOOP_SLOTS: usize = 8;

/// Decode a view (with its length prefix). Every cel must decode, and end within its
/// listed size; every loop must end where its listed size says.
pub fn decode(b: &[u8]) -> Result<View, String> {
    let u16_at = |o: usize| -> Result<usize, String> {
        Ok(u16::from_le_bytes([*b.get(o).ok_or("table out of range")?, *b.get(o + 1).ok_or("table out of range")?])
            as usize)
    };
    let nloops = *b.get(2).ok_or("view too short")? as usize;
    if nloops > LOOP_SLOTS {
        return Err(format!("{nloops} loops; there are {LOOP_SLOTS} slots"));
    }
    let mut at = 3 + 2 * LOOP_SLOTS;
    let mut loops = Vec::with_capacity(nloops);
    for l in 0..nloops {
        let size = u16_at(3 + 2 * l)?;
        let end = at + size;
        let header: [u8; 4] = b.get(at..at + 4).ok_or(format!("loop {l} header out of range"))?.try_into().unwrap();
        let ncels = header[3] as usize;
        let mut c = at + 4 + 2 * ncels;
        let mut cels = Vec::with_capacity(ncels);
        for k in 0..ncels {
            let csize = u16_at(at + 4 + 2 * k)?;
            let cel = kq1_formats::view::decode_cel(b, c).map_err(|e| format!("loop {l} cel {k}: {e}"))?;
            let used = 2 + (cel.nibbles as usize).div_ceil(2);
            if used > csize {
                return Err(format!("loop {l} cel {k}: {used} bytes of data, listed as {csize}"));
            }
            cels.push(cel);
            c += csize;
        }
        if c != end {
            return Err(format!("loop {l}: cels end at {c}, the loop at {end}"));
        }
        loops.push(Loop { header, cels });
        at = end;
    }
    Ok(View { loops })
}
