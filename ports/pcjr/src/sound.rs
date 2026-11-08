//! PCjr sounds: streams of writes to the SN76496 sound chip (port `C0h`), timed in
//! ticks of the 18.2 Hz BIOS timer (docs/disk-layout.md, "Sounds").
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

/// The PCjr's sound chip clock: the 14.318 MHz crystal / 4.
pub const CHIP_HZ: f64 = 3_579_545.0;
/// The BIOS timer tick rate (1,193,182 Hz / 65,536).
pub const TICK_HZ: f64 = 18.206_5;

/// The SN76496's state: three tone channels and a noise channel, each with a 4-bit
/// attenuation (2 dB steps; 15 is off). Bytes arrive as the chip takes them: a byte with
/// bit 7 set latches a register (channel in bits 6-5, volume if bit 4) and sets its low
/// 4 bits; a byte without it sets a tone's upper 6 bits.
#[derive(Debug, Clone)]
pub struct Chip {
    tone: [u16; 3],
    atten: [u8; 4],
    noise: u8,
    latched: usize,
    // Synthesis state.
    count: [f64; 4],
    out: [bool; 4],
    lfsr: u16,
}

impl Default for Chip {
    fn default() -> Self {
        Chip { tone: [0; 3], atten: [15; 4], noise: 0, latched: 0, count: [0.0; 4], out: [false; 4], lfsr: 0x4000 }
    }
}

impl Chip {
    pub fn write(&mut self, b: u8) {
        if b & 0x80 != 0 {
            let ch = (b >> 5 & 3) as usize;
            let vol = b & 0x10 != 0;
            self.latched = ch * 2 + vol as usize;
            if vol {
                self.atten[ch] = b & 0x0F;
            } else if ch == 3 {
                self.noise = b & 0x07;
                self.lfsr = 0x4000;
            } else {
                self.tone[ch] = (self.tone[ch] & 0x3F0) | (b & 0x0F) as u16;
            }
        } else if self.latched.is_multiple_of(2) && self.latched < 6 {
            let ch = self.latched / 2;
            self.tone[ch] = (self.tone[ch] & 0x0F) | ((b & 0x3F) as u16) << 4;
        }
    }

    /// One output sample at `rate` Hz, in -1..1.
    pub fn sample(&mut self, rate: f64) -> f32 {
        // The chip counts down at clock / 16; a tone flips each time its count runs out.
        let step = CHIP_HZ / 16.0 / rate;
        let mut mix = 0.0f32;
        for ch in 0..4 {
            let period = if ch < 3 {
                self.tone[ch].max(1) as f64
            } else {
                match self.noise & 3 {
                    0 => 16.0,
                    1 => 32.0,
                    2 => 64.0,
                    _ => self.tone[2].max(1) as f64,
                }
            };
            self.count[ch] -= step;
            while self.count[ch] <= 0.0 {
                self.count[ch] += period;
                if ch < 3 {
                    self.out[ch] = !self.out[ch];
                } else {
                    // 15-bit shift register; white noise feeds back bits 0 and 1.
                    let fb = if self.noise & 4 != 0 { (self.lfsr ^ self.lfsr >> 1) & 1 } else { self.lfsr & 1 };
                    self.lfsr = self.lfsr >> 1 | fb << 14;
                    self.out[3] = self.lfsr & 1 != 0;
                }
            }
            let level = if self.atten[ch] >= 15 { 0.0 } else { 10f32.powf(-0.1 * self.atten[ch] as f32) };
            // A tone register of 0 or 1 holds the output high (no audible tone).
            let high = if ch < 3 && self.tone[ch] <= 1 { true } else { self.out[ch] };
            mix += if high { level } else { -level } * 0.25;
        }
        mix
    }
}

/// A sound as 16-bit samples at `rate` Hz, played once (a repeating sound is played
/// through once, a stopping one to its end).
pub fn render(sound: &Sound, rate: u32) -> Vec<i16> {
    let mut chip = Chip::default();
    let mut out = Vec::new();
    for r in &sound.records {
        for &b in &r.writes {
            chip.write(b);
        }
        let n = (r.ticks as f64 / TICK_HZ * rate as f64).round() as usize;
        out.extend((0..n).map(|_| (chip.sample(rate as f64) * 32767.0) as i16));
    }
    out
}

/// A mono 16-bit WAV file.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}
