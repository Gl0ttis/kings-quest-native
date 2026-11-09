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

/// Draw each room's picture with the engine's routines and write the buffers.
fn draw_pictures(m: &mut Machine, cpu0: &Cpu, game: &kq1_pcjr::disk::Disk, dir: &str) {
    std::fs::create_dir_all(dir).expect("create the pictures folder");
    let mut drawn = 0;
    for r in 1..kq1_pcjr::disk::ROOMS {
        let Some(room) = game.room(r) else { continue };
        if room.script.len() <= 8 {
            continue;
        }
        let mut cpu = cpu0.clone();
        m.ram[0xF000..0x10000].fill(0xFF);
        let n = room.picture.len().min(0x1000);
        m.ram[0xF000..0xF000 + n].copy_from_slice(&room.picture[..n]);
        // The engine's own sequence for a room picture ($34FC): clear ($615), draw the
        // frame at DS:$1981 ($353F; fills stop at it), draw the picture ($3534). Run as a
        // few bytes placed in the boot code's place (0060:0010, unused once the engine
        // runs), ending at 0060:001C.
        let at = 0x600 + 0x10;
        let call = |from: u16, to: u16| (to.wrapping_sub(from + 3)).to_le_bytes();
        let mut code = vec![0xE8];
        code.extend(call(0x10, 0x0615));
        code.extend([0xBE, 0x81, 0x19]);
        code.push(0xE8);
        code.extend(call(0x16, 0x353F));
        code.push(0xE8);
        code.extend(call(0x19, 0x3534));
        m.ram[at..at + code.len()].copy_from_slice(&code);
        cpu.s[CS] = 0x0060;
        cpu.ip = 0x0010;
        let start = cpu.cycles;
        while (cpu.s[CS], cpu.ip) != (0x0060, 0x001C) && cpu.fault.is_none() && cpu.cycles - start < 200_000_000 {
            m.step(&mut cpu);
        }
        if let Some(f) = &cpu.fault {
            eprintln!("room {r}: {f}");
            continue;
        }
        if (cpu.s[CS], cpu.ip) != (0x0060, 0x001C) {
            eprintln!("room {r}: the drawing didn't finish");
            continue;
        }
        let mut buf = Vec::with_capacity(160 * 168);
        for y in 0..168usize {
            let row = (y & 3) * 0x2000 + (y >> 2) * 160;
            for x in 0..160 {
                let (v, p) = (m.ram[0x18000 + row + x] & 0x0F, m.ram[0x10000 + row + x] & 0x0F);
                buf.push(p << 4 | v);
            }
        }
        std::fs::write(format!("{dir}/{r:03}.bin"), buf).expect("write a picture");
        drawn += 1;
        if r == 1 {
            println!("room 1 drawn in {:.2} s", (cpu.cycles - start) as f64 / CPU_HZ);
        }
    }
    println!("{drawn} pictures drawn by the engine into {dir}");
}

