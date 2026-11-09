# Port plan: native King's Quest (IBM PC, 1984, CGA)

## Goal and constraints

The goal is a native Windows and Linux program, written in Rust, that plays the 1984 IBM
PC King's Quest, Sierra's CGA release, as the original does: on every pass, on screen
and at its speed. The ground rules are the repository's
([docs/porting-guide.md](../../../docs/porting-guide.md)):
- no 8088 emulation in the port;
- the player's own disk image;
- no IBM ROM;
- no game content in git;
- the original's quirks reproduced.

The PC release is the PCjr release's sibling (both 1984, same engine design, mostly the
same data), so this port follows the PCjr port ([ports/pcjr/](../../pcjr/README.md))
phase by phase, and reuses what carries over. As the Tandy 1000 plan has it: fork
what's needed from the PCjr port, then move what the ports share into `crates/` once
both have it working.

## Where we are

| Phase | Status |
|---|---|
| 1. Disk image | **Done.** The original as dumped (HxC `.mfm`, through the shared decoder, which now lists unreadable sectors) and the cracked 360K `.img`. The copy protection, one unreadable sector on track 6, is required on the original; the two images hold the same game (tested). The CGA+RGB edition is recognised and refused. |
| 2. Disk layout | **Done** ([disk-layout.md](disk-layout.md)): a DOS disk with three files; the engine `MAIN.EXE`, run at 0060:0000, its data segment at CS + `$511`; the resource table on sectors 10-11, read over the code at CS:`$88`, in the PCjr's entry format; the read routine. |
| 3. Static formats | **Done** (`kqextract-pc`). With the PCjr port's decoders: all 80 well-formed scripts parse, all 79 pictures draw cleanly, all 88 views decode (byte for byte the PCjr's), all 40 sounds parse, and the 369-word vocabulary. The scripts are a revision of the PCjr's (renumbered variables, some rooms changed); eight pictures and three sound resources differ. |
| 4. Measuring tool | **Next.** |
| 5-10 | To do. |

## Next steps

1. **The measuring tool**, `refemu-pc` (crate `kq1-pc-refemu`, in `refemu/`), forked
   from the PCjr's: the same 8086 core, on an IBM PC with the CGA instead of the
   PCjr's hardware, and no IBM ROM. What differs:
   - the machine: 8253 timer and 8259 interrupt controller as on the PCjr; the speaker
     through the timer's channel 2 and port `61h` (the engine writes both); the CGA's
     registers and its memory at `B800h`; the keyboard through the PC's BIOS calls;
   - the boot: the original runs `LOAD` and then `MAIN.EXE`'s protection code, which
     reads the unreadable sector. Either emulate the controller's answer for that
     sector from the `.mfm` (as the PCjr's refemu reads its 1 KB sectors), or start
     the engine as the crack does, at CS:IP 0, and record the protection's code as
     not measured;
   - timing: the 4.77 MHz 8088 with the PC's DMA refresh, and the CGA's wait states,
     which cost more than the PCjr's video memory; calibrate against 86Box's IBM PC
     with CGA, as the PCjr's was against its PCjr.

   Then find the main loop's entry, record the reference routes (the PCjr's plans and
   routes, edited for this revision's scripts), and write trace CSVs.
2. **The engine**, forked from the PCjr's (`src/engine`). The routines are the same
   design at other addresses; map each of the PCjr port's routines to the PC
   engine's (an address table in engine.md), then compare pass by pass. Expect the
   variable renumbering in the scripts to come from the engine's own variables.
3. **The screen.** The PC's picture buffer and its copy to the CGA (`$1966`), the
   text in 640-dot mode, and the composite colours (below).
4. **Sound.** Find how the engine plays the sound-chip streams on the speaker (the
   timer's channel 2 at `$158B`-`$15C8`, port `61h`), and play them as it does.
5. **Timing**, as on the PCjr: a cost model fitted to refemu-pc.
6. **The player**: the PCjr's, with the composite display and the speaker.

## Decisions to make

- **Composite colour.** The original shows 16 colours only on a composite monitor
  (or a TV), as NTSC artefacts of the dot patterns. The port has to turn the dot
  patterns into colours. Options: a table of 16 colours, one per 4-dot group (simple,
  no fringes); or a model of the composite signal's decoding, which also gives the
  fringes between groups as a monitor shows them. Both depend on the CGA's revision
  (the 1981 and 1983 cards differ in their composite output) and on the foreground
  colour the engine sets (7, not 15). Record the choice here.
- **The protection's code in refemu.** As above: emulate the unreadable sector, or
  start as the crack does.
- **What to share with the PCjr port.** The painter, views, sounds' format, the
  engine's structure: move into `crates/` once both ports have them working, not
  before.

## Findings so far

- **A DOS disk, booted without DOS.** A FAT filesystem with three files, of which
  `290K` covers the data area so that DOS sees it used. The resources are read by
  sector number through a table in the PCjr's format.
- **The engine is a different build from the PCjr's**, as an EXE: about 4.5 KB of its
  29 KB are byte for byte the PCjr's (data and messages); the code is at other
  addresses.
- **The data is the PCjr's, mostly.** All views, seven of ten sound resources and all
  but eight pictures are byte for byte the same; the scripts are a revision
  ([disk-layout.md](disk-layout.md)).
- **The display:** CGA mode 4, switched to 640 dots with the colour burst on: 160 × 200
  in 16 composite colours. The picture is 160 × 168, as on the PCjr.
