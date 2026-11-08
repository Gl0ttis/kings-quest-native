//! refemu-pcjr DISK [--plan PLAN] [--keys FILE] [--passes N] [--ticks N] [--trace FILE]
//!              [--frames FILE] [--shot FILE.png] [--profile]
//!
//! A reference IBM PCjr, built only to measure the original King's Quest: it boots the
//! game from the user's disk image (an HxC `.mfm` dump of the original, or the crack's
//! `.img`) and runs it on an 8086 core with 8088 timing. No IBM ROM: the BIOS services
//! the game uses are answered natively (machine.rs). See README.md.

mod cpu;
mod machine;

use cpu::{Cpu, CS};
use machine::{Disk, Machine, CPU_HZ, CYCLES_PER_TICK};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// One pass of the game cycle (`$14F2`, in the engine's code segment 0060): the actors
/// move and animate, then the cycle rules run, the global script's repeating while it
/// asks to, then the room's (`$163B`, `$1658`). The input wait (`$13CE`) runs it over
/// and over while the player types; the main loop (`$D1C`) comes round only after a
/// typed line, to run the parser rules (`$47E1`).
const MAIN_LOOP: (u16, u16) = (0x0060, 0x14F2);
/// The engine's data segment, and its variables (`DS:$1003`) and actor table
/// (`DS:$11E1`, 30 bytes an actor: room at 0, loop 3, cel 8, direction `$0F`, x and y
/// `$18`/`$19`). The score is v19 (`add_score`, `$1D4C`, adds to `$1016`).
const DATA_SEG: u16 = 0x0543;
/// Routines whose entries --profile counts.
const WATCH: [u16; 8] = [0x0D1C, 0x14F2, 0x13CE, 0x1069, 0x0DC1, 0x47E1, 0x47EE, 0x0D37];
const VARS: u16 = 0x1003;
const ACTORS: u16 = 0x11E1;

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
        // The arrow keys: scan codes, no character.
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
    let ticks: u64 = opt("--ticks").map_or(u64::MAX / CYCLES_PER_TICK, |v| v.parse().expect("--ticks"));
    let max_passes: u64 = opt("--passes").map_or(400, |v| v.parse().expect("--passes"));
    let plan = opt("--plan").unwrap_or_default();
    // Key log: "PASS KEY-HEX" per line (the key as INT 16h returns it, scan << 8 | char,
    // or just a character); pressed at the start of that pass.
    let key_log: BTreeMap<u64, u16> = opt("--keys")
        .map(|p| {
            std::fs::read_to_string(&p)
                .expect("key log")
                .lines()
                .filter_map(|l| {
                    let (n, k) = l.split_once(' ')?;
                    let k = u16::from_str_radix(k.trim(), 16).ok()?;
                    let k = if k < 0x100 { key_for(&format!("k{k:02X}"))?[0] } else { k };
                    Some((n.trim().parse().ok()?, k))
                })
                .collect()
        })
        .unwrap_or_default();
    let trace_path = opt("--trace");
    // REFEMU_DUMP=p,...: print actor 0 and v0-v47 at the start of those passes.
    let dump: Vec<u64> = std::env::var("REFEMU_DUMP")
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    let frames_path = opt("--frames");
    let shot = opt("--shot");
    let profile = args.iter().any(|a| a == "--profile");
    args.retain(|a| a != "--profile");
    let Some(path) = args.first() else {
        eprintln!("usage: refemu-pcjr DISK [--plan PLAN] [--keys FILE] [--passes N] [--ticks N] [--trace FILE] [--frames FILE] [--shot FILE.png] [--profile]");
        std::process::exit(2);
    };
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let disk = if raw.starts_with(b"HXCMFM\0") {
        Disk::from_tracks(&kq1_pcjr::mfm::decode(&raw).unwrap_or_else(|e| panic!("{path}: {e}")))
    } else {
        // A 360K sector image: 9 sectors of 512 bytes a track.
        let mut d = Disk::default();
        for (i, s) in raw.chunks(512).enumerate() {
            let (c, h, r) = ((i / 18) as u8, (i / 9 % 2) as u8, (i % 9 + 1) as u8);
            d.sectors.insert((c, h, r, 2), s.to_vec());
        }
        d
    };

    let mut m = Machine::new(disk);
    let mut cpu = Cpu::new();
    m.boot(&mut cpu).unwrap_or_else(|e| panic!("{e}"));
    let limit = ticks.saturating_mul(CYCLES_PER_TICK);
    let mut items: VecDeque<String> = plan.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut wait_until = 0u64;
    let mut passes = 0u64;
    let mut last_pass_cycle: Option<u64> = None;
    let mut first_pass_cycle = 0;
    let mut trace = String::from("pass,cycles,delta,room,ego_x,ego_y,ego_dir,score,vars_hash\n");
    let mut frames: Vec<u8> = Vec::new();
    let mut last_frame: Vec<u8> = Vec::new();
    let mut deltas: Vec<u64> = Vec::new();
    let mut hits: HashMap<(u16, u16), u64> = HashMap::new();
    let mut entries: BTreeMap<u16, u64> = BTreeMap::new();
    let mut last: VecDeque<(u16, u16, [u16; 8])> = VecDeque::new();
    let mut wait_since: Option<u64> = None;
    let rd = |m: &Machine, off: u16| m.ram[cpu::lin(DATA_SEG, off) as usize];

    while cpu.cycles < limit && cpu.fault.is_none() && passes < max_passes {
        last.push_back((cpu.s[CS], cpu.ip, cpu.r));
        if last.len() > 40 {
            last.pop_front();
        }
        if profile {
            *hits.entry((cpu.s[CS], cpu.ip & 0xFFF0)).or_default() += 1;
            if cpu.s[CS] == 0x60 && WATCH.contains(&cpu.ip) {
                *entries.entry(cpu.ip).or_default() += 1;
            }
        }
        if (cpu.s[CS], cpu.ip) == MAIN_LOOP {
            passes += 1;
            wait_since = None;
            m.idle_polls = 0;
            if passes == 1 {
                first_pass_cycle = cpu.cycles;
                eprintln!("main loop reached after {:.1} s", cpu.cycles as f64 / CPU_HZ);
            }
            if frames_path.is_some() {
                let mut f = m.screen_mode9();
                f.extend(m.text.iter().flatten());
                if f != last_frame {
                    frames.extend_from_slice(&(passes as u32).to_le_bytes());
                    frames.extend_from_slice(&f);
                    last_frame = f;
                }
            }
            if dump.contains(&passes) {
                let act: Vec<String> = (0..30u16).map(|i| format!("{:02x}", rd(&m, ACTORS + i))).collect();
                let vars: Vec<String> = (0..48u16).map(|i| format!("{}", rd(&m, VARS + i))).collect();
                eprintln!("pass {passes}: actor0 {} | v0-47 {}", act.join(" "), vars.join(","));
            }
            if let Some(&k) = key_log.get(&passes) {
                m.keys.push_back(k);
            }
            let delta = last_pass_cycle.map_or(0, |l| cpu.cycles - l);
            if last_pass_cycle.is_some() {
                deltas.push(delta);
            }
            last_pass_cycle = Some(cpu.cycles);
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
            // The plan.
            if passes >= wait_until {
                while let Some(item) = items.pop_front() {
                    if let Some(n) = item.strip_prefix('w') {
                        wait_until = passes + n.parse::<u64>().unwrap_or(1);
                        break;
                    }
                    match key_for(&item) {
                        Some(ks) => m.keys.extend(ks),
                        None => eprintln!("plan: unknown item {item}"),
                    }
                }
            }
        }
        // A game polling the keyboard with no pass and no key for 3M cycles is waiting
        // for one ("press a key"): it gets RETURN, as refemu does for the Apple.
        if m.idle_polls > 0 && m.keys.is_empty() {
            let since = *wait_since.get_or_insert(cpu.cycles);
            if cpu.cycles - since > 3_000_000 {
                m.keys.push_back(0x1C0D);
                wait_since = None;
                m.idle_polls = 0;
            }
        }
        m.step(&mut cpu);
    }

    println!(
        "ran {:.2} s emulated ({} cycles), {passes} passes; stopped at {:04X}:{:04X}",
        cpu.cycles as f64 / CPU_HZ,
        cpu.cycles,
        cpu.s[CS],
        cpu.ip
    );
    if let Some(f) = &cpu.fault {
        println!("STOPPED: {f}");
        for (cs, ip, r) in &last {
            println!(
                "  {cs:04X}:{ip:04X}  ax {:04X} bx {:04X} cx {:04X} dx {:04X} sp {:04X}",
                r[0], r[3], r[1], r[2], r[4]
            );
        }
    }
    if !deltas.is_empty() {
        let mut d = deltas.clone();
        d.sort();
        let mean = d.iter().sum::<u64>() as f64 / d.len() as f64;
        println!(
            "cycles per pass: min {} median {} max {} mean {:.0}; {:.1} passes/s; {:.1} s since the first pass",
            d[0],
            d[d.len() / 2],
            d[d.len() - 1],
            mean,
            CPU_HZ / mean,
            (cpu.cycles - first_pass_cycle) as f64 / CPU_HZ
        );
    }
    let mut calls: Vec<_> = m.calls.iter().collect();
    calls.sort();
    println!(
        "BIOS calls: {}",
        calls
            .iter()
            .filter(|((n, _), _)| *n != 8)
            .map(|((n, ah), c)| format!("{n:02X}h/{ah:02X}h x{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !m.unknown_ports.is_empty() {
        println!("unmodelled ports (bit 15 = write): {:?}", m.unknown_ports);
    }
    println!("video mode {} CRT page {}; {} sound chip writes", m.video_mode, m.crt_page, m.sound.len());
    let text: Vec<String> =
        m.text.iter().map(|r| String::from_utf8_lossy(r).trim_end().to_string()).filter(|r| !r.is_empty()).collect();
    if !text.is_empty() {
        println!("text on screen:\n  {}", text.join("\n  "));
    }
    if profile {
        println!("entries: {}", entries.iter().map(|(a, n)| format!("${a:04X} x{n}")).collect::<Vec<_>>().join(", "));
        let mut v: Vec<_> = hits.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        for ((seg, ip), n) in v.into_iter().take(25) {
            println!("  {seg:04X}:{ip:04X}  {n}");
        }
    }
    if let Some(p) = trace_path {
        std::fs::write(p, trace).expect("write trace");
    }
    if let Some(p) = frames_path {
        std::fs::write(p, frames).expect("write frames");
    }
    if let Some(f) = shot {
        let px = m.screen_mode9();
        let rgb: Vec<u8> = px.iter().flat_map(|&c| kq1_pcjr::color::rgb(c)).collect();
        std::fs::write(&f, kq1_apple2::png::encode_rgb(320, 200, &rgb)).expect("write shot");
    }
}