/// One plan item: a key or keys, or a disk swap (`@play`, `@master`).
fn apply(item: &str, m: &mut Machine) {
    match item {
        "@master" => m.insert(0),
        "@play" => m.insert(1),
        // The last arrow again: the ego stops.
        "S" => {
            let k = m.last_arrow;
            m.keys.push_back(k)
        }
        _ => match key_for(item) {
            Some(ks) => {
                if let Some(&a) = ks.iter().find(|&&k| matches!(k, 0x4800 | 0x5000 | 0x4B00 | 0x4D00)) {
                    m.last_arrow = a;
                }
                m.keys.extend(ks)
            }
            None => eprintln!("plan: unknown item {item}"),
        },
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = args.iter().position(|a| a == name)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let wait_states: u64 = opt("--wait-states").map_or(2, |v| v.parse().expect("--wait-states"));
    let ticks: u64 = opt("--ticks").map_or(u64::MAX / CYCLES_PER_TICK, |v| v.parse().expect("--ticks"));
    let max_passes: u64 = opt("--passes").map_or(400, |v| v.parse().expect("--passes"));
    // --plan PLAN, or --plan @FILE: a route file, items separated by commas or lines,
    // with # comments.
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
    // --play FILE: the Play diskette, a 360K sector image (read if it exists; written
    // back if the game wrote to it).
    let play_path = opt("--play");
    // --pictures DIR: once the engine has started, draw every room's picture with the
    // engine's own code (clear `$615`, the frame `$353F`, the picture `$3534` from
    // linear F000) and write each buffer to DIR/NNN.bin: 168 rows of 160 bytes,
    // priority << 4 | colour.
    let pictures_dir = opt("--pictures");

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
    let game = kq1_pcjr::disk::Disk::from_file(raw.clone()).ok();
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

    let mut disk = disk;
    disk.write_protected = true;
    let mut m = Machine::new(disk);
    m.wait_states = wait_states;
    let mut play = Disk::default();
    if let Some(raw) = play_path.as_ref().and_then(|p| std::fs::read(p).ok()) {
        for (i, s) in raw.chunks(512).enumerate() {
            play.sectors.insert(((i / 18) as u8, (i / 9 % 2) as u8, (i % 9 + 1) as u8, 2), s.to_vec());
        }
    }
    m.disks.push(play);
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
        if (cpu.s[CS], cpu.ip) == MAIN_LOOP && passes == 0 {
            if let (Some(dir), Some(game)) = (&pictures_dir, &game) {
                draw_pictures(&mut m, &cpu, game, dir);
                return;
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
            if std::env::var_os("REFEMU_KEYLOG").is_some() && passes.is_multiple_of(10) {
                eprintln!("pass {passes} at cycle {}", cpu.cycles);
            }
            if passes >= wait_until {
                while items.front().is_some_and(|i| !i.starts_with('p')) {
                    let item = items.pop_front().unwrap();
                    if let Some(n) = item.strip_prefix('w') {
                        wait_until = passes + n.parse::<u64>().unwrap_or(1);
                        break;
                    }
                    apply(&item, &mut m);
                }
            }
        }
        // A game polling the keyboard with no pass for a while is at a prompt, which holds
        // the game cycle (a pass is about 300,000 cycles). A `p` item is typed there after
        // 1.5M cycles; with none, after 3M cycles it gets RETURN, as refemu does for the
        // Apple ("press a key"). The keyboard is polled every tick by the game's timer
        // handler, so polling alone doesn't mean a prompt.
        if m.idle_polls > 0 && m.keys.is_empty() {
            let since = *wait_since.get_or_insert(cpu.cycles);
            let waited = cpu.cycles - since;
            // "Please insert your Play / Master diskette in the drive and press Enter":
            // done as a player would, once the game is held at the prompt.
            let screen: String = m.text.iter().map(|r| String::from_utf8_lossy(r).to_string()).collect();
            let swap = if screen.contains("insert your   Play") {
                Some(1)
            } else if screen.contains("insert your  Master") {
                Some(0)
            } else {
                None
            };
            if let Some(d) = swap.filter(|_| m.blocked_reads > 0) {
                if std::env::var_os("REFEMU_KEYLOG").is_some() {
                    eprintln!("disk swap: {} in the drive, Enter", if d == 1 { "Play" } else { "Master" });
                }
                m.insert(d);
                m.keys.push_back(0x1C0D);
                wait_since = None;
                m.idle_polls = 0;
                m.blocked_reads = 0;
                m.step(&mut cpu);
                continue;
            }
            let typed = items.front().filter(|i| i.starts_with('p')).cloned();
            if let Some(item) = typed.filter(|_| waited > 1_500_000) {
                items.pop_front();
                // At a prompt, inserting a disk means pressing Enter after it.
                apply(&item[1..], &mut m);
                if item[1..].starts_with('@') {
                    m.keys.push_back(0x1C0D);
                }
                wait_since = None;
                m.idle_polls = 0;
            } else if waited > 3_000_000
                && (m.blocked_reads > 0 || passes == 0)
                && !items.front().is_some_and(|i| i.starts_with('p'))
            {
                if std::env::var_os("REFEMU_KEYLOG").is_some() {
                    eprintln!("auto RETURN at pass {passes} cycle {}", cpu.cycles);
                }
                m.keys.push_back(0x1C0D);
                wait_since = None;
                m.idle_polls = 0;
                m.blocked_reads = 0;
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
    println!(
        "video mode {} CRT page {}; {} sound chip writes; wait states {} a shared-RAM access, {:.0}% of the time",
        m.video_mode,
        m.crt_page,
        m.sound.len(),
        m.wait_states,
        100.0 * m.wait_cycles as f64 / cpu.cycles.max(1) as f64
    );
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
    if let (Some(p), Some(play)) = (&play_path, m.disks.get(1)) {
        if play.written {
            let mut img = vec![0xF6u8; 368_640];
            for (&(c, h, r, n), data) in &play.sectors {
                if n == 2 && c < 40 && h < 2 && (1..=9).contains(&r) {
                    let lin = (c as usize * 2 + h as usize) * 9 + r as usize - 1;
                    img[lin * 512..lin * 512 + 512].copy_from_slice(data);
                }
            }
            std::fs::write(p, img).expect("write the Play diskette");
            println!("Play diskette written to {p}");
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
