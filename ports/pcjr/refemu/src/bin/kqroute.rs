//! kqroute-pcjr DISK ROUTE [--events OUT] [--trace OUT] [--keys OUT] [--resume EVENTS PASS LINE]
//!
//! Plays a route through the original PCjr game, in refemu-pcjr, with an autopilot, and
//! writes the event log and the trace of that run: the reference the port is replayed
//! against (tests/engine.rs). It is the Apple port's `kqroute`, reading the original's
//! own memory, with the PCjr's geometry (src/engine/sprite.rs, `move_actor`).
//!
//! Route file, one step per line (# starts a comment):
//!   room N [X1 Y1 X2 Y2]  travel to room N through edge exits, arriving where the
//!                     rectangle (if given) can be reached
//!   goto X Y [TOL]    walk to (X, Y) in the current room, within TOL pixels (default 2)
//!   area X1 Y1 X2 Y2  walk into a rectangle (as in the scripts' "ego in" tests)
//!   enter ROOM X1 Y1 X2 Y2 [water]  walk into a doorway until the room changes
//!   edge L|R|U|D      walk off that edge of the current room
//!   hold U|D|L|R      keep walking that way for a pass
//!   say TEXT          type a command and RETURN
//!   prompt TEXT       the keys to type at the next prompt (the save dialog's letter)
//!   key HHHH          press a raw key (scan << 8 | character, hex)
//!   wait N            let N passes go by
//!   waitvar V VAL [MAX]  let passes go by until variable V equals VAL
//!   require V VAL     a condition set by chance (the line is retried with other timing,
//!                     or after round trips to a neighbouring room, which draw v14 anew)
//!   keepin X1 Y1 X2 Y2 ...  keep the planner in these rectangles in this room
//!   avoid X1 Y1 X2 Y2 ...   keep the planner out of these rectangles in this room
//!   robbable          a score lost in this room (a robbery that can't be helped) is
//!                     not retried
//!   final X1 Y1 X2 Y2  walk into the game-ending rectangle
//!   dies N            wait up to N passes for Graham to die
//!   if V VAL STEP     the step only when variable V equals VAL
//!   map [Y]           print the planner's map of the room, or row Y's priorities (stderr)
//!   expect score N | expect room N
//!
//! The autopilot is crates/kq1-autopilot's, shared with the other IBM port's; this gives
//! it the original PCjr game, in refemu-pcjr, and the engine's data addresses.

use kq1_pcjr::disk::Disk;
use kq1_pcjr_refemu::runner::Runner;

fn main() {
    kq1_autopilot::main("kqroute-pcjr", |path| {
        let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let disk = Disk::from_file(raw.clone()).expect("the game disk");
        let cs = disk.code_segment();
        let ds = &cs[kq1_pcjr::vocab::DATA_SEGMENT..];
        // The engine's data addresses (docs/engine.md).
        let port = kq1_autopilot::Port {
            actors: 0x11E1,
            game_over: 0x125A,
            key_dir: 0x1279,
            program: 0x1262,
            actor_count: 0x125B,
            message: 0x0E98,
            loops2: ds[0x174F..0x174F + 9].try_into().unwrap(),
            loops4: ds[0x1758..0x1758 + 9].try_into().unwrap(),
            frame: ds[kq1_pcjr::pic::FRAME..kq1_pcjr::pic::FRAME + 11].to_vec(),
            listings: (1..=84u8).filter_map(|r| kq1_pcjr::script::listing(&disk, r as usize).map(|t| (r, t))).collect(),
            pictures: (0..=84).filter_map(|r| disk.room(r).map(|x| (r, x.picture.to_vec()))).collect(),
            views: (0..=255).filter_map(|v| disk.view(v).map(|raw| (v, raw.to_vec()))).collect(),
        };
        #[allow(unused_mut)]
        let mut r = Runner::new(&raw).expect("refemu");
        (r, port)
    });
}
