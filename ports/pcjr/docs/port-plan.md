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
| 3. Static formats | **Mostly done.** Pictures: all 79 draw cleanly (Apple decoder, IBM colours). Scripts: the PCjr operand table, read from the handlers; all 84 well-formed scripts parse. Views: the PCjr reader; all 88 decode. Colours: the default IBM palette. Left: sounds, the picture size the PCjr shows, and room 64. |
| 4. Measuring tool | Not started. An 8088 PCjr emulator, with the BIOS calls the game makes substituted. |
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

## Next

1. **Sounds.** Work out the format from the `INT 1Ch` handler the engine installs
   (`$808`; it writes the sound chip at `$85B`) and the code that starts a sound.
2. **The picture on screen.** Read the PCjr plotting code for the picture size it
   keeps and how a picture pixel maps to mode 9's 320×200, and compare fills with the
   Apple painter (the Apple's colour table merges two colours; the PCjr's doesn't).
3. **The rule matchers.** Check that the cycle and parser rule formats match the
   Apple's (the dispatch tables at `$2958` and `$2B3B` look like them).
4. **The measuring tool**, before the engine: an 8088 PCjr that boots the original
   from the `.mfm`, with the BIOS calls the game makes substituted, recording traces
   and frames as refemu does.
5. **The 86F format**, so the `.86f` dump can be read too.

## Repository notes

- `kq1-pcjr` depends on `kq1-apple2` for its decoders for now. Once the PCjr engine
  needs them, the shared readers move to a crate under `crates/`, keeping the Apple
  port's tests green.
- The crate is not in release-please yet: it has no player to release. Add it, and
  generalise the Release workflow's build job, when it has one.
