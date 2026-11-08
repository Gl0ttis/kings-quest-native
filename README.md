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
| IBM PCjr | 1984, Sierra/IBM | Planned | |

## Layout

```
Cargo.toml              workspace
ports/
  apple2/               the Apple IIe port: crate kq1-apple2 (engine, kqplay, tools)
    refemu/             crate kq1-apple2-refemu: the reference emulator (measuring only)
    docs/ routes/ tools/
    original/           your disk images (not in git)
    extracted/          decoded data (not in git); extracted/reference/*.csv are kept
.github/                CI, PR title check, release-please, Dependabot
```

Each port is self-contained in `ports/<platform>/`. Code shared between ports will go
into crates under `crates/` when a second port needs it.

## Quick start (Apple IIe)

Put the three game disk images (`.nib`) in `ports/apple2/original/`, then:

```
cd ports/apple2
cargo run --release --bin kqplay -- "original/<disk 1>.nib" "original/<disk 2>.nib" "original/<disk 3>.nib"
```

See [ports/apple2/README.md](ports/apple2/README.md) for keys, options and tools.

## Contributing

Conventional commits, semantic versioning per crate, and the checks every pull
request must pass are described in [CONTRIBUTING.md](CONTRIBUTING.md).
