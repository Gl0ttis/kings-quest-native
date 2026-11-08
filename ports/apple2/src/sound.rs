//! The game's only sound: the Apple ROM bell, reached through `$6618` (one beep) and
//! `$6625` (two beeps with a short pause, for save and restore disk errors).
//!
//! The bell (`$FBDD`) waits 0.01 s (`WAIT` with A = `$40`), then toggles the speaker 192
//! times, calling `WAIT` with A = `$0C` between toggles. `WAIT` takes (26 + 27A + 5A²) / 2
//! cycles including its JSR, so the toggles are 546 cycles apart: a square wave of about
//! 935 Hz for 0.1 s. The sound is rebuilt here from those cycle counts; no ROM is used.

use crate::engine::cost::CPU_HZ;

/// `WAIT` (`$FCA8`) with this A, including the JSR and RTS.
const fn wait(a: u32) -> u32 {
    (26 + 27 * a + 5 * a * a) / 2
}

/// Cycles from `JSR $6618` to the first speaker toggle: JSR, LDA $C082, JSR $FBDD,
/// LDA #$40, WAIT, LDY #$C0, then LDA #$0C, WAIT and the LDA $C030 that toggles.
const FIRST_TOGGLE: u32 = 6 + 4 + 6 + 2 + wait(0x40) + 2 + 2 + wait(0x0C) + 4;
/// One BELL2 loop: LDA #$0C, WAIT, LDA $C030, DEY, BNE.
const TOGGLE_PERIOD: u32 = 2 + wait(0x0C) + 4 + 2 + 3;
const TOGGLES: u32 = 192;
/// One beep, from `JSR $6618` to its return (after the last toggle: DEY, BNE, RTS, two
/// LDA $C08B, RTS).
pub const BELL_CYCLES: u32 = FIRST_TOGGLE + (TOGGLES - 1) * TOGGLE_PERIOD + 2 + 2 + 6 + 4 + 4 + 6;
/// `$6625`'s pause between its two beeps: LDY #$20, then 32 rounds of a 256-step DEX loop.
const DOUBLE_GAP: u32 = 2 + 32 * (256 * 5 - 1 + 5) - 1;
/// Two beeps, from `JSR $6625` to its return.
pub const DOUBLE_BELL_CYCLES: u32 = 6 + BELL_CYCLES + DOUBLE_GAP + BELL_CYCLES + 6;

/// The cycles (from the start of the call) at which the speaker toggles.
pub fn toggles(double: bool) -> Vec<u32> {
    let one = |start: u32| (0..TOGGLES).map(move |i| start + FIRST_TOGGLE + i * TOGGLE_PERIOD);
    if double {
        one(6).chain(one(6 + BELL_CYCLES + DOUBLE_GAP)).collect()
    } else {
        one(0).collect()
    }
}

/// The duration of the call, in seconds of the original's time.
pub fn seconds(double: bool) -> f64 {
    (if double { DOUBLE_BELL_CYCLES } else { BELL_CYCLES }) as f64 / CPU_HZ
}

/// The speaker as 16-bit samples at `rate` Hz: silent until the first toggle and after
/// the last, a square wave between them (each sample averages its interval, so the
/// edges are not aliased).
pub fn samples(double: bool, rate: u32) -> Vec<i16> {
    let t = toggles(double);
    let total = if double { DOUBLE_BELL_CYCLES } else { BELL_CYCLES } as f64;
    let n = (total / CPU_HZ * rate as f64).ceil() as usize;
    let per = CPU_HZ / rate as f64;
    const AMP: f64 = 9000.0;
    // Level over [a, b) cycles: 0 outside each beep's toggles, else +-1 by parity.
    let level = |c: f64| -> f64 {
        let k = t.partition_point(|&x| (x as f64) <= c);
        // Inside a beep if an odd number of toggles lie before it within that beep, or
        // between toggles of the same beep.
        if k == 0 || k == t.len() {
            return 0.0;
        }
        let beep = |i: usize| i / TOGGLES as usize;
        if beep(k - 1) != beep(k) {
            return 0.0;
        }
        if k % 2 == 1 {
            1.0
        } else {
            -1.0
        }
    };
    (0..n)
        .map(|i| {
            const SUB: usize = 8;
            let s: f64 = (0..SUB).map(|j| level((i as f64 + (j as f64 + 0.5) / SUB as f64) * per)).sum();
            (s / SUB as f64 * AMP) as i16
        })
        .collect()
}

/// A mono 16-bit WAV file of the bell.
pub fn wav(double: bool, rate: u32) -> Vec<u8> {
    let s = samples(double, rate);
    let data = (s.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for x in s {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_timing() {
        // refemu charges the bell at about 116,000 cycles.
        assert_eq!(wait(0x0C), 535);
        assert_eq!(TOGGLE_PERIOD, 546);
        assert_eq!(BELL_CYCLES, 115_988);
        let hz = CPU_HZ / (2.0 * TOGGLE_PERIOD as f64);
        assert!((hz - 934.5).abs() < 0.5, "{hz}");
        let s = samples(false, 44_100);
        assert_eq!(s.len(), 5013);
        // Silent for the first 0.0114 s, then the square wave.
        assert!(s[..500].iter().all(|&x| x == 0));
        assert!(s[600..4900].iter().any(|&x| x > 8000) && s[600..4900].iter().any(|&x| x < -8000));
        assert_eq!(toggles(true).len(), 384);
    }
}
