//! The text screen against the original: the king's speech ("speak king" in room 53,
//! typed at pass 6301 of the max-score route), whose text page refemu dumped
//! (REFEMU_TEXTDUMP) into extracted/reference/textpage_speak_king.txt. Skipped when
//! extracted/ is absent.
use kq1_apple2::engine::{Resources, Scripted, World};
use std::{collections::HashMap, fs};

#[test]
fn king_speech_text_page_matches_original() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/");
    let Some(imgs) = (1..=3).map(|n| fs::read(format!("{base}extracted/disk{n}.do")).ok()).collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let Ok(dump) = fs::read_to_string(format!("{base}extracted/reference/textpage_speak_king.txt")) else { return };
    let mut keys: HashMap<usize, u8> = fs::read_to_string(format!("{base}routes/max_score.keys"))
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (p, k) = l.split_once(' ')?;
            Some((p.parse().ok()?, u8::from_str_radix(k.trim(), 16).ok()?))
        })
        .filter(|&(p, _)| p < 6300)
        .collect();
    for (i, c) in b"speak king\r".iter().enumerate() {
        keys.insert(6301 + i, *c);
    }
    let mut w = World::new(Resources::load(&kq1_apple2::kq::Disks::new(imgs), true).unwrap());
    w.io = Some(Box::new(Scripted::default()));
    w.boot();
    for p in 1..=6311 {
        if let Some(&k) = keys.get(&p) {
            w.key(k);
        }
        w.cycle();
    }
    // Rows 0-22; refemu's COUT stand-in leaves 00 where the ROM's would have wrapped.
    for (r, line) in dump.lines().take(23).enumerate() {
        for (c, h) in line.split(' ').enumerate() {
            let want = u8::from_str_radix(h, 16).unwrap();
            if want != 0 {
                assert_eq!(w.text.page[r][c], want, "row {r} column {c}");
            }
        }
    }
    // Upper case shows as inverse capitals, like everything else the game prints.
    assert_eq!(kq1_apple2::engine::text::displayed(w.text.page[1][5]), (b'W', true));
}
