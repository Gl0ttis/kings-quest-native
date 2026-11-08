// plotruns KEYS PASS (or: plotruns pic N): replay a key log to PASS and print the drawing runs of the pictures
// drawn in that pass ("plots_before kind", one per drawing command).
use kq1_apple2::engine::{Resources, Scripted, World};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "pic" {
        // plotruns pic N: the runs of picture N drawn on its own.
        let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
        let res = Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap();
        let p = kq1_apple2::pic::draw(&res.pictures[a[2].parse::<usize>().unwrap()].clone().unwrap());
        println!("# pass 0 plots {}", p.plots);
        for (n, fill) in &p.runs {
            println!("{n} {}", if *fill { "fill" } else { "line" });
        }
        return;
    }
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let keys: std::collections::HashMap<usize, u8> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    // PASS 0: every pass that draws a picture, each block headed "# pass P plots N".
    let pass: usize = a[2].parse().unwrap();
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    let last = if pass == 0 { a.get(3).map_or(17339, |v| v.parse().unwrap()) } else { pass };
    for p in 1..=last {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
        if pass != 0 && p != pass {
            w.take_cost();
            continue;
        }
        for d in &w.pic_draws {
            println!("# pass {p} plots {} before {:.0}", d.plots, d.before.cycles());
            for (n, fill) in &d.runs {
                println!("{n} {}", if *fill { "fill" } else { "line" });
            }
        }
        w.take_cost();
    }
}
