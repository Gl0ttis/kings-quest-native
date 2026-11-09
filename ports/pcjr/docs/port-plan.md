# Port plan: native King's Quest (IBM PCjr, 1984)

## Goal and constraints

The goal is a native Windows and Linux program, written in Rust, that plays the 1984 IBM
PCjr King's Quest as the original does, on every pass and on screen, at its speed. The
ground rules are the repository's ([docs/porting-guide.md](../../../docs/porting-guide.md)):
- no 8088 emulation in the port;
- the player's own disk image;
- no IBM ROM;
- no game content in git;
- the original's quirks reproduced.

The order of work follows the guide's phases, with the Apple port's lesson applied:
the measuring emulator comes before the engine, and screen comparison as soon as the
engine draws.

## Where we are

| Phase | Status |
|---|---|
| 1. Disk image | **Done.** The original as dumped (HxC `.mfm`: MFM decoded, CRCs checked) and the cracked 360K `.img`. The copy protection, 1 KB-sector tracks holding the engine, is worked out, and the two give the same engine and data (tested). |
| 2. Disk layout | **Done** ([disk-layout.md](disk-layout.md)): boot chain, the engine's position, the resource table, its entry format, the read routine. |
| 3. Static formats | **Done.** Pictures: all 79 draw cleanly (Apple decoder, IBM colours); the PCjr shows them in full, 160 × 168. Scripts: the PCjr operand table, read from the handlers; all 84 well-formed scripts parse. Rules: the Apple format, with the PCjr's word and variable numbers. Views: all 88 decode. Sounds: all 40 parse and render. Vocabulary: all 358 words decode. Colours: the default IBM palette. Left for the engine phase: a PCjr painter for the full picture, and room 64. |
| 4. Measuring tool | **Working** ([refemu/](../refemu/README.md)). An 8086 core with 8088 timing, on a PCjr with no IBM ROM. It boots the original from the `.mfm` (the 1 KB-sector engine included) and runs plans (routes) of keys, typed commands and prompt answers. It answers disk swaps, and makes, saves to and restores from a Play diskette. It records a trace per pass and the screen, and can draw every room picture with the engine's own code. Timing: the 8088's instruction fetches are added to the core's counts, calibrated against 86Box (walking within 10%, a room change within 10%; [tools/86box/](../tools/86box/README.md)). 86Box is the timing reference: no working PCjr is available, and 86Box adds no PCjr wait states for this game. Still to do: a font for text in screenshots. |
| Reference routes | `routes/walk_rooms_1_16.plan` (rooms 1 and 16), `routes/save_restore.plan` (Play diskette, save, restore) and `routes/restore_play.plan` (restoring that diskette on a fresh start), recorded from the original as `reference/*.csv`. Compare the state columns; the cycles change with calibration. |
| Picture painter | **Done** ([picture-format.md](picture-format.md)). The PCjr's own line, plot and fill routines, modelled literally; all 80 room pictures byte-identical to the engine's. |
| 5. The engine | **Running** ([src/engine/](../src/engine/mod.rs), [engine.md](engine.md)). A native reimplementation, routine by routine, in the Apple port's style: decoded resources, named state, native screen planes. The timer interrupt (messages, music, keys) is replayed from refemu-pcjr's event log at the same points in the code. **Both reference routes match the original on every pass**: the walk (900 passes) and the save/restore (1,000 passes: making a Play diskette, saving, restoring), with every checkpoint in the same order (tests in `tests/engine.rs`). Saves are written in the original's format. The speed model and the player are below. |
| 6. Pass-by-pass comparison | **Done.** Nine routes, played on the original by refemu's autopilot (`kqroute-pcjr`), match on every pass: the walk, save/restore, three deaths, the gold key, the maximum score, the magic ring and the giant with the sling (about 89,000 passes; `tests/engine.rs`). |
| 8. The screen | **Done.** `kqframes-pcjr` compares the screen at every pass in the hardware's terms (the displayed page's picture bytes and the character cells): every screen of the nine routes is the same. The original's frame hashes are in `reference/*.frames`, checked by the tests. |
| 9. Timing | **Done** ([engine.md](engine.md), "Speed"). A fitted cost model of each pass's work, charged at every checkpoint; whole routes within 0.2% of the original's time, passes within 1.6% (median). |
| 10. The player | **Working** (`src/bin/kq-pcjr.rs`). A window at the PCjr's 4:3 shape (resizable, Alt+Enter full screen, painted during drags on Windows), the keyboard, the SN76496's sound (winmm on Windows, cpal on Linux), the speed model pacing the clock in real time (F11/F12), the Play diskette kept as an image, the disk swaps answered. The port's own 8 × 8 font (the ROM's isn't shipped; `--font` takes a dump). A joystick or game controller as the PCjr's joystick (the original's read, `$0563`). Fast loads (F9): the disk and the picture drawing take no time (`Clock::loading`). Not yet in release-please. |

## Findings so far

- **The data is the Apple version's data, in PCjr form.** The two versions share:
  - the picture commands;
  - the script layout and most of its opcodes;
  - the loop headers and cel encoding of views;
  - the room 64 header defect.

  The Apple version was converted from this one. The Apple port's docs are therefore
  a map of this engine, and its decoders carry over with small changes. The
  differences found so far: view tables hold sizes, not offsets; colour numbers differ;
  and some opcodes differ.
- **No separate picture directory.** Each room's picture follows its script in one
  resource. The Apple version has a directory per resource type.
- **The video buffers are PCjr video pages.** Pages 4 (`1000:0`) and 6 (`1800:0`),
  in the 4-bank interleaved layout. Mode 9 is shown and the pages are switched with
  `INT 10h` AX=0583h.
- **Saves go to a "Play diskette"** that the game formats and fills itself.
- **The copy protection** is only the 1 KB-sector tracks the engine loads from. The
  engine has no other check that I've found, and the crack changes no engine code.
- **Room 64 shows no picture on the PCjr** (confirmed): the engine starts the picture a
  byte early because of the script header defect, and the interpreter stops at once.
- **Saves need a Play diskette.** The Master is write-protected, and saving to it fails
  ("Games may only be saved on your Play diskette"). "copy disk" makes a Play diskette:
  the game formats a blank disk and copies its data across in four swaps. Saves go to
  sectors `0x282 + 3 × slot`, signed `vrsn#` and the slot letter. A Play diskette holds
  data and saves but no engine, so play still starts from the Master.
- **More animation frames than the Apple version:** 722 cels to its 674.
- **The game cycle is shaped differently.** One pass is one call of `$14F2`: the actors,
  then the cycle rules (global, repeating while asked, then the room's). The input
  wait calls it over and over while the player types. The parser rules run in the
  main loop (`$D1C`) once a line is complete, not inside a pass as on the Apple. The
  timer interrupt (`$808`) reads the keyboard and plays the music.
- **The actor record is 30 bytes**, the Apple's 31: the direction is at `$0F` (the
  Apple's `$10`), and x and y at `$18`/`$19` as on the Apple. The score is v19, as on
  the Apple.
- **First measurements** (provisional, timing uncalibrated): about 190,000 CPU cycles
  a pass in room 1, 21 passes a second. The main loop is reached 3.6 s after the
  engine starts.

## Next

1. **More routes:** the Apple port's seven (the gold key, the magic ring, the full
   score, the deaths), played on the PCjr. An autopilot playing a route in the port,
   as the Apple port's `kqroute` does, would write the plans.
2. **The variable renumbering** between the versions, so the Apple port's knowledge of
   each variable carries over.
3. **The 86F format**, so the `.86f` dump can be read too.

## Repository notes

- `kq1-pcjr` depends on `kq1-apple2` for its decoders for now. Once the PCjr engine
  needs them, the shared readers move to a crate under `crates/`, keeping the Apple
  port's tests green.
- The crate is not in release-please yet: it has no player to release. Add it, and
  generalise the Release workflow's build job, when it has one.
