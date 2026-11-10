# King's Quest: IBM PC port (1984, CGA and CGA+RGB)

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PC with the
Color/Graphics Adapter, from your own disk image, with no 8088 emulation. It plays both
releases: the first, in composite colour, and the second (August 1984), which added RGB
colour, a new copy protection and three script fixes. It follows [docs/porting-guide.md](../../docs/porting-guide.md), with the
IBM PCjr port in [ports/pcjr/](../pcjr/README.md), this release's sibling, as the
model.

**Status:** playable, both editions. The native engine matches each original on every
pass of its reference runs (three whole games among them for each edition, and two RGB
runs for the second), in state, on screen and in the speaker's notes, and runs at the
original's speed (within 1%); its Play diskettes are the originals'. The player,
`kq-pc`, shows the screen as a composite monitor did, in two modes, or in the second
edition's RGB colours as an RGB monitor did, and plays the speaker. See
[docs/port-plan.md](docs/port-plan.md).

## Playing

```
cargo run --release -p kq1-pc --bin kq-pc
```

[PLAYING.md](PLAYING.md) has the keys, the options and the details (it is the guide
shipped with the release, where the player is `kq`).

## What you need

Your own image of the game disk, in `original/` (git ignores it):
- the original as an HxC MFM dump (`.mfm`, the "[cp] [!]" dump), which is read
  directly, copy protection and all; or
- the cracked 360K sector image (`.img`, 368,640 bytes).

Both give the same game. Either edition plays: the first release (its boot sector says
"BOOT v1.1") and the second, the CGA+RGB edition ("BOOT v1.2", August 1984: composite or
RGB colour). Their runs from the originals are in `reference/` and
`reference/cga_rgb/`.

## Tools

The player's `--headless "SECONDS:KEYS;..." --until S --shot FILE.png` plays without a
window and writes the screen (see the top of
[src/bin/kq-pc.rs](src/bin/kq-pc.rs)).

```
cargo run --release -p kq1-pc --bin kqextract-pc -- original/<disk>.mfm
```

`kqextract-pc` writes every resource to `extracted/` (git ignores it: it is game
content):
- the disk's files, and the engine's code segment as it runs;
- rooms as `res/room/NNN.script.bin` and `NNN.pic.bin`;
- views and sounds;
- each room's picture in composite colour: `pics/NNN.png` with each 4-dot group in
  its colour, `pics/NNN.signal.png` decoded from the signal, with its fringes.

It checks every script, picture, view and sound, and the vocabulary, and reports each
one.

The measuring tool is in [refemu/](refemu/README.md): the reference PC with the CGA.
[tools/86box/](tools/86box/README.md) measures the original in 86Box, with IBM's BIOS:
its colours and its timing.

## Tests

The reference the port is compared with is in `reference/`: ten runs of the first
edition's original in refemu-pc, from `routes/`, and in `reference/cga_rgb/` the same
ten from the second edition's in composite, and two in RGB (`rgb_*`). Numbers only: the
event logs, the traces, the screens' hashes, the speaker's events.

`cargo test -p kq1-pc`. The tests that need the disk image skip themselves without it.
They check that every resource decodes (the colour table included), that each edition's
original and crack hold the same game, that the editions differ only in their three
script fixes, each original's protection sector, and that every route matches its
original on every pass, in state, on screen and in the speaker's notes.

The screens were recorded with the IBM PC BIOS's font, the characters drawn as the BIOS
draws them. The font is in IBM's ROM, which isn't shipped: put your own BIOS image in
`original/` as `bios.rom` (the IBM PC's 8 KB ROM, as in 86Box's ROM set,
`roms/machines/ibmpc82/BIOS_5150_27OCT82_1501476_U33.BIN`; git ignores it). Without it the
screens are left out of the tests, and refemu-pc leaves the characters' cells blank.

## Documentation

- [docs/port-plan.md](docs/port-plan.md): status, findings, next steps, decisions
- [docs/disk-layout.md](docs/disk-layout.md): the disk, its files and protection, boot,
  the resource table, the resources, the display
- [docs/engine.md](docs/engine.md): the engine against the PCjr's, the address map, the
  checkpoints, how the PC differs, the speed model
