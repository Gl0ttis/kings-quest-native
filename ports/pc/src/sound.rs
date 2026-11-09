//! Sounds, in the PCjr's format (ports/pcjr/src/sound.rs, where the addresses below
//! are from): streams of writes to the PCjr's SN76496 sound chip, timed in ticks of the
//! 18.2 Hz BIOS timer. Seven of the PC's ten sound resources are the PCjr's byte for
//! byte; the PC has only its speaker, and how the engine plays them on it is still to
//! find.
//!
//! A sound resource holds four sounds. It is an 8-byte header of four `u16` sizes, then
//! the four streams. Sound *n* (from 1) is stream `(n − 1) mod 4` of resource
//! `(n − 1) / 4` (engine `$41D5`: room-table number 90 + (n − 1) / 4). A stream is a
//! list of records:
//!
//! | Bytes | Meaning |
//! |---|---|
//! | `k` | how many chip bytes follow |
//! | `k` bytes | written to port `C0h` in order (`$853`) |
//! | `t` | then wait `t` ticks; `0` ends the sound, `FF` starts it again (`$861`) |
//!
//! The `INT 1Ch` handler (`$808`) plays one record each time its countdown runs out.

/// How a sound ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// `0`: the sound stops (and the next queued one starts, `$879`).
    Stop,
    /// `FF`: the sound starts again from its first record (`$86A`).
    Repeat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Bytes written to the sound chip.
    pub writes: Vec<u8>,
    /// Ticks until the next record (none for the last).
    pub ticks: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    pub records: Vec<Record>,
    pub end: End,
}

impl Sound {
    /// The sound's length in ticks, once through.
    pub fn ticks(&self) -> u32 {
        self.records.iter().map(|r| r.ticks as u32).sum()
    }
}

/// Parse one stream. It must end with its end marker exactly at its size.
pub fn parse_stream(b: &[u8]) -> Result<Sound, String> {
    let mut records = Vec::new();
    let mut p = 0;
    loop {
        let k = *b.get(p).ok_or("stream ends without an end marker")? as usize;
        let writes = b.get(p + 1..p + 1 + k).ok_or("chip bytes run past the stream")?.to_vec();
        p += 1 + k;
        let t = *b.get(p).ok_or("stream ends without a duration")?;
        p += 1;
        let end = match t {
            0 => Some(End::Stop),
            0xFF => Some(End::Repeat),
            _ => None,
        };
        records.push(Record { writes, ticks: if end.is_some() { 0 } else { t } });
        if let Some(end) = end {
            if p != b.len() {
                return Err(format!("end marker at {p}, the stream is {} bytes", b.len()));
            }
            return Ok(Sound { records, end });
        }
    }
}

/// The four sounds in a sound resource. A section of size 0 (or the size-1 empty
/// marker the scripts use) has no sound.
pub fn parse(resource: &[u8]) -> Result<[Option<Sound>; 4], String> {
    let size = |i: usize| -> Result<usize, String> {
        let b = resource.get(2 * i..2 * i + 2).ok_or("sound header too short")?;
        Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
    };
    let mut at = 8;
    let mut out: [Option<Sound>; 4] = Default::default();
    for (i, slot) in out.iter_mut().enumerate() {
        let n = size(i)?;
        let data = resource.get(at..at + n).ok_or(format!("sound {i}: runs past the resource"))?;
        if n > 1 {
            *slot = Some(parse_stream(data).map_err(|e| format!("sound {i}: {e}"))?);
        }
        at += n;
    }
    Ok(out)
}
