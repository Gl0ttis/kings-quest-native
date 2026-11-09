//! kqplotlog-pcjr DISK EVENTS OUT: the original's room-picture drawing, timed. The event
//! log (kqroute-pcjr `--events`) is followed in the original (`Runner::follow`) to its
//! last pass, and each room picture's drawing is logged to OUT (machine.rs,
//! `REFEMU_PLOTLOG`): its start, the bytes written to the displayed page as it goes, by
//! cycle, and the colours merged. For timing the port's drawing as it is shown.

use kq1_pcjr_refemu::runner::{Runner, Stopped};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        eprintln!("usage: kqplotlog-pcjr DISK EVENTS OUT");
        std::process::exit(2);
    }
    std::env::set_var("REFEMU_PLOTLOG", &a[2]);
    let raw = std::fs::read(&a[0]).expect("disk");
    let events = std::fs::read_to_string(&a[1]).expect("events");
    let passes = events.lines().filter(|l| l.starts_with("P ")).count() as u64;
    let mut r = Runner::new(&raw).expect("boot");
    while r.passes < passes {
        match r.follow(&events) {
            Ok(Stopped::Pass | Stopped::Prompt) => {}
            Err(e) => {
                eprintln!("stopped: {e}");
                break;
            }
        }
    }
    let log = r.m.plot_log.take().unwrap_or_default();
    let pictures = log.lines().filter(|l| l.starts_with('S')).count();
    std::fs::write(&a[2], log).expect("write the log");
    println!("{} passes, {pictures} pictures", r.passes);
}
