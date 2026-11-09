# King's Quest: IBM PCjr port (in progress)

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PCjr, the release IBM
published, from your own disk image, with no 8088 emulation. It follows
[docs/porting-guide.md](../../docs/porting-guide.md), with the Apple IIe port in
[ports/apple2/](../apple2/README.md) as the worked example.

**Status:** the disk, its copy protection and its layout are worked out. Every
resource extracts and decodes: pictures, scripts, views, sounds and the vocabulary.
[refemu/](refemu/README.md), the reference PCjr, boots the original and records it
pass by pass. No engine or player yet.
See [docs/port-plan.md](docs/port-plan.md).

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

It checks every script, picture and view with the Apple port's decoders, and reports
each one.

## Tests

`cargo test -p kq1-pcjr`. The tests that need the disk image skip themselves without it.

## Documentation

- [docs/port-plan.md](docs/port-plan.md): status, findings, next steps
- [docs/picture-format.md](docs/picture-format.md): the picture commands and the PCjr drawing routines
- [docs/disk-layout.md](docs/disk-layout.md): the image, boot, the resource table, the
  resource formats found so far
