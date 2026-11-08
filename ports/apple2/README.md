# King's Quest: Apple IIe port

A native Rust port of Sierra's 1984 Apple IIe *King's Quest* (Apple version by Jeff
Stephenson; double hi-res, 128K). It plays the original game from your own disk
images, with no 6502 emulation: the engine is rewritten from the original's
behaviour, documented in [docs/](docs/), and the game's own 81 scripts run on it
unchanged.

It matches the original pass by pass on everything traced so far: a 158-point
playthrough from power-on to the coronation, a death and restart, and saving and
restoring both ways between the port and the original. Timing follows the
original too: each pass of the main loop lasts as long as it did on a 1 MHz Apple
IIe ([src/engine/cost.rs](src/engine/cost.rs)).

## What you need

Your own images of the three game disks, as `.nib` (232,960 bytes) or DOS-order
`.do`/`.dsk` (143,360 bytes). Optionally:

- a blank save disk image (disk 4, volume 10) for `--savedisk`;
- an Apple IIe video ROM dump (342-0265 or 342-0133 in its name) for the Apple's own
  text-mode characters; it is picked up automatically from next to the disks.

Nothing from the game or the Apple is included in the repository.

## Playing

Players download `kq` from the releases page; [PLAYING.md](PLAYING.md) is their guide
(and the README in the release archive). From source:

```
cargo run --release --bin kq -- [FOLDER | disk1 disk2 disk3]
```

With no disks given, `kq` looks for them next to the executable, then in the current
folder ([`Disks::find`](src/kq.rs)): `.nib` images by the disk number each records,
`.do`/`.dsk` by the disk number in the file name. Given as three files, `.nib` images
may come in any order and `.do` images must be in order 1, 2, 3. For development,
keep them in `original/` (git ignores it) and run `kq original`.

**Keys, as on the Apple:** arrows, and `'` `/` `.` `;` for the diagonals, move Graham
(press again to stop). Type commands and press RETURN. Tab shows the inventory, Esc
pauses, Ctrl-E recalls the last line. `1` saves, `3` restores, `5` restarts (after a
death too). A joystick or game controller connected at start-up works as the Apple
joystick; its buttons 1 and 2 are the Apple's two buttons.

**Front-end keys:** F11/F12 slower/faster, F9 fast loads (skips the disk and
picture-drawing time of room changes), F8 sound, F7 NTSC colour fringing, F6 fill the
white specks the picture fills miss, Alt+Enter full screen.

**Linux:** the window uses X11 (XWayland on Wayland desktops), which also provides full
screen; the bell goes through ALSA (and so PulseAudio or PipeWire); the joystick is
read from `/dev/input/js0`. Building needs the X11 and ALSA development packages
(`libx11-dev libxkbcommon-dev libxcursor-dev libxrandr-dev libasound2-dev` on
Debian and Ubuntu).

**Options:** `--speed N`, `--fast-loads`, `--fullscreen`, `--mute`, `--ntsc`,
`--fill-specks`, `--savedisk FILE` (save to an original save disk image instead of
save files), `--saves DIR` (where save files go; by default `saves/` next to the disks), `--swap-prompts` (the original's "Please insert
side B" prompts, off by default), `--charrom FILE` / `--no-charrom`,
`--no-joystick`, `--keys FILE` (replay a key log, such as
[routes/max_score.keys](routes/max_score.keys)), `--rate N` (a fixed rate),
`--scale N` (window size). For testing: `--headless PLAN`, `--room N`,
`--trace FILE`, `--picbuf FILE`.

## Tools

Most take the three disk images; `nibdecode` takes any number of `.nib` files, and
`kqroute` also takes a route file.

| Binary | Does |
|---|---|
| `nibdecode` | Decodes `.nib` images to `.do`/`.po` in `extracted/`, checking every sector |
| `kqextract` | Extracts every resource with the engine's own loader rules ([docs/disk-layout.md](docs/disk-layout.md)) |
| `kqpic` | Renders all 83 pictures ([docs/picture-format.md](docs/picture-format.md)) |
| `kqview` | Renders all 88 views ([docs/view-format.md](docs/view-format.md)) |
| `kqscript` | Disassembles the scripts and vocabulary ([docs/script-format.md](docs/script-format.md)) |
| `kqroom` | Enters each room through the engine and renders it |
| `kqroute` | Plays a scripted route with an autopilot and records the keys ([docs/route.md](docs/route.md)) |

Their output goes to `extracted/`, which git ignores: it is game content.
[refemu/](refemu/README.md) is the reference Apple IIe emulator the port is measured
against; it is a measuring tool, not part of the port. [tools/](tools/) holds the
Python analysis scripts.

## Tests

```
cargo test -p kq1-apple2
```

With the disks decoded as `extracted/disk1.do` to `disk3.do` (`nibdecode` names its
output after its input, so name the `.nib` files `disk1.nib` to `disk3.nib`, or
rename the output), the tests replay the reference runs and compare them with
traces measured from the original (`reference/*.csv`). Without the game
data those tests skip themselves. Two use further local-only files, since they
hold game data: `save_disk.rs` (`original/disk4.nib` and the original's save disk
`extracted/reference/save_disk_pass2001.nib`) and `text_page.rs`
(`extracted/reference/textpage_speak_king.txt`, a dump of the original's text
screen).

## Comparing the screen with the original

The traces compare the game's state; `framediff` compares what is on screen, dot by
dot, on every pass. Record the original with refemu, then replay the same key log in
the port:

```
cargo run --release -p kq1-apple2-refemu -- original/disk1.nib original/disk2.nib original/disk3.nib     --keys routes/max_score.keys --passes 17340 --frames orig.frames
cargo run --release --example framediff -- routes/max_score.keys orig.frames 17339 framediff-out
```

It prints each run of differing passes and writes the worst frame of each as an image
(the original, the port, and the differing dots in red). `FRAMEDIFF_DUMP=p,…` adds
images and the port's actor table for those passes; `FRAMEDIFF_CROP=x0,y0,x1,y1`
zooms the images into a region. `FRAMEDIFF_SWAP=1` shows the disk-swap prompts, as
`kq --swap-prompts` does. With it, the screen matches the original on every pass of
every recorded route; without it, only the passes where the original asks for another
disk ("Please insert side B and press RETURN.") differ.

## Documentation

- [docs/port-plan.md](docs/port-plan.md): goals, milestones, what was checked and how,
  and what is still open
- [docs/disk-layout.md](docs/disk-layout.md): boot chain, addressing, directories
- [docs/picture-format.md](docs/picture-format.md), [docs/view-format.md](docs/view-format.md):
  pictures and sprites
- [docs/script-format.md](docs/script-format.md): the script VM and every opcode
- [docs/motion.md](docs/motion.md): the game cycle, movement, the RNG, the joystick
- [docs/text-and-parser.md](docs/text-and-parser.md): text output, the parser, sound
- [docs/save-format.md](docs/save-format.md): saves, restore and the save disk
- [docs/route.md](docs/route.md): the maximum-score route and the autopilot
