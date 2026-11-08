//! Saves on the original's save disk (disk 4), against the original in refemu. Needs
//! extracted/ (the decoded game disks, original/disk4.nib, and the reference files);
//! skipped when they are absent.
//!
//! The reference: refemu replayed routes/max_score.keys, pressed 1 and A at pass 2001
//! (save to letter A on a blank disk 4: extracted/reference/save_disk_pass2001.nib),
//! went on to pass 2300, then pressed 3 and A (restore) and ran 100 more passes
//! (extracted/reference/restore_pass2301.csv). The original also restored a save the
//! port wrote and gave the same 100 passes (checked once with refemu; see
//! docs/save-format.md).
use kq1_apple2::engine::{f, IoImpl, Resources, World};
use kq1_apple2::savedisk::SaveDisk;
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fs,
    rc::Rc,
};

const BASE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/");

struct DiskIo {
    keys: Rc<RefCell<VecDeque<u8>>>,
    disk: Rc<RefCell<SaveDisk>>,
}
impl IoImpl for DiskIo {
    fn wait_key(&mut self, _f: &[u32]) -> u8 {
        self.keys.borrow_mut().pop_front().unwrap_or(0x0D)
    }
    fn bell(&mut self) {}
    fn save(&mut self, slot: u8, data: &[u8]) -> Result<(), String> {
        self.disk.borrow_mut().save_game(slot, data)
    }
    fn load(&mut self, slot: u8) -> Option<Vec<u8>> {
        self.disk.borrow().load_game(slot)
    }
}

/// Replay the route (keys to pass 2300), saving to A at `save` and restoring A at 2301;
/// returns (room, x, y, score, vars hash) after each of passes 2301-2400, and the disk.
fn run(disk: Vec<u8>, save: Option<usize>) -> Option<(Vec<(u8, u8, u8, u8, String)>, Vec<u8>)> {
    let imgs: Option<Vec<Vec<u8>>> = (1..=3).map(|n| fs::read(format!("{BASE}extracted/disk{n}.do")).ok()).collect();
    let keys: HashMap<usize, u8> = fs::read_to_string(format!("{BASE}routes/max_score.keys"))
        .ok()?
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let disk = Rc::new(RefCell::new(SaveDisk::open(disk).ok()?));
    let q = Rc::new(RefCell::new(VecDeque::new()));
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs?), false).ok()?);
    w.io = Some(Box::new(DiskIo { keys: q.clone(), disk: disk.clone() }));
    w.boot();
    let mut out = Vec::new();
    for p in 1..=2400 {
        if p <= 2301 {
            if let Some(&k) = keys.get(&p) {
                w.key(k);
            }
        }
        if Some(p) == save {
            w.key(b'1');
            q.borrow_mut().push_back(b'a');
        }
        if p == 2301 {
            w.key(b'3');
            q.borrow_mut().push_back(b'a');
        }
        w.cycle();
        if p > 2300 {
            let hash =
                w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
            out.push((w.vars[1], w.actors[0][f::X], w.actors[0][f::Y], w.vars[19], format!("{hash:016x}")));
        }
    }
    let bytes = disk.borrow().bytes().to_vec();
    Some((out, bytes))
}

fn reference() -> Option<Vec<(u8, u8, u8, u8, String)>> {
    let csv = fs::read_to_string(format!("{BASE}extracted/reference/restore_pass2301.csv")).ok()?;
    // Trace row k + 1 is the port's pass k.
    Some(
        csv.lines()
            .skip(1)
            .filter_map(|l| {
                let c: Vec<&str> = l.split(',').collect();
                let pass: usize = c[0].parse().ok()?;
                (2302..=2401).contains(&pass).then(|| {
                    (
                        c[3].parse().unwrap(),
                        c[4].parse().unwrap(),
                        c[5].parse().unwrap(),
                        c[7].parse().unwrap(),
                        c[8].to_string(),
                    )
                })
            })
            .collect(),
    )
}

#[test]
fn restore_of_the_originals_save_matches_the_original() {
    let (Some(disk), Some(want)) =
        (fs::read(format!("{BASE}extracted/reference/save_disk_pass2001.nib")).ok(), reference())
    else {
        return;
    };
    let Some((got, _)) = run(disk, None) else { return };
    assert_eq!(got.len(), 100);
    for (i, (g, r)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g, r, "pass {} after restoring the original's save", 2301 + i);
    }
}

#[test]
fn the_ports_save_restores_like_the_originals() {
    let (Some(blank), Some(want)) = (fs::read(format!("{BASE}original/disk4.nib")).ok(), reference()) else { return };
    let Some((got, disk)) = run(blank, Some(2001)) else { return };
    for (i, (g, r)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g, r, "pass {} after restoring the port's save", 2301 + i);
    }
    // The port's save holds the same game as the original's, block for block, apart
    // from fields the port does not model.
    let mine = SaveDisk::open(disk).unwrap().load_game(1).unwrap();
    let theirs = SaveDisk::open(fs::read(format!("{BASE}extracted/reference/save_disk_pass2001.nib")).unwrap())
        .unwrap()
        .load_game(1)
        .unwrap();
    let vars = 8 + 0x500 + 0x7C + 0x90;
    assert_eq!(mine[vars..], theirs[vars..], "variables v32-v222");
}
