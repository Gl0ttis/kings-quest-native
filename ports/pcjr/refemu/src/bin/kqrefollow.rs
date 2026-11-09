//! kqrefollow-pcjr DISK OLD.events OUT.events OUT.csv [--checkpoints]: run a recorded route again on the
//! original, after a change to the emulator that moves its timing (where the timer's
//! ticks land), and record it anew. The old log's typed keys are typed at the same
//! stops (`Runner::run`'s: the start of pass n, or the next prompt) as when it was
//! recorded, whatever the ticks do; the keys refemu-pcjr presses itself (at the title,
//! after a disk swap) it presses again. The new run's event log and trace are written,
//! and its game state compared with the old trace (`OLD.csv` next to `OLD.events`), pass
//! by pass: the route still plays the same if they agree. `--checkpoints`: for a log
//! whose keys were pressed at their checkpoints (gold_key's, `Runner::replay`).

use kq1_pcjr_refemu::runner::{Runner, Stopped};
use std::collections::VecDeque;

/// Where a group of keys was typed.
#[derive(Debug, PartialEq)]
enum Stop {
    Pass(u64),
    Prompt,
}

/// The typed keys of a log, by the stop they were typed at.
fn typed(log: &str) -> VecDeque<(Stop, Vec<u16>)> {
    let mut groups: VecDeque<(Stop, Vec<u16>)> = VecDeque::new();
    let (mut passes, mut prev) = (0u64, "");
    for l in log.lines() {
        let mut f = l.split_whitespace();
        let kind = f.next().unwrap_or("");
        match kind {
            "P" => passes += 1,
            "K" if passes > 0 && prev != "D" => {
                let k = f.nth(1).and_then(|h| u16::from_str_radix(h, 16).ok()).expect("a key");
                match prev {
                    "K" if !groups.is_empty() => groups.back_mut().unwrap().1.push(k),
                    "P" => groups.push_back((Stop::Pass(passes), vec![k])),
                    _ => groups.push_back((Stop::Prompt, vec![k])),
                }
            }
            // A swap's RETURN, refemu's own: still after the swap.
            "K" => {}
            _ => {}
        }
        // A key refemu pressed itself keeps the next one out of its group.
        prev = if kind == "K" && (passes == 0 || prev == "D") { "D" } else { kind };
    }
    groups
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let at_checkpoints = a.iter().position(|x| x == "--checkpoints").map(|p| a.remove(p)).is_some();
    if a.len() != 4 {
        eprintln!("usage: kqrefollow-pcjr DISK OLD.events OUT.events OUT.csv [--checkpoints]");
        std::process::exit(2);
    }
    let raw = std::fs::read(&a[0]).expect("disk");
    let old = std::fs::read_to_string(&a[1]).expect("old events");
    let old_trace = std::fs::read_to_string(a[1].replace(".events", ".csv")).unwrap_or_default();
    let passes = old.lines().filter(|l| l.starts_with("P ")).count() as u64;
    let mut groups = typed(&old);
    let mut r = Runner::new(&raw).expect("boot");
    if at_checkpoints {
        r.replay(&old);
        groups.clear();
    }
    let mut why = None;
    while r.passes < passes {
        let step = if at_checkpoints { r.run(u64::MAX, 4_000_000_000) } else { r.run(1_500_000, 200_000_000) };
        match step {
            Ok(Stopped::Pass) => {
                if let Some((Stop::Pass(n), _)) = groups.front() {
                    if *n == r.passes {
                        for k in groups.pop_front().unwrap().1 {
                            r.press(k);
                        }
                    } else if *n < r.passes {
                        why = Some(format!("pass {}: the keys of pass {n} weren't typed", r.passes));
                        break;
                    }
                }
            }
            Ok(Stopped::Prompt) => match groups.front() {
                Some((Stop::Prompt, _)) => {
                    for k in groups.pop_front().unwrap().1 {
                        r.press(k);
                    }
                }
                g => {
                    why = Some(format!(
                        "pass {}: a prompt the old run didn't stop at (next: {:?})",
                        r.passes,
                        g.map(|g| &g.0)
                    ));
                    break;
                }
            },
            Err(e) => {
                why = Some(format!("pass {}: {e}", r.passes));
                break;
            }
        }
    }
    std::fs::write(&a[2], r.events()).expect("write the events");
    std::fs::write(&a[3], &r.trace).expect("write the trace");
    // The game state (room, ego, direction, score, variables), pass by pass.
    let state = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| !l.starts_with("pass"))
            .map(|l| l.split(',').skip(3).collect::<Vec<_>>().join(","))
            .collect()
    };
    let (o, n) = (state(&old_trace), state(&r.trace));
    let first = o.iter().zip(&n).position(|(a, b)| a != b);
    println!(
        "{} passes of {passes}{}; {} keys left; state against the old trace: {}",
        r.passes,
        why.map_or(String::new(), |w| format!(" (stopped: {w})")),
        groups.iter().map(|g| g.1.len()).sum::<usize>(),
        match (first, o.is_empty()) {
            (_, true) => "no old trace".to_string(),
            (None, _) => format!("the same on all {} passes", o.len().min(n.len())),
            (Some(i), _) => format!("first differs at pass {}", i + 1),
        }
    );
}
