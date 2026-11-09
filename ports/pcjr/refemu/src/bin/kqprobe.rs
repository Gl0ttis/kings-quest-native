//! kqprobe-pcjr DISK EVENTS FROM TO [ADDR...]: replay refemu-pcjr's event log in the
//! original and print its state at the start of passes FROM-TO: the ego (x, y, motion,
//! direction), the key direction (DS:1279), the joystick's (DS:1278), the BIOS buffer,
//! and any other data-segment bytes asked for (hex addresses). For finding where the
//! port parts from the original.

use kq1_pcjr_refemu::runner::{Runner, Stopped};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("usage: kqprobe-pcjr DISK EVENTS FROM TO [ADDR...]");
        std::process::exit(2);
    }
    let raw = std::fs::read(&a[0]).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let (from, to): (u64, u64) = (a[2].parse().expect("FROM"), a[3].parse().expect("TO"));
    let extra: Vec<u16> = a[4..].iter().map(|h| u16::from_str_radix(h, 16).expect("hex address")).collect();
    let mut r = Runner::new(&raw).expect("boot");
    r.replay(&events);
    while r.passes < to {
        match r.run(u64::MAX, 4_000_000_000) {
            Ok(Stopped::Pass) => {}
            Ok(Stopped::Prompt) => {}
            Err(e) => {
                eprintln!("stopped: {e}");
                break;
            }
        }
        if r.passes >= from {
            let e = 0x11E1u16;
            let more: Vec<String> = extra.iter().map(|&x| format!("{x:04X}={:02X}", r.ds(x))).collect();
            println!(
                "pass {} ego ({}, {}) motion {:02X} dir {} key_dir {} joy {} bios {:04X?} {}",
                r.passes,
                r.ds(e + 0x18),
                r.ds(e + 0x19),
                r.ds(e + 0x0F),
                r.ds(e + 0x10),
                r.ds(0x1279),
                r.ds(0x1278),
                r.m.keys,
                more.join(" ")
            );
        }
    }
}
