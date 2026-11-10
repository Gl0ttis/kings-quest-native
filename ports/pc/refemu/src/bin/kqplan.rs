//! kqplan-pc DISK PLAN [--events OUT] [--trace OUT] [--frames OUT] [--speaker OUT] [--play IMG] [--play-out IMG]
//!
//! Plays a plan through the original in refemu-pc (the runner) and writes the reference
//! the port is replayed against: the event log, the trace and the screens. A plan is
//! refemu-pcjr's (routes/*.plan, items separated by commas or lines, `#` comments):
//!
//! - at the start of each pass, items run until a `wN` (wait N passes): keys (`>text` a
//!   line and Enter, `U` `D` `L` `R` the arrows, `S` the last arrow again, which stops
//!   Graham, `kHH` a character, `sHHHH` a scan code and character), `@play` and `@master`
//!   (a disk into the drive);
//! - a `p` item (`p>a`, `pk79`, ...) is typed when the game is held at a prompt; a prompt
//!   with none due gets Enter. "Please insert your ... diskette" is answered as a player
//!   would (the runner).
//!
//! `--play IMG` starts with that Play diskette (a 360K image; blank otherwise), and
//! `--play-out IMG` writes the Play diskette as it is at the end. `--frames` writes a
//! hash of the screen (runner `frame`, FNV-1a) at the start of every pass, one a line
//! in hex, as the PCjr port's reference/*.frames (numbers, not the game's screens).

use kq1_pc_refemu::machine::Disk;
use kq1_pc_refemu::runner::{Runner, Stopped};
use std::collections::VecDeque;

/// A key as INT 16h returns it: scan code << 8 | character.
fn keys_for(item: &str) -> Option<Vec<u16>> {
    let ascii = |c: u8| -> u16 {
        let scan = match c {
            b'\r' => 0x1C,
            b' ' => 0x39,
            0x1B => 0x01,
            0x08 => 0x0E,
            0x09 => 0x0F,
            _ => 0,
        };
        scan << 8 | c as u16
    };
    Some(match item {
        "U" => vec![0x4800],
        "D" => vec![0x5000],
        "L" => vec![0x4B00],
        "R" => vec![0x4D00],
        _ => {
            if let Some(t) = item.strip_prefix('>') {
                t.bytes().map(ascii).chain([ascii(b'\r')]).collect()
            } else if let Some(h) = item.strip_prefix('k') {
                vec![ascii(u8::from_str_radix(h, 16).ok()?)]
            } else {
                vec![u16::from_str_radix(item.strip_prefix('s')?, 16).ok()?]
            }
        }
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let (events, trace, frames) = (opt("--events"), opt("--trace"), opt("--frames"));
    let (play, play_out) = (opt("--play"), opt("--play-out"));
    let speaker = opt("--speaker");
    let [disk, plan] = args.as_slice() else {
        eprintln!(
            "usage: kqplan-pc DISK PLAN [--events OUT] [--trace OUT] [--frames OUT] [--play IMG] [--play-out IMG]"
        );
        std::process::exit(2);
    };
    let raw = std::fs::read(disk).unwrap_or_else(|e| panic!("{disk}: {e}"));
    let text = std::fs::read_to_string(plan).unwrap_or_else(|e| panic!("{plan}: {e}"));
    let mut items: VecDeque<String> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(|l| l.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut r = Runner::new(&raw).unwrap_or_else(|e| panic!("{e}"));
    // The BIOS's font, from bios.rom next to the disk, if it is there.
    if let Some(b) = kq1_pc_refemu::bios_next_to(disk) {
        r.m.set_font(&b).unwrap_or_else(|e| panic!("bios.rom: {e}"));
    }
    if let Some(p) = &play {
        r.m.disks[1] = Disk::from_image(&std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}")));
    }
    let (mut wait_until, mut last_arrow) = (0u64, 0x4D00u16);
    let mut out_frames = String::new();
    let press = |r: &mut Runner, item: &str, last_arrow: &mut u16| match item {
        "@play" => r.m.insert(1),
        "@master" => r.m.insert(0),
        "S" => r.press(*last_arrow),
        _ => {
            let ks = keys_for(item).unwrap_or_else(|| panic!("plan: unknown item {item}"));
            if let Some(&a) = ks.iter().find(|&&k| matches!(k, 0x4800 | 0x5000 | 0x4B00 | 0x4D00)) {
                *last_arrow = a;
            }
            ks.into_iter().for_each(|k| r.press(k));
        }
    };
    loop {
        // Making a Play diskette copies the disk with no pass between, swap after swap.
        let stop = r.run(1_500_000, 4_000_000_000).unwrap_or_else(|e| {
            let screen: Vec<String> =
                r.m.text
                    .iter()
                    .map(|l| String::from_utf8_lossy(l).trim_end().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
            panic!(
                "pass {} at cycle {}: {e}
screen:
  {}",
                r.passes,
                r.cpu.cycles,
                screen.join(
                    "
  "
                )
            )
        });
        match stop {
            Stopped::Pass => {
                if frames.is_some() {
                    let h = r
                        .frame()
                        .iter()
                        .fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
                    out_frames += &format!(
                        "{h:016x}
"
                    );
                }
                if r.passes >= wait_until {
                    while items.front().is_some_and(|i| !i.starts_with('p')) {
                        let item = items.pop_front().unwrap();
                        if let Some(n) = item.strip_prefix('w') {
                            wait_until = r.passes + n.parse::<u64>().expect("wN");
                            break;
                        }
                        press(&mut r, &item, &mut last_arrow);
                    }
                }
                if items.is_empty() && r.passes >= wait_until {
                    break;
                }
            }
            Stopped::Prompt => match items.front().filter(|i| i.starts_with('p')).cloned() {
                Some(item) => {
                    items.pop_front();
                    press(&mut r, &item[1..], &mut last_arrow);
                }
                None => r.press(0x1C0D),
            },
        }
    }
    eprintln!("{} passes, room {}, score {}", r.passes, r.v(1), r.v(19));
    if let Some(p) = events {
        std::fs::write(p, r.events()).expect("write the events");
    }
    if let Some(p) = trace {
        std::fs::write(p, &r.trace).expect("write the trace");
    }
    if let Some(p) = speaker {
        std::fs::write(p, kq1_pc_refemu::speaker_log(&r.m.sound)).expect("write the speaker's events");
    }
    if let Some(p) = frames {
        std::fs::write(p, out_frames).expect("write the frames");
    }
    if let Some(p) = play_out {
        let d = &r.m.disks[1];
        let mut img = vec![0u8; 368_640];
        for (&(c, h, rec, size), data) in &d.sectors {
            let lin = (c as usize * 2 + h as usize) * 9 + rec as usize - 1;
            if size == 2 && (1..=9).contains(&rec) && lin < 720 {
                img[lin * 512..lin * 512 + 512].copy_from_slice(data);
            }
        }
        std::fs::write(p, img).expect("write the Play diskette");
    }
}
