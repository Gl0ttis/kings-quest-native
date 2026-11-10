//! refemu-pc DISK [--seconds S] [--passes N] [--plan PLAN] [--trace FILE] [--shot FILE.png]
//!           [--signal] [--profile] [--fetch-cycles F] [--cga-wait N]
//!
//! A reference IBM PC with the CGA, built only to measure the original King's Quest: it
//! starts the game's engine from the user's disk image (an HxC `.mfm` dump of the
//! original, or the crack's `.img`) and runs it on an 8086 core with 8088 timing. No IBM
//! ROM: the BIOS services the game uses are answered natively (machine.rs). See
//! README.md.
//!
//! - `--seconds S`, `--passes N`: stop after S seconds of the PC's time (default 600)
//!   or N passes of the game cycle (default no limit).
//! - `--plan PLAN` (or `@FILE`, items separated by commas or lines, `#` comments), as
//!   refemu-pcjr's: items run at the start of each pass until a `wN` (wait N passes);
//!   a `p` item (`p>text`, `pU`, ...) is typed instead when the game is held at a
//!   prompt. The keys: `>text` a line and Enter, `U` `D` `L` `R` the arrows, `kHH` a
//!   character, `sHHHH` a scan code and character. A prompt with no `p` item due gets
//!   Enter.
//! - `--trace FILE`: a row per pass, the columns of refemu-pcjr's: the pass, its start
//!   cycle and length, the room, the ego's x, y and direction, the score, and a hash of
//!   the variables.
//! - `--shot FILE.png`: the screen at the end, in composite colour (`--signal`: the
//!   signal decoded, with its fringes).
//! - `--profile`: where the time went: cycles by 16-byte block of the engine's code.
//! - `--fetch-cycles F`, `--cga-wait N`: the timing's two figures (README.md, "Timing";
//!   default 1.5 and 4).
//!
//! Debug switches: `REFEMU_TIMELINE` (every 0.1 s: the pass, the room, how many of the
//! CGA's bytes changed, the disk reads), `REFEMU_WORK` (each pass's wait for the timer),
//! `REFEMU_RAM=FILE` (the whole memory at the end).

use kq1_pc::composite::{self, Mode};
use kq1_pc_refemu::{cpu, machine};

use cpu::{Cpu, CS};
use machine::{Disk, Machine, CPU_HZ};
use std::collections::{HashMap, VecDeque};

/// One pass of the game cycle: `$293A` in the engine's code segment 0060 (the PCjr's
/// `$14F2`: the actors move and animate, then the cycle rules run).
const MAIN_LOOP: (u16, u16) = (0x0060, 0x293A);
/// The engine's data segment: CS + `$511`. Its variables are at DS:`$1609` and the
/// actor table at DS:`$17E7` (the PCjr's `$1003` and `$11E1`, `$606` on; 30 bytes an
/// actor: x and y at `$18`/`$19`, the direction at `$0F`). The room is v1, the score
/// v19.
const DATA_SEG: u16 = 0x0060 + 0x0511;
const VARS: u16 = 0x1609;
const ACTORS: u16 = 0x17E7;

