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
| 1. Disk image | **Done.** A plain 360K sector image; `Disk::new` checks its size and every directory entry. |
| 2. Disk layout | **Done** ([disk-layout.md](disk-layout.md)): boot chain, the engine's position, the resource table, its entry format, the read routine. |
| 3. Static formats | **Started.** Pictures: all 79 that follow a well-formed script draw cleanly with the Apple port's decoder. Scripts: 44 of 81 parse with the Apple opcode table. Views: layout worked out on view 0; reader to write. Sounds: identified, format to work out. |
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

## Next

1. **Scripts.** Read the PCjr action interpreter's opcode table, and list the
   opcodes and operand counts that differ from the Apple's (start from the failing
   rooms: `kqextract` reports the first bad opcode in each).
2. **Views.** Write the PCjr view reader (sizes, 8 loop slots), and check all 111.
3. **Colours.** Find the engine's palette handling and the picture colour numbering,
   then render pictures and views in the PCjr's own 16 colours.
4. **Sounds.** Work out the format from the `INT 1Ch` player (`$5608` onwards in the
   engine).
5. **The measuring tool**, before the engine.

## Repository notes

- `kq1-pcjr` depends on `kq1-apple2` for its decoders for now. Once the PCjr engine
  needs them, the shared readers move to a crate under `crates/`, keeping the Apple
  port's tests green.
- The crate is not in release-please yet: it has no player to release. Add it, and
  generalise the Release workflow's build job, when it has one.
