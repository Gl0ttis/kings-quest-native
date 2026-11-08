// savetest KEYS DISK_IN DISK_OUT SAVE_PASS RESTORE_PASS TOTAL TRACE: replay a key log
// (keys after RESTORE_PASS dropped), save to slot A on the save-disk image at SAVE_PASS
// (0: no save), restore slot A at RESTORE_PASS, run to TOTAL passes and write a trace;
// the disk image goes to DISK_OUT.
use kq1_apple2::engine::{f, IoImpl, Resources, World};
use kq1_apple2::savedisk::SaveDisk;
use std::collections::VecDeque;
use std::{cell::RefCell, rc::Rc};

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

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let imgs: Vec<Vec<u8>> = (1..=3).map(|n| std::fs::read(format!("extracted/disk{n}.do")).unwrap()).collect();
    let keys: std::collections::HashMap<usize, u8> = std::fs::read_to_string(&a[1])
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .collect();
    let disk = Rc::new(RefCell::new(SaveDisk::open(std::fs::read(&a[2]).unwrap()).unwrap()));
    let (save, restore, total): (usize, usize, usize) =
        (a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap());
    let q = Rc::new(RefCell::new(VecDeque::new()));
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), false).unwrap());
    w.io = Some(Box::new(DiskIo { keys: q.clone(), disk: disk.clone() }));
    w.boot();
    let mut trace = String::from("pass,room,ego_x,ego_y,ego_dir,score,model_cycles,vars_hash\n");
    for p in 1..=total {
        if restore == 0 || p <= restore {
            if let Some(&k) = keys.get(&p) {
                w.key(k);
            }
        }
        if p == save {
            w.key(b'1');
            q.borrow_mut().push_back(b'a');
        }
        if p == restore {
            w.key(b'3');
            q.borrow_mut().push_back(b'a');
        }
        w.cycle();
        if std::env::var("KQ_DUMP").is_ok_and(|v| v.split(',').any(|x| x.parse::<usize>() == Ok(p))) {
            let vals: Vec<String> = w.vars[..0xDF].iter().map(|v| v.to_string()).collect();
            eprintln!("vars after pass {p}: {}", vals.join(","));
        }
        let hash =
            w.vars[..0xDF].iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211));
        trace += &format!(
            "{p},{},{},{},{},{},0,{hash:016x}\n",
            w.vars[1],
            w.actors[0][f::X],
            w.actors[0][f::Y],
            w.actors[0][f::DIR],
            w.vars[19]
        );
    }
    std::fs::write(&a[7], trace).unwrap();
    std::fs::write(&a[3], disk.borrow().bytes()).unwrap();
    for l in w.log.iter().filter(|l| l.contains("save") || l.contains("restor") || l.contains("Restor")) {
        println!("{l}");
    }
}