/// A key as INT 16h returns it: scan code << 8 | character.
fn key_for(item: &str) -> Option<Vec<u16>> {
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

/// The screen as an RGB PNG: the CGA's 640 × 200 dots through the composite model, each
/// line doubled.
fn shot(m: &Machine, mode: Mode) -> Vec<u8> {
    let dots = m.screen_dots();
    let rgb: Vec<u8> = dots
        .chunks(640)
        .flat_map(|line| {
            let row: Vec<u8> = composite::line(line, mode).into_iter().flatten().collect();
            [row.clone(), row]
        })
        .flatten()
        .collect();
    kq1_formats::png::encode_rgb(640, 400, &rgb)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let seconds: f64 = opt("--seconds").map_or(600.0, |v| v.parse().expect("--seconds"));
    let max_passes: u64 = opt("--passes").map_or(u64::MAX, |v| v.parse().expect("--passes"));
    let plan = opt("--plan").unwrap_or_default();
    let plan = match plan.strip_prefix('@') {
        Some(f) => std::fs::read_to_string(f)
            .unwrap_or_else(|e| panic!("{f}: {e}"))
            .lines()
            .map(|l| l.split('#').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(","),
        None => plan,
    };
    let trace_path = opt("--trace");
    let fetch_cycles: Option<f64> = opt("--fetch-cycles").map(|v| v.parse().expect("--fetch-cycles"));
    let cga_wait: Option<u64> = opt("--cga-wait").map(|v| v.parse().expect("--cga-wait"));
    let shot_path = opt("--shot");
    let mut flag = |name: &str| {
        let on = args.iter().any(|a| a == name);
        args.retain(|a| a != name);
        on
    };
    let profile = flag("--profile");
    let signal = flag("--signal");
    let Some(path) = args.first() else {
        eprintln!("usage: refemu-pc DISK [--seconds S] [--plan PLAN] [--shot FILE.png] [--signal] [--profile]");
        std::process::exit(2);
    };
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let game = kq1_pc::disk::Disk::from_file(raw.clone()).unwrap_or_else(|e| panic!("{path}: {e}"));
    let exe = game.read_file(&game.file("MAIN.EXE").expect("MAIN.EXE")).expect("MAIN.EXE");
    let mut disk = if raw.starts_with(b"HXCMFM\0") {
        Disk::from_tracks(&kq1_formats::mfm::decode_with_errors(&raw).expect("the image").0)
    } else {
        Disk::from_image(&raw)
    };
    disk.write_protected = true;
    let mut m = Machine::new(disk);
    if let Some(b) = kq1_pc_refemu::bios_next_to(path) {
        m.set_font(&b).unwrap_or_else(|e| panic!("bios.rom: {e}"));
    }
    if let Some(f) = fetch_cycles {
        m.fetch_quarters = (f * 4.0).round() as u64;
    }
    if let Some(w) = cga_wait {
        m.cga_wait = w;
    }
    let mut cpu = Cpu::new();
    m.load_engine(&mut cpu, &exe).unwrap_or_else(|e| panic!("{e}"));

    let limit = (seconds * CPU_HZ) as u64;
    let mut items: VecDeque<String> = plan.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut hits: HashMap<u16, u64> = HashMap::new();
    let mut last: VecDeque<(u16, u16)> = VecDeque::new();
    let mut entered = false;
    // --profile: cycles by 16-byte block of the engine's code.
    let mut prof_at: Option<(u16, u64)> = None;
    let mut wait_since: Option<u64> = None;
    let (mut passes, mut wait_until, mut last_pass) = (0u64, 0u64, None::<u64>);
    // REFEMU_WORK: each pass's cycles before it waits for the timer ($3FE7).
    let work_log = std::env::var_os("REFEMU_WORK").is_some();
    let mut waited_this_pass = false;
    let timeline = std::env::var_os("REFEMU_TIMELINE").is_some();
    let (mut next_sample, mut sample) = (0u64, vec![0u8; 0x4000]);
    let mut trace = String::from("pass,cycles,delta,room,ego_x,ego_y,ego_dir,score,vars_hash\n");
    let rd = |m: &Machine, off: u16| m.ram[cpu::lin(DATA_SEG, off) as usize];
    let push = |m: &mut Machine, item: &str| {
        for k in key_for(item).unwrap_or_else(|| panic!("plan: unknown item {item}")) {
            m.push_key(k);
        }
    };
    while cpu.cycles < limit && cpu.fault.is_none() && passes < max_passes {
        last.push_back((cpu.s[CS], cpu.ip));
        if last.len() > 30 {
            last.pop_front();
        }
        if cpu.s[CS] == 0x60 {
            if !entered {
                entered = true;
                eprintln!("engine running at 0060 after {} cycles", cpu.cycles);
            }
            if profile {
                prof_at = Some((cpu.ip & 0xFFF0, cpu.cycles));
            }
        }
        if (cpu.s[CS], cpu.ip) == MAIN_LOOP {
            passes += 1;
            if passes == 1 {
                eprintln!("main loop reached after {:.1} s", cpu.cycles as f64 / CPU_HZ);
            }
            wait_since = None;
            m.idle_polls = 0;
            let delta = last_pass.map_or(0, |l| cpu.cycles - l);
            last_pass = Some(cpu.cycles);
            let vars: u64 = (0..0xDFu16)
                .map(|v| rd(&m, VARS + v) as u64)
                .fold(1469598103934665603u64, |h, b| (h ^ b).wrapping_mul(1099511628211));
            trace += &format!(
                "{passes},{},{delta},{},{},{},{},{},{vars:016x}\n",
                cpu.cycles,
                rd(&m, VARS + 1),
                rd(&m, ACTORS + 0x18),
                rd(&m, ACTORS + 0x19),
                rd(&m, ACTORS + 0x0F),
                rd(&m, VARS + 19)
            );
            if passes >= wait_until {
                while items.front().is_some_and(|i| !i.starts_with('p')) {
                    let item = items.pop_front().unwrap();
                    if let Some(n) = item.strip_prefix('w') {
                        wait_until = passes + n.parse::<u64>().unwrap_or(1);
                        break;
                    }
                    push(&mut m, &item);
                }
            }
        }
        // A game polling the keyboard with no pass for a while is at a prompt (a pass is
        // a fraction of a second): a `p` item is typed after 1.5M cycles; with none due,
        // Enter after 3M cycles. The title screens only poll, before the first pass.
        if m.idle_polls > 0 && m.keys.is_empty() && (m.blocked_reads > 0 || passes == 0) {
            let since = *wait_since.get_or_insert(cpu.cycles);
            let waited = cpu.cycles - since;
            let typed = items.front().is_some_and(|i| i.starts_with('p')) && waited > 1_500_000;
            if typed {
                let item = items.pop_front().unwrap();
                push(&mut m, &item[1..]);
            } else if waited > 3_000_000 {
                eprintln!("Enter at a prompt, {:.1} s", cpu.cycles as f64 / CPU_HZ);
                m.push_key(0x1C0D);
            }
            if typed || waited > 3_000_000 {
                wait_since = None;
                m.idle_polls = 0;
                m.blocked_reads = 0;
            }
        }
        if work_log && cpu.s[CS] == 0x60 {
            // Entering the wait (from $3FE5), and leaving it ($3FEF).
            match (cpu.ip, waited_this_pass) {
                (0x3FE7, false) => {
                    waited_this_pass = true;
                    eprintln!("pass {passes}: wait from {}", cpu.cycles - last_pass.unwrap_or(0));
                }
                (0x3FEF, true) => {
                    waited_this_pass = false;
                    eprintln!("pass {passes}:   to {}", cpu.cycles - last_pass.unwrap_or(0));
                }
                _ => {}
            }
        }
        if timeline && cpu.cycles >= next_sample {
            // REFEMU_TIMELINE: every 0.1 s, how many of the CGA's bytes changed.
            next_sample += (CPU_HZ / 10.0) as u64;
            let now = m.ram[0xB8000..0xBC000].to_vec();
            let changed = now.iter().zip(&sample).filter(|(a, b)| a != b).count();
            let reads = m.calls.get(&(0x13, 2)).copied().unwrap_or(0);
            eprintln!(
                "t {:.2} pass {passes} room {} changed {changed} reads {reads} sectors {}",
                cpu.cycles as f64 / CPU_HZ,
                rd(&m, VARS + 1),
                m.sectors_read
            );
            sample = now;
        }
        m.step(&mut cpu);
        if let Some((block, c)) = prof_at.take() {
            *hits.entry(block).or_default() += cpu.cycles - c;
        }
    }
    eprintln!("stopped after {:.1} s ({} cycles), {passes} passes", cpu.cycles as f64 / CPU_HZ, cpu.cycles);
    // REFEMU_RAM=FILE: the whole memory at the end, for inspection.
    if let Some(p) = std::env::var_os("REFEMU_RAM") {
        std::fs::write(p, &m.ram).expect("REFEMU_RAM");
    }
    if let Some(p) = &trace_path {
        std::fs::write(p, &trace).expect("write the trace");
    }
    if let Some(f) = &cpu.fault {
        eprintln!("FAULT: {f}");
        for (s, o) in &last {
            eprintln!("  {s:04X}:{o:04X}");
        }
    }
    let mut calls: Vec<_> = m.calls.iter().collect();
    calls.sort();
    eprintln!(
        "BIOS calls: {}",
        calls.iter().map(|((n, ah), c)| format!("{n:02X}/{ah:02X}:{c}")).collect::<Vec<_>>().join(" ")
    );
    let mut ports: Vec<_> = m.unknown_ports.iter().collect();
    ports.sort();
    eprintln!(
        "ports not modelled: {}",
        ports.iter().map(|(p, c)| format!("{p:04X}:{c}")).collect::<Vec<_>>().join(" ")
    );
    eprintln!(
        "CGA: mode {:02X} colour {:02X}, CRTC R2 {:02X}; video mode {}",
        m.cga.mode, m.cga.colour, m.cga.crtc[2], m.video_mode
    );
    eprintln!("speaker events: {}; sectors read: {}; keys waiting {:04X?}", m.sound.len(), m.sectors_read, m.keys);
    eprintln!("vectors: 08 {:04X?} 09 {:04X?} 1C {:04X?}", m.vector(8), m.vector(9), m.vector(0x1C));
    let text: Vec<String> =
        m.text.iter().map(|r| String::from_utf8_lossy(r).trim_end().to_string()).filter(|r| !r.is_empty()).collect();
    if !text.is_empty() {
        eprintln!("text on screen:\n  {}", text.join("\n  "));
    }
    for n in &m.notes {
        eprintln!("note: {n}");
    }
    if profile {
        let mut h: Vec<_> = hits.into_iter().collect();
        h.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (a, n) in h.iter().take(30) {
            eprintln!("  {a:04X}: {n}");
        }
    }
    if let Some(p) = shot_path {
        std::fs::write(&p, shot(&m, if signal { Mode::Signal } else { Mode::Groups })).expect("write the shot");
    }
}
