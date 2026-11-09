# King's Quest: IBM PCjr port (in progress)

A native Rust port of Sierra's 1984 *King's Quest* for the IBM PCjr, the release IBM
published, from your own disk image, with no 8088 emulation. It follows
[docs/porting-guide.md](../../docs/porting-guide.md), with the Apple IIe port in
[ports/apple2/](../apple2/README.md) as the worked example.

**Status:** the engine plays the whole game. On nine routes recorded from the original
(about 89,000 passes: the walk, save and restore, three deaths, the gold key, the
maximum score, the magic ring and the giant with the sling), it matches the original
on every pass, in its state and on screen. A speed model gives the original's timing,
within 0.2% over whole routes. The player is `kq` in its download (`kq-pcjr` in the workspace).
See [docs/port-plan.md](docs/port-plan.md).

## Playing

```
cargo run --release -p kq1-pcjr --bin kq-pcjr
```

`kq-pcjr` finds the game's disk next to itself, in the current folder or in
`original/` (or give its path). It plays in a window at the PCjr's speed, with its
sound, and the room pictures are drawn on screen as the original draws them. The keys are the PCjr's: the arrows and Home, PgUp, End and PgDn walk (the same
key again stops), typing enters commands, Enter submits, Tab shows the inventory, Esc
pauses, 0 jumps. A joystick or game controller is the PCjr's joystick: the stick or
d-pad walks, button 1 is the `-` key held down and button 2 jumps (`--no-joystick`
leaves it out). F11 and F12 make the game slower and faster, F9 turns fast loads on and
off (`--fast-loads`: a room change's disk reads and picture drawing take no time, the
rest keeps the PCjr's pace), F8 turns the sound off and on, and Alt+Enter is full
screen. Saves go on the Play diskette, which the game
makes itself ("copy disk"); it is kept as `KQ1 Play.img` next to the game's disk. It
is the original's Play diskette, saves and all: the original restores the port's saves
and the other way round.
When the game asks for the Play or the Master diskette, it is put in for you: press
Enter as asked.

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

It checks every script, picture and view with the Apple port's decoders, and reports
each one.

## Tests

`cargo test -p kq1-pcjr`. The tests that need the disk image skip themselves without it.

## Documentation

- [docs/port-plan.md](docs/port-plan.md): status, findings, next steps
- [docs/picture-format.md](docs/picture-format.md): the picture commands and the PCjr drawing routines
- [docs/disk-layout.md](docs/disk-layout.md): the image, boot, the resource table, the
  resource formats found so far
