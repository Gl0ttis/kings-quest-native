# King's Quest: IBM PCjr port

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PCjr, the release IBM
published, from your own disk image, with no 8088 emulation. It follows
[docs/porting-guide.md](../../docs/porting-guide.md), with the Apple IIe port in
[ports/apple2/](../apple2/README.md) as the worked example.

**Status:** playable start to finish. On ten routes recorded from the original (about
90,000 passes: the walk, saving and restoring, restoring on a fresh start, three
deaths, the gold key, the maximum score, the magic ring and the giant with the sling),
the engine matches the original on every pass in its state, and on screen on all of
them but the fresh-start restore, whose screen isn't recorded. A speed model gives the
original's timing, within 0.2% over whole routes, and room pictures are drawn at the
original's measured pace. Its Play diskette is the original's: each side restores the
other's saves. The player is `kq` in its download (`kq-pcjr` in the workspace). See
[docs/port-plan.md](docs/port-plan.md).

## Playing

```
cargo run --release -p kq1-pcjr --bin kq-pcjr
```

It finds the game's disk next to itself, in the current folder or in `original/` (or
give its path). It plays in a window at the PCjr's speed, with its sound; the room
pictures are drawn on screen as the original draws them.

- **Keys:** the PCjr's. The arrows and Home, PgUp, End and PgDn walk (the same key
  again stops), typing enters commands, Enter submits, Tab shows the inventory, Esc
  pauses, 0 jumps; 3, 5, 7 and 9 save, restore, restart and bring back the last line.
- **A joystick or game controller** is the PCjr's joystick: the stick or d-pad walks,
  button 1 is the `-` key held down and button 2 jumps (`--no-joystick` leaves it out).
- **The player's own keys:** F11 and F12 slower and faster, F9 fast loads
  (`--fast-loads`: a room change's disk reads and picture drawing take no time, the
  rest keeps the PCjr's pace), F8 sound, Alt+Enter full screen.
- **Saves** go on the Play diskette, which the game makes itself ("copy disk"), kept as
  `KQ1 Play.img` next to the game's disk. It is the original's Play diskette, saves
  and all. When the game asks for the Play or the Master diskette, it is put in for
  you: press Enter as asked.
- **For developers:** `--route NAME` plays a recorded route (`reference/NAME.events`)
  at any `--speed`, then hands over; `--rate N` and `--room N` as in the Apple player;
  `--headless "SECONDS:KEYS;..." --until S --shot FILE.png` runs without a window.

[PLAYING.md](PLAYING.md), which ships in the download, is the players' guide.

The PCjr's font is in its ROM, which isn't shipped. If you have the PCjr BIOS image
(64 KB, as in an emulator's ROM set, for example 86Box's `roms/machines/ibmpcjr/bios.rom`),
put it next to the game's disk as `bios.rom` and the text is in the original font; or
give it with `--font FILE` (a 1,024-byte dump of the font alone works too). Otherwise
the text is in the port's own font, of its own design in the same bold style.

## What you need

Your own image of the game disk, in `original/` (git ignores it):
- the original as an HxC MFM dump (`.mfm`, the "[cp] [!]" dump), which is read
  directly, copy protection and all; or
- the cracked 360K sector image (`.img`, 368,640 bytes).

Both give the same game.

## Tools

```
cargo run --release -p kq1-pcjr --bin kqextract-pcjr -- original/<disk>.img
```

`kqextract-pcjr` writes every resource to `extracted/` (git ignores it: it is game
content):
- rooms as `res/room/NNN.script.bin` and `NNN.pic.bin`;
- views and sounds;
- the engine's code segment;
- each room's picture as `pics/NNN.png`.

It checks every script, picture and view with the shared decoders (`kq1-formats`),
and reports each one.

The measuring tools are in [refemu/](refemu/README.md): the reference PCjr, its route
autopilot (`kqroute-pcjr`), and the comparisons of state, screen and drawing. The
speed model's fits are `tools/costfit.py` (passes) and `tools/drawfit.py` (the
pictures' drawing), on the counts from `examples/costdump.rs` and
`examples/drawcmp.rs`; [tools/86box/](tools/86box/README.md) measures the original in
86Box, with IBM's BIOS.

## Tests

`cargo test -p kq1-pcjr`. The tests that need the disk image skip themselves without it.
They replay every route against the original's trace and screens
(`reference/*.events`, `*.csv`, `*.frames`: numbers only, no game data), check the Play
diskette's save block against the original's, every picture against the engine's own
drawing, the joystick, the credits, and the player's scaling.

## Documentation

- [PLAYING.md](PLAYING.md): the players' guide
- [docs/port-plan.md](docs/port-plan.md): status, findings, next steps
- [docs/engine.md](docs/engine.md): the original engine and the port's reimplementation,
  the timer's replay, the speed model
- [docs/picture-format.md](docs/picture-format.md): the picture commands and the PCjr drawing routines
- [docs/disk-layout.md](docs/disk-layout.md): the image, boot, the resource table, the
  resource formats
- [refemu/README.md](refemu/README.md): the reference PCjr and its tools
- [tools/86box/README.md](tools/86box/README.md): measuring in 86Box
