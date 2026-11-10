# King's Quest I: native ports

Native reimplementations of Sierra's *King's Quest* (1984), one per original
release, each behaving as that release did: same rules, same timing, same quirks.

- **No emulation.** No CPU emulation and no instruction-by-instruction
  translation. Each engine is rewritten from its documented behaviour.
- **Bring your own game.** The ports ship engines only. They read your own copy
  of the original disk images at run time and never include or redistribute any
  game content, and no ROM images.
- **Checked against the original.** Each port is compared pass by pass with the
  original running in a reference emulator, used only as a measuring tool.

## Ports

| Port | Original | Status | Crates |
|---|---|---|---|
| [Apple IIe](ports/apple2/README.md) | 1984, Sierra, Apple version by Jeff Stephenson (double hi-res, 128K) | Playable start to finish; matches the original on every pass of a 158-point playthrough | `kq1-apple2`, `kq1-apple2-refemu` |
| [IBM PC (CGA)](ports/pc/README.md) | 1984, Sierra (self-booting; CGA: composite colour, and RGB in the second edition) | Playable start to finish, both editions (the first, and the second with RGB colour); matches the originals on every pass, screen and speaker note of three whole-game playthroughs each; its Play diskette is the original's | `kq1-pc`, `kq1-pc-refemu` |
| [IBM PCjr](ports/pcjr/README.md) | 1984, Sierra/IBM (self-booting; PCjr only) | Playable start to finish; matches the original on every pass and screen of three whole-game playthroughs; its Play diskette is the original's | `kq1-pcjr`, `kq1-pcjr-refemu` |

## Layout

```
Cargo.toml              workspace
docs/porting-guide.md   how a port is built and checked; start here for a new one
crates/
  kq1-formats/          the resource formats every port reads (views, scripts, vocabulary)
  kq1-frontend/         the players' platform side (Windows paint hook, full screen, joystick)
  kq1-ibm-player/       the IBM PC and PCjr players' shared half (clocks, window, sound output)
  kq1-ibm/              what the IBM PC and PCjr engines share (the clock, the picture painter,
                        the text screens, the views, the table entries, the built-in font)
  kq1-x86/              the 8086 core of the IBM ports' reference machines (measuring only)
  kq1-autopilot/        the IBM ports' route autopilot, kqroute (measuring only)
ports/
  apple2/               the Apple IIe port: crate kq1-apple2 (engine, kq-apple2, tools)
    refemu/             crate kq1-apple2-refemu: the reference emulator (measuring only)
    docs/ routes/ tools/
    reference/          traces measured from the original (numbers only), for the tests
    original/           your disk images (not in git)
    extracted/          decoded game data (not in git)
  pc/                   the IBM PC port (CGA): crate kq1-pc (engine, kq-pc, tools)
    refemu/             crate kq1-pc-refemu: the reference PC with the CGA (measuring only)
  pcjr/                 the IBM PCjr port: crate kq1-pcjr (engine, kq-pcjr, tools)
    refemu/             crate kq1-pcjr-refemu: the reference PCjr (measuring only)
    docs/ routes/ tools/ reference/ original/ extracted/   as for the Apple port
.github/                CI, PR title check, releases (release-plz), Dependabot
```

Each port lives in `ports/<platform>/`. To start a new one, read
[docs/porting-guide.md](docs/porting-guide.md). Code the ports share lives in crates
under `crates/`: `kq1-formats`, the resource formats, which the releases have in
common; `kq1-frontend`, the platform side of every player; `kq1-ibm-player`, the half
of the IBM PC and PCjr players that is the same; `kq1-ibm`, what the IBM PC and PCjr
engines share;
and, for measuring only, `kq1-x86` and `kq1-autopilot`, the IBM ports' reference
machines' 8086 core and their route autopilot. Each port's player is `kq` in its download (`kq-<port>` in the workspace).

## Playing

Download the port's archive for Windows or Linux from the
[releases page](https://github.com/Gl0ttis/kings-quest-native/releases), unpack it into
a folder of its own, put your own game disk images next to `kq` in it and start `kq`:

- Apple IIe: `kq-apple2-<version>-<system>`, with the three game disks.
  [ports/apple2/PLAYING.md](ports/apple2/PLAYING.md) has the details, keys and options.
- IBM PC: `kq-pc-<version>-<system>`, with the game disk.
  [ports/pc/PLAYING.md](ports/pc/PLAYING.md) has the details, keys and options.
- IBM PCjr: `kq-pcjr-<version>-<system>`, with the game disk.
  [ports/pcjr/PLAYING.md](ports/pcjr/PLAYING.md) has the details, keys and options.

## Building

Put the disk images in the port's `original/` folder (git ignores it), then:

```
cargo run --release -p kq1-apple2 --bin kq-apple2 -- ports/apple2/original
cargo run --release -p kq1-pc --bin kq-pc
cargo run --release -p kq1-pcjr --bin kq-pcjr
```

See [ports/apple2/README.md](ports/apple2/README.md),
[ports/pc/README.md](ports/pc/README.md) and
[ports/pcjr/README.md](ports/pcjr/README.md) for the tools, tests and documentation.

## Licence

The code is under the [MIT licence](LICENSE). King's Quest itself is Sierra's and
is not part of this repository: the ports read your own copy.

## Contributing

Conventional commits, semantic versioning per crate, and the checks every pull
request must pass are described in [CONTRIBUTING.md](CONTRIBUTING.md).
