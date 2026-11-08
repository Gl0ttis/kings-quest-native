//! refemu disk1.nib disk2.nib disk3.nib [--plan PLAN] [--passes N] [--trace FILE] [--shot FILE]
//!
//! Reference Apple IIe for measuring the original King's Quest. It is a measuring tool
//! only; the port in the parent crate does not use it.
//!
//! PLAN is a comma-separated list executed in main-loop passes: wN (wait N passes),
//! U/UR/R/DR/D/DL/L/UL (direction key), S (repeat the last direction key, which stops),
//! >text (type a command and RETURN), kHH (raw key code).
mod cpu;
mod machine;

use cpu::Cpu;
use machine::{Disk, Machine, NIB_TRACK};
use std::{collections::BTreeMap, env, fs};

/// Physical sector of track 0, decoded with the port's GCR decoder.
pub fn boot_sector(d: &Disk, phys: usize) -> Option<[u8; 256]> {
    let t = kq1_apple2::nib::decode_track(0, d.tracks.first()?);
    t.sectors.iter().find(|s| s.sector as usize == phys && s.data_checksum_ok).map(|s| s.data)
}

const MAIN_LOOP: u16 = 0x4026;

/// Inner-loop entry points counted per pass for the cost model: (name, address, aux?).
const EVENTS: [(&str, u16, bool); 11] = [
    ("rule_bytes", 0x6358, false),
    ("operands", 0x58F2, false),
    ("cel_pixels", 0x1149, true),
    ("cel_nibbles", 0x1194, true),
    ("screen_pixels", 0x145A, true),
    ("screen_rows", 0x144F, true),
    ("bg_save_pixels", 0x15DC, true),
    ("bg_restore_pixels", 0x1612, true),
    ("chars", 0x14A4, true),
    ("plots", 0x102F, true),
    ("sector_reads", 0xBD00, false),
];
const CPU_HZ: f64 = 1_020_484.0;

