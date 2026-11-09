//! kqframes-pc DISK EVENTS --hashes OUT [--trace OUT]: the screen's hash at every pass of a
//! run kqroute-pc recorded (its `--events` log), for the reference (reference/*.frames).
//!
//! The run is followed exactly (runner `follow`: the log's keys typed where the run
//! stood when they were typed), and at the start of every pass the screen (runner
//! `frame`) is hashed (FNV-1a), one a line in hex, as kqplan-pc's `--frames`. The run
//! must stay on the log (it is checked): `--trace` writes the followed run's trace,
//! which is the recorded one's.

use kq1_pc_refemu::runner::{Runner, Stopped};

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let p = a.iter().position(|x| x == name)?;
        let v = a.get(p + 1).cloned();
        a.drain(p..(p + 2).min(a.len()));
        v
    };
    let (hashes, trace) = (opt("--hashes"), opt("--trace"));
    let ([disk, events], Some(hashes)) = (a.as_slice(), hashes) else {
        eprintln!("usage: kqframes-pc DISK EVENTS --hashes OUT [--trace OUT]");
        std::process::exit(2);
    };
    let raw = std::fs::read(disk).unwrap_or_else(|e| panic!("{disk}: {e}"));
    let log = std::fs::read_to_string(events).unwrap_or_else(|e| panic!("{events}: {e}"));
    let passes = log.lines().filter(|l| l.starts_with("P ")).count() as u64;
    let mut r = Runner::new(&raw).unwrap_or_else(|e| panic!("{e}"));
    let mut out = String::new();
    while r.passes < passes {
        match r.follow(&log) {
            Ok(Stopped::Pass) => {
                let h =
                    r.frame().iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
                out += &format!("{h:016x}\n");
            }
            Ok(Stopped::Prompt) => {}
            Err(e) => panic!("pass {}: {e}", r.passes),
        }
    }
    std::fs::write(&hashes, out).expect("write the hashes");
    if let Some(p) = trace {
        std::fs::write(p, &r.trace).expect("write the trace");
    }
    eprintln!("{} passes followed", r.passes);
}
