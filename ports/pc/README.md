# King's Quest: IBM PC port (1984, CGA)

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PC with the
Color/Graphics Adapter, in composite colour, from your own disk image, with no 8088
emulation. It follows [docs/porting-guide.md](../../docs/porting-guide.md), with the
IBM PCjr port in [ports/pcjr/](../pcjr/README.md), this release's sibling, as the
model.

**Status:** started. The disk image (the original's copy protection included), its
files, the resource table and every resource are read and checked. See
[docs/port-plan.md](docs/port-plan.md) for what comes next.

## What you need

Your own image of the game disk, in `original/` (git ignores it):
- the original as an HxC MFM dump (`.mfm`, the "[cp] [!]" dump), which is read
  directly, copy protection and all; or
- the cracked 360K sector image (`.img`, 368,640 bytes).

Both give the same game. The disk's boot sector says "BOOT v1.1". The CGA+RGB edition
("BOOT v1.2") is a different disk, and not this port's.

## Tools

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

The reference the port will be compared with is in `reference/`: ten runs of the
original in refemu-pc, from `routes/` (numbers only: the event logs, the traces, the
screens' hashes).

`cargo test -p kq1-pc`. The tests that need the disk image skip themselves without it.
They check that every resource decodes (the colour table included), that the original and the crack hold the same
game, and the original's protection sector.

## Documentation

- [docs/port-plan.md](docs/port-plan.md): status, findings, next steps, decisions
- [docs/disk-layout.md](docs/disk-layout.md): the disk, its files and protection, boot,
  the resource table, the resources, the display
- [docs/engine.md](docs/engine.md): the engine against the PCjr's, the address map, the
  checkpoints
