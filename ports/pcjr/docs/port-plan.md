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
| 4. Measuring tool | **Working** ([refemu/](../refemu/README.md)). An 8086 core with 8088 timing, on a PCjr with no IBM ROM. It boots the original from the `.mfm` (the 1 KB-sector engine included), plays the title, takes keys from a plan or a key log, and records a trace per pass and the screen. Still to do: calibrate its timing (PCjr video memory contention, the BIOS's own time), saves and the Play diskette, and a font for text in screenshots. |
| 5–10. Engine onwards | Not started. |

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
- **Room 64** may show no picture on the PCjr: the engine starts the picture a byte
  early because of the script header defect (disk-layout.md). To confirm.
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

1. **Finish the measuring tool:** calibrate its timing, saves to the Play diskette,
   and a reference route recorded from the original (a walk through the first rooms,
   then longer routes).
2. **A PCjr painter** for the full 160 × 168 picture, from the PCjr's 15 picture
   handlers (`$3534`), checked against the measuring tool's frames.
3. **The variable renumbering** between the versions, so the Apple port's knowledge of
   each variable carries over.
4. **The 86F format**, so the `.86f` dump can be read too.

## Repository notes

- `kq1-pcjr` depends on `kq1-apple2` for its decoders for now. Once the PCjr engine
  needs them, the shared readers move to a crate under `crates/`, keeping the Apple
  port's tests green.
- The crate is not in release-please yet: it has no player to release. Add it, and
  generalise the Release workflow's build job, when it has one.