fn key_for(item: &str) -> Option<u8> {
    Some(match item {
        "U" => 0x0B,
        "D" => 0x0A,
        "L" => 0x08,
        "R" => 0x15,
        "UR" => b'\'',
        "DR" => b'/',
        "DL" => b'.',
        "UL" => b';',
        _ => return None,
    })
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args[p + 1].clone();
        args.drain(p..p + 2);
        Some(v)
    };
    let plan = opt("--plan").unwrap_or_default();
    let max_passes: u64 = opt("--passes").map_or(400, |v| v.parse().unwrap());
    let trace_path = opt("--trace");
    let shot_path = opt("--shot");
    let buf_path = opt("--picbuf");
    // Key log from kqroute: "PASS HEXKEY" per line; each key is pressed at the start of
    // that pass, so the engine reads it during the pass, as in the port.
    let key_log: BTreeMap<u64, u8> = opt("--keys")
        .map(|p| {
            fs::read_to_string(p)
                .expect("read keys")
                .lines()
                .filter_map(|l| {
                    let (a, b) = l.split_once(' ')?;
                    Some((a.parse().ok()?, u8::from_str_radix(b.trim(), 16).ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    // --save-out FILE: with a fourth (save) disk, write it here afterwards if the game
    // wrote to it.
    let save_out = opt("--save-out");
    let max_cycles: u64 = opt("--max-cycles").map_or(u64::MAX, |v| v.parse().unwrap());
    let mut pc_hist: BTreeMap<u16, u64> = BTreeMap::new();
    let profile = env::var_os("REFEMU_PROFILE").is_some();
    let rng_log = env::var_os("REFEMU_RNG").is_some();
    let f2_watch: Option<(u64, u64)> = env::var("REFEMU_F2").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });
    let mut f2_last: Option<u8> = None;
    let mut f2_pc: u16 = 0;
    // REFEMU_PEEK=hexaddr,...: log every change of these main-memory bytes, with the pass.
    let peek: Vec<u16> = env::var("REFEMU_PEEK")
        .map(|v| v.split(',').filter_map(|x| u16::from_str_radix(x.trim().trim_start_matches('$'), 16).ok()).collect())
        .unwrap_or_default();
    // REFEMU_CHARS=pass: log each character drawn ($14A4) during that pass.
    let chars_pass: Option<u64> = env::var("REFEMU_CHARS").ok().and_then(|v| v.parse().ok());
    let mut peek_last: Vec<Option<u8>> = vec![None; peek.len()];
    let dump_passes: Vec<u64> =
        env::var("REFEMU_DUMP").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let mut prof: BTreeMap<u32, u64> = BTreeMap::new();
    let mut ring: std::collections::VecDeque<(u16, u8, u8, u8, u8, bool, bool, i32)> = Default::default();
    let mut dumped = false;
    if args.len() < 3 {
        eprintln!(
            "usage: refemu disk1.nib disk2.nib disk3.nib [--plan PLAN] [--passes N] [--trace FILE] [--shot FILE]"
        );
        std::process::exit(2);
    }
    let disks: Vec<Disk> = args
        .iter()
        .map(|p| {
            let raw = fs::read(p).expect("read nib");
            assert_eq!(raw.len(), NIB_TRACK * 35, "{p}: not a 35-track .nib");
            Disk::from_nib(&raw)
        })
        .collect();
    let mut m = Machine::new(disks);
    let mut cpu = Cpu { s: 0xFF, p: 0x24, ..Default::default() };

    // What the disk card's boot ROM leaves behind after reading T0 S0 into $0800.
    let s0 = boot_sector(&m.disks[0], 0).expect("boot sector");
    m.main[0x800..0x900].copy_from_slice(&s0);
    m.main[0x26] = 0x00;
    m.main[0x27] = 0x09;
    m.main[0x2B] = 0x60;
    m.main[0x3D] = 0x00;
    cpu.x = 0x60;
    cpu.pc = 0x0801;

    let mut items: std::collections::VecDeque<String> =
        plan.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut wait_until: u64 = 0;
    let mut last_dir_key: u8 = 0;
    let mut passes: u64 = 0;
    let mut last_pass_cycle: Option<u64> = None;
    let mut deltas: Vec<(u8, u64)> = Vec::new(); // (room, cycles per pass)
    let mut trace = String::from("pass,cycles,delta,room,ego_x,ego_y,ego_dir,score,vars_hash");
    for (n, _, _) in EVENTS {
        trace += &format!(",{n}");
    }
    trace.push('\n');
    let mut counts = [0u64; EVENTS.len()];
    let mut first_pass_cycle = 0;
    let mut pass_polls: u64 = 0;
    let mut last_auto: u64 = 0;
    let mut prompt_keys: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut text_dump = env::var("REFEMU_TEXTDUMP").ok();
    // REFEMU_PLOTLOG=pass,file: "pass cycles" for every picture plot (aux $102F) in that
    // pass (0: in every pass).
    let plot_log: Option<(u64, String)> = env::var("REFEMU_PLOTLOG").ok().and_then(|v| {
        let (p, f) = v.split_once(',')?;
        Some((p.parse().ok()?, f.to_string()))
    });
    let mut plot_cycles: Vec<u64> = Vec::new();
    // REFEMU_PCLOG=hex,...: log the cycle count each time main-memory code reaches these
    // addresses (the first 20 times each).
    let pc_log: Vec<u16> = env::var("REFEMU_PCLOG")
        .map(|v| v.split(',').filter_map(|x| u16::from_str_radix(x.trim().trim_start_matches('$'), 16).ok()).collect())
        .unwrap_or_default();
    let mut pc_hits = vec![0u32; pc_log.len()];

    while passes < max_passes {
        if cpu.cycles > max_cycles {
            eprintln!(
                "cycle budget reached at PC ${:04X}; drive: disk {} halftrack {} motor {} q6 {} q7 {}",
                cpu.pc,
                m.cur_disk + 1,
                m.halftrack,
                m.motor,
                m.q6,
                m.q7
            );
            let mut top: Vec<_> = pc_hist.iter().collect();
            top.sort_by(|a, b| b.1.cmp(a.1));
            for (pc, n) in top.iter().take(12) {
                eprintln!("  ${pc:04X}: {n}");
            }
            for n in &m.notes {
                eprintln!("  note: {n}");
            }
            break;
        }
        if cpu.cycles > max_cycles.saturating_sub(2_000_000) {
            *pc_hist.entry(cpu.pc).or_default() += 1;
        }
        if profile && passes > 3 {
            let bank = if m.sw.ramrd { 0x10000u32 } else { 0 };
            *prof.entry(bank | (cpu.pc as u32 & 0xFF00)).or_default() += 1;
        }
        if let Some((lo, hi)) = f2_watch {
            if passes >= lo && passes <= hi {
                let now = m.main_at(0xF2);
                if Some(now) != f2_last {
                    eprintln!(
                        "f2: pass {passes} pc {}${:04X} -> {now} (actor ptr ${:02X}{:02X})",
                        if m.sw.ramrd { "aux " } else { "" },
                        f2_pc,
                        m.main_at(0x95),
                        m.main_at(0x94)
                    );
                    f2_last = Some(now);
                }
            }
        }
        f2_pc = cpu.pc;
        for (i, &a) in peek.iter().enumerate() {
            let now = m.main_at(a);
            if Some(now) != peek_last[i] {
                eprintln!(
                    "peek: pass {passes} ${a:04X} = ${now:02X} (pc {}${:04X})",
                    if m.sw.ramrd { "aux " } else { "" },
                    f2_pc
                );
                peek_last[i] = Some(now);
            }
        }
        ring.push_back((cpu.pc, cpu.a, cpu.x, cpu.y, cpu.s, m.sw.ramrd, m.sw.lc_read_ram, m.halftrack));
        if ring.len() > 80 {
            ring.pop_front();
        }
        if cpu.pc < 0x0100 && !dumped {
            dumped = true;
            eprintln!("execution reached ${:04X}; last instructions (pc a x y s ramrd lcram halftrack):", cpu.pc);
            for r in &ring {
                eprintln!("  {:04X} {:02X} {:02X} {:02X} {:02X} {} {} {}", r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7);
            }
        }
        if rng_log && cpu.pc == 0x65F1 && !m.sw.ramrd {
            let sp = 0x100 + cpu.s as u16;
            let ret = (m.main_at(sp + 1) as u16 | (m.main_at(sp + 2) as u16) << 8).wrapping_add(1);
            let next = (m.main_at(0x4C) as u16) << 8 | m.main_at(0x4B).wrapping_add(1) as u16;
            eprintln!(
                "rng: pass {passes} caller ${:04X} carry {} ptr ${:02X}{:02X} counter {:02X} byte@{next:04X} {:02X}",
                ret.wrapping_sub(3),
                cpu.p & 1,
                m.main_at(0x4C),
                m.main_at(0x4B),
                m.main_at(0x4D),
                m.main_at(next)
            );
        }
        if let Some((pp, _)) = &plot_log {
            if (*pp == 0 || passes == *pp) && cpu.pc == 0x102F && m.sw.ramrd {
                plot_cycles.push(passes << 48 | cpu.cycles);
            }
        }
        for (i, &(_, a, aux)) in EVENTS.iter().enumerate() {
            if cpu.pc == a && m.sw.ramrd == aux {
                counts[i] += 1;
                if a == 0x14A4 && chars_pass == Some(passes) {
                    eprintln!(
                        "char: pass {passes} A=${:02X} '{}' X=${:02X} Y=${:02X} ch={} cv={}",
                        cpu.a,
                        (cpu.a & 0x7F) as char,
                        cpu.x,
                        cpu.y,
                        m.main_at(0x24),
                        m.main_at(0x25)
                    );
                }
            }
        }
        // Like the port's headless front end: answer a wait (a "more" prompt, a Y/N
        // question) with RETURN when no pass has completed for a long time.
        // Only while the engine is polling the keyboard in a wait loop, not during a long
        // room load (that RETURN would be read as an empty typed line).
        // Plan keys held for a prompt (pHH): typed once the game is waiting for a key.
        if !prompt_keys.is_empty() && m.key_queue.is_empty() && !m.key_ready && m.key_polls > pass_polls + 10_000 {
            m.key_queue.push_back(prompt_keys.pop_front().unwrap());
            pass_polls = m.key_polls;
            last_auto = cpu.cycles;
        }
        // REFEMU_TEXTDUMP=FILE: the text page's character codes, the first time the game
        // waits for a key in text mode (24 lines of 40 hex codes).
        if let Some(p) = text_dump.as_ref() {
            if m.sw.text && passes > 0 && m.key_polls > pass_polls + 10_000 {
                let mut out = String::new();
                for row in 0..24u16 {
                    let base = 0x400 + (row & 7) * 0x80 + (row >> 3) * 0x28;
                    let codes: Vec<String> = (0..40).map(|c| format!("{:02X}", m.main[(base + c) as usize])).collect();
                    out += &codes.join(" ");
                    out.push(10 as char);
                }
                fs::write(p, out).expect("write text dump");
                eprintln!("text page dumped at pass {passes}");
                text_dump = None;
            }
        }
        if passes > 0
            && prompt_keys.is_empty()
            && m.key_queue.is_empty()
            && !m.key_ready
            && cpu.cycles > last_pass_cycle.unwrap_or(0).max(last_auto) + 3_000_000
            && m.key_polls > pass_polls + 10_000
        {
            // Measured from the last automatic RETURN too, so a wait right after a long
            // room load gets one RETURN, not several (the extra ones would be typed lines).
            m.key_queue.push_back(0x0D);
            last_auto = cpu.cycles;
            pass_polls = m.key_polls;
        }
        if !m.sw.ramrd {
            if let Some(i) = pc_log.iter().position(|&a| a == cpu.pc) {
                if pc_hits[i] < 20 {
                    pc_hits[i] += 1;
                    eprintln!(
                        "pc ${:04X} at cycle {} ({:.3} s), pass {passes}",
                        cpu.pc,
                        cpu.cycles,
                        cpu.cycles as f64 / CPU_HZ
                    );
                }
            }
        }
        cpu.step(&mut m);
        if let Some((at, op)) = cpu.bad_opcode {
            eprintln!("stopped: unsupported opcode ${op:02X} at ${at:04X} after {} cycles", cpu.cycles);
            for n in &m.notes {
                eprintln!("  {n}");
            }
            break;
        }
        // A pass of the engine's main loop, with main memory mapped in.
        if cpu.pc == MAIN_LOOP && !m.sw.ramrd {
            passes += 1;
            if passes == 1 {
                first_pass_cycle = cpu.cycles;
                eprintln!(
                    "engine main loop reached after {} cycles ({:.1} s at 1.02 MHz)",
                    cpu.cycles,
                    cpu.cycles as f64 / CPU_HZ
                );
            }
            if dump_passes.contains(&passes) {
                let vals: Vec<String> = (0..0xDFu16)
                    .map(|v| format!("{}", if v < 0x20 { m.main_at(0xD0 + v) } else { m.main_at(0x6E06 + v) }))
                    .collect();
                eprintln!("vars at start of pass {passes}: {}", vals.join(","));
                let act: Vec<String> = (0x100..0x100 + 31 * 4u16).map(|a| format!("{}", m.main_at(a))).collect();
                eprintln!("actors at start of pass {passes}: {}", act.join(","));
                eprintln!(
                    "f2f3 at start of pass {passes}: {},{},{},{},{},{},{},{},{}",
                    m.main_at(0xF2),
                    m.main_at(0xF3),
                    m.main_at(0x69),
                    m.main_at(0x6A),
                    m.main_at(0x6E),
                    m.main_at(0x76),
                    m.main_at(0x6D),
                    m.main_at(0x7020),
                    m.main_at(0x7021)
                );
            }
            if let Some(&k) = key_log.get(&passes) {
                m.key = k;
                m.key_ready = true;
            }
            let room = m.main_at(0xD1);
            let delta = last_pass_cycle.map_or(0, |l| cpu.cycles - l);
            if last_pass_cycle.is_some() {
                deltas.push((room, delta));
            }
            last_pass_cycle = Some(cpu.cycles);
            pass_polls = m.key_polls;
            let vars: u64 = (0..0x20u16)
                .map(|v| m.main_at(0xD0 + v) as u64)
                .chain((0x20..0xDFu16).map(|v| m.main_at(0x6E06 + v) as u64))
                .fold(1469598103934665603u64, |h, b| (h ^ b).wrapping_mul(1099511628211));
            trace += &format!(
                "{passes},{},{delta},{room},{},{},{},{},{vars:016x}",
                cpu.cycles,
                m.main_at(0x118),
                m.main_at(0x119),
                m.main_at(0x110),
                m.main_at(0xE3)
            );
            for c in counts.iter_mut() {
                trace += &format!(",{c}");
                *c = 0;
            }
            trace.push('\n');
            // Run the plan.
            if passes >= wait_until {
                while let Some(item) = items.pop_front() {
                    if let Some(n) = item.strip_prefix('w') {
                        wait_until = passes + n.parse::<u64>().unwrap_or(1);
                        break;
                    } else if let Some(text) = item.strip_prefix('>') {
                        for c in text.bytes() {
                            m.key_queue.push_back(c);
                        }
                        m.key_queue.push_back(0x0D);
                    } else if let Some(h) = item.strip_prefix('p') {
                        prompt_keys.push_back(u8::from_str_radix(h, 16).unwrap_or(0));
                    } else if let Some(h) = item.strip_prefix('k') {
                        m.key_queue.push_back(u8::from_str_radix(h, 16).unwrap_or(0));
                    } else if item == "S" {
                        m.key_queue.push_back(last_dir_key);
                    } else if let Some(k) = key_for(&item) {
                        last_dir_key = k;
                        m.key_queue.push_back(k);
                    } else {
                        eprintln!("plan: unknown item {item}");
                    }
                }
            }
        }
    }

    // Report: cycles per main-loop pass, overall and by room.
    if !deltas.is_empty() {
        let mut all: Vec<u64> = deltas.iter().map(|d| d.1).collect();
        all.sort_unstable();
        let pct = |p: f64| all[((all.len() - 1) as f64 * p) as usize];
        let mean = all.iter().sum::<u64>() as f64 / all.len() as f64;
        println!("main-loop passes measured: {}", all.len());
        println!(
            "cycles per pass: min {} p10 {} median {} p90 {} max {} mean {:.0}",
            all[0],
            pct(0.1),
            pct(0.5),
            pct(0.9),
            all[all.len() - 1],
            mean
        );
        println!("passes per second at 1.02 MHz: median {:.1}, mean {:.1}", CPU_HZ / pct(0.5) as f64, CPU_HZ / mean);
        let mut by_room: BTreeMap<u8, Vec<u64>> = BTreeMap::new();
        for (r, d) in &deltas {
            by_room.entry(*r).or_default().push(*d);
        }
        for (r, mut v) in by_room {
            v.sort_unstable();
            println!(
                "  room {r:3}: {} passes, median {} cycles ({:.1}/s)",
                v.len(),
                v[v.len() / 2],
                CPU_HZ / v[v.len() / 2] as f64
            );
        }
        println!("elapsed since first pass: {:.1} s emulated", (cpu.cycles - first_pass_cycle) as f64 / CPU_HZ);
    }
    if profile {
        let total: u64 = prof.values().sum();
        let mut v: Vec<_> = prof.into_iter().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        println!("instruction profile (by 256-byte page; aux pages marked a):");
        for (k, n) in v.iter().take(10) {
            println!(
                "  {}{:04X}: {:5.1}%",
                if k & 0x10000 != 0 { "a" } else { " " },
                k & 0xFFFF,
                100.0 * *n as f64 / total as f64
            );
        }
    }
    if let Some(p) = &save_out {
        if m.disks.len() >= 4 && m.disk_written[3] {
            fs::write(p, m.disks[3].to_nib()).expect("write save disk");
            println!("save disk written to {p}");
        } else {
            println!("the save disk was not written");
        }
    }
    println!("bells: {}, speaker clicks: {}", m.bells, m.speaker_clicks);
    for n in &m.notes {
        println!("note: {n}");
    }
    if let Some((_, f)) = &plot_log {
        let text: String = plot_cycles
            .iter()
            .map(|c| {
                format!(
                    "{} {}
",
                    c >> 48,
                    c & 0xFFFF_FFFF_FFFF
                )
            })
            .collect();
        fs::write(f, text).expect("write plot log");
    }
    if let Some(p) = trace_path {
        fs::write(&p, trace).expect("write trace");
    }
    if let Some(p) = buf_path {
        // The engine's picture buffer: aux $4000, 140 x 160 bytes (priority << 4 | colour).
        fs::write(&p, &m.aux[0x4000..0x4000 + 140 * 160]).expect("write picture buffer");
    }
    if let Some(p) = shot_path {
        fs::write(&p, screenshot(&m)).expect("write shot");
    }
}

/// Double hi-res page 1 as a 560x192 PNG. Colour is taken from each aligned group of
/// four dots, read lowest bit first (Apple IIe Technical Reference order).
fn screenshot(m: &Machine) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(560 * 192 * 3);
    for y in 0..192usize {
        let base = 0x2000 + (y & 7) * 0x400 + ((y >> 3) & 7) * 0x80 + (y >> 6) * 0x28;
        let mut dots = Vec::with_capacity(560);
        for col in 0..40 {
            for mem in [&m.aux, &m.main] {
                let b = mem[base + col];
                for i in 0..7 {
                    dots.push(b >> i & 1);
                }
            }
        }
        for g in dots.chunks(4) {
            let idx = (g[0] << 3) | (g[1] << 2) | (g[2] << 1) | g[3];
            const PAL: [u32; 16] = [
                0x000000, 0xDD0033, 0x885500, 0xFF6600, 0x007722, 0x555555, 0x11DD00, 0xFFFF00, 0x000099, 0xDD22DD,
                0xAAAAAA, 0xFF9988, 0x2222FF, 0x66AAFF, 0x44FF99, 0xFFFFFF,
            ];
            let c = PAL[idx as usize];
            for _ in 0..4 {
                rgb.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
            }
        }
    }
    kq1_apple2::png::encode_rgb(560, 192, &rgb)
}
