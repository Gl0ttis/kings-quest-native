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
| Composite colour | **Done** ([src/composite.rs](../src/composite.rs)). The engine draws only colours 0 and 7, so the signal is two levels a dot, and the colours are the monitor's decoding of the dot patterns. Two modes from one model: each 4-dot group in its own colour, and the signal decoded dot by dot with the fringes between colours. Its hue, saturation and level are measured from the original in 86Box (IBM's BIOS, the CGA's composite output; [tools/86box/](../tools/86box/README.md)): within 6/255 in flat areas. The engine's colour table (DS:`$12BA`) gives each PCjr colour's pattern. |
| 4. Measuring tool | **Working** ([refemu/](../refemu/README.md)). The PCjr's 8086 core on an IBM PC with the CGA, no IBM ROM. It starts the engine as the loader does (the protection's code calls into IBM's ROM, and then starts the engine at the same place), runs plans of keys and records a trace per pass and the screen in composite colour. The pass is `$293A`, the PCjr's `$14F2`. Timing, fitted to 86Box running the original: walking within 1%, a room's drawing within 5%, a room change within 8%. The game paces itself on the timer: a pass is a whole number of ticks. |
| Address map | **Done** ([engine.md](engine.md), `tools/addrmap.py`): 174 of the 207 addresses the PCjr port cites found in the PC engine, and all 22 checkpoints. For citing and measuring only; the engine is rewritten from its behaviour. |
| Reference routes | **Ten**, as on the PCjr: three plans (`routes/*.plan`, `kqplan-pc`: the walk through rooms 1 and 16, making a Play diskette and saving and restoring, restoring that diskette on a fresh start) and seven routes (`routes/*.route`, `kqroute-pc`: three deaths, the gold key, the maximum score, the magic ring, the giant with the sling), the PCjr's with this revision's variable numbers. In room 52 the walk through the hole to room 51 is given by hand: the autopilot finds no landing there (the PCjr's doesn't any more either). |
| 5-10 | To do: next, the engine. |

## Next steps

1. **The engine**, forked from the PCjr port's (`src/engine`), which reimplements the
   engine natively from its documented behaviour. The same rule holds here: no
   instruction-by-instruction translation. The PC's routines are the same design at
   other addresses ([engine.md](engine.md): the address map and the checkpoints);
   each difference from the PCjr is found by comparing pass by pass, described, and
   then implemented. Expect the variable renumbering in the scripts to come from the
   engine's own variables.
2. **The screen.** The PC's picture buffer and its copy to the CGA (`$1966`), and the
   text in 640-dot mode, shown through the composite model.
3. **Sound.** Find how the engine plays the sound-chip streams on the speaker (the
   timer's channel 2 at `$158B`-`$15C8`, port `61h`), and play them as it does.
4. **Timing**, as on the PCjr: a cost model fitted to refemu-pc.
5. **The player**: the PCjr's, with the speaker, and the composite display in either
   mode (the group colours by default, the signal with its fringes as an option).

## Decisions to make

- **What to share with the PCjr port.** The painter, views, sounds' format, the
  engine's structure: move into `crates/` once both ports have them working, not
  before.

## Decisions

- **Composite colour: both modes** (the user's choice), from one model of the signal.
  Each 4-dot group's own colour is the default. The signal decoded with its fringes is
  the option. The CGA's revision isn't a setting: with only colours 0 and 7 on
  screen, the old and new cards differ only in the signal's overall level, which the
  monitor's knobs set. The model's numbers are 86Box's old CGA.

- **refemu-pc starts the engine where the protection hands over** (refemu's README,
  "Start-up"): the protection's code calls IBM's ROM, which refemu doesn't have, and
  goes to the engine's start whether it finds the disk or not. The boot, the loader and
  the protection aren't measured.

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
