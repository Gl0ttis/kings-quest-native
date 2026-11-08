# King's Quest: IBM PCjr port (in progress)

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PCjr, the release IBM
published, from your own disk image, with no 8088 emulation. It follows
[docs/porting-guide.md](../../docs/porting-guide.md), with the Apple IIe port in
[ports/apple2/](../apple2/README.md) as the worked example.

**Status:** the disk layout is worked out and every resource extracts. Pictures, and
most scripts, already decode with the Apple port's readers. No engine or player yet.
See [docs/port-plan.md](docs/port-plan.md).

## What you need

Your own image of the game disk as a 360K sector image (`.img`, 368,640 bytes), in
`original/` (git ignores it).

## Tools

```
cargo run --release -p kq1-pcjr --bin kqextract -- original/<disk>.img
```

`kqextract` writes every resource to `extracted/` (git ignores it: it is game
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
- [docs/disk-layout.md](docs/disk-layout.md): the image, boot, the resource table, the
  resource formats found so far
