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
| [IBM PCjr](ports/pcjr/README.md) | 1984, Sierra/IBM (self-booting; PCjr only) | In progress: disk layout done, resources extract | `kq1-pcjr` |

## Layout

```
Cargo.toml              workspace
docs/porting-guide.md   how a port is built and checked; start here for a new one
ports/
  apple2/               the Apple IIe port: crate kq1-apple2 (engine, kq, tools)
    refemu/             crate kq1-apple2-refemu: the reference emulator (measuring only)
    docs/ routes/ tools/
    reference/          traces measured from the original (numbers only), for the tests
    original/           your disk images (not in git)
    extracted/          decoded game data (not in git)
  pcjr/                 the IBM PCjr port (in progress): crate kq1-pcjr
.github/                CI, PR title check, release-please, Dependabot
```

Each port is self-contained in `ports/<platform>/`. To start a new one, read
[docs/porting-guide.md](docs/porting-guide.md). Code shared between ports will go
into crates under `crates/` when a second port needs it.

## Playing (Apple IIe)

Download `kq` for Windows or Linux from the
[releases page](https://github.com/Gl0ttis/kings-quest-native/releases), put your three
game disk images next to it and start it. [ports/apple2/PLAYING.md](ports/apple2/PLAYING.md)
has the details, keys and options.

## Building (Apple IIe)

Put the disk images in `ports/apple2/original/` (git ignores it), then:

```
cd ports/apple2
cargo run --release --bin kq -- original
```

See [ports/apple2/README.md](ports/apple2/README.md) for the tools, tests and documentation.

## Licence

The code is under the [MIT licence](LICENSE). King's Quest itself is Sierra's and
is not part of this repository: the ports read your own copy.

## Contributing

Conventional commits, semantic versioning per crate, and the checks every pull
request must pass are described in [CONTRIBUTING.md](CONTRIBUTING.md).
