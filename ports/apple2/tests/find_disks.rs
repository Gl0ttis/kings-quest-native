//! Finding the game disks in a folder (kq with no disk arguments). Needs the original
//! .nib images in original/; skipped when they are absent.

use kq1_apple2::kq::Disks;
use std::path::Path;

#[test]
fn finds_the_three_game_disks_in_order() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("original");
    if !dir.join("disk1.nib").exists() {
        eprintln!("skipped: no original disks");
        return;
    }
    let found = Disks::find(&dir).expect("find");
    assert_eq!(found.len(), 3);
    // Whatever their names, they come in disk order (by the volume each records), and
    // the save disks in the same folder are passed over.
    let disks = Disks::from_files(&found).expect("load");
    let ordered = Disks::from_files(&[dir.join("disk1.nib"), dir.join("disk2.nib"), dir.join("disk3.nib")]).unwrap();
    for i in 0..3 {
        assert!(disks.images[i] == ordered.images[i], "disk {}", i + 1);
    }
}

#[test]
fn reports_missing_disks() {
    let dir = std::env::temp_dir().join(format!("kq-find-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let e = Disks::find(&dir).unwrap_err();
    std::fs::remove_dir(&dir).ok();
    assert!(e.contains("no game disks in"), "{e}");
}
