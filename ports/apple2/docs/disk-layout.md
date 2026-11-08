# Disk and resource layout

This describes King's Quest (Apple II, 1983–84, Apple version by Jeff Stephenson)
as it is stored on disks 1–3. All engine addresses refer to the 6502 engine image
that loads at `$4000`. Each claim is marked as **confirmed** (read directly from
code or data) or **probable** (fits the evidence, not yet proven).

## Boot chain (confirmed)

1. **Stage 0, disk 1 T0 S0 at `$0801`.** This is the stock DOS 3.3 boot sector.
   It loads T0 logical sectors 9 down to 0 into `$BF00` down to `$B600`, then
   jumps to `$B700`.
2. **Stage 1, `$B700` (T0 S1).** It initialises the screen and keyboard, then
   makes one RWTS call through the parameter block at `$B7E8`. That call reads
   50 sectors from **T11 S0 into `$4000`–`$71FF`**. A second call has a count of
   0 and reads nothing. Stage 1 then runs `JMP $4000`.
3. **RWTS at `$B800`–`$BFFF`.** This is the stock DOS 3.3 RWTS; its interleave
   table at `$BFB8` is standard. Logical sector order therefore equals `.do`
   order: the byte offset of a sector is `(track*16 + sector) * 256`.

The engine is at offset `0xB000`–`0xE1FF` of `disk1.do`. `kqextract` writes it
out as `engine_4000.bin`. The engine banks the language card (`$C08B`/`$C082`)
and auxiliary memory (`$C005`), so the game needs a 128K Apple IIe.

## Disks (confirmed)

| Disk | RWTS volume | Role |
|---|---|---|
| 1 | 1 | Boot, engine, fixed resources, part of the rooms |
| 2 | 2 | Rooms |
| 3 | 3 | Rooms |
| 4 | 10 | Save disk |

- **The volume number identifies the disk.** The loader puts the expected disk
  number into the volume field of the RWTS parameter block (`$6F33`). If the
  volume read back (`$6F3E`) is different, it shows "Please insert side A/B/C" (the letters at `$6CD0`)
  (`$6075`).
- **Disk 4 is the save disk.** The "init disk" routine (`$4AEE`) formats a disk
  (RWTS command 4) with volume `$0A`.
- **Tracks 1–4 are byte-identical on disks 1–3.** They hold shared resources.

## Resource addressing (confirmed, engine `$6037`)

A resource is located by a 3-byte record:

| Byte | Meaning |
|---|---|
| 0 | Track |
| 1 | High nibble: disk (0 = any, see shared tracks); low nibble: sector |
| 2 | Byte offset within the sector |

At that address there is a little-endian `u16` length, followed by the data. The
data continues through consecutive logical sectors: after sector 15 comes sector
0 of the next track (`$5E3F`, `$5EC7`, `$605E`). The destination is the pointer
in `$57`/`$58`, and `$59` selects the memory bank: 0 = main RAM, 1 = aux RAM,
2 = language card.

## Fixed resources (confirmed)

The engine loads these at startup (`$44F6`–`$45B8`). The records are at
`$6781`–`$6795`.

| Record | Disk location | Length | Destination | Contents |
|---|---|---|---|---|
| `$6781` | d1 T15 S0 | 4894 | aux `$09C0` | 6502 code |
| `$6784` | d1 T17 S4 | 768 | aux `$1D00` | the font: 96 glyphs × 8 bytes (see text-and-parser.md) |
| `$6787` | d1 T17 S8 | 3155 | main `$0800` | parser vocabulary |
| `$678A` | d1 T18 S5 | 240 | main `$1500` | view sizes: 120 `u16` values, used when allocating memory for views (see view-format.md) |
| `$678D` | d1 T18 S6 | 896 | main `$1700` | the resource directories (below) |
| `$6790` | d1 T18 S10 | 1200 | main `$1A80` | object table: 111 records of 8 bytes, then the object names and 80 further bytes (see text-and-parser.md, save-format.md) |
| `$6793` | d1 T14 S15 | 199 | LC `$FF00` | 6502 code |

## Directories (confirmed)

The picture and script directories are each a 256-byte page of 85 three-byte
records; the view directory runs on to 120 entries (below). `$4318` looks up an
entry as `base + 3*index`.

| Page | Lookup | Loads to | Role | Evidence |
|---|---|---|---|---|
| `$1700` | `$4234` | aux `$9800` | room pictures (confirmed) | All 83 entries render as recognisable scenes; see [picture-format.md](picture-format.md) |
| `$1800` | `$42A2` | `$7068` (the global script); room scripts load right after it | scripts | Entry 0 is the global script, 1–80 the rooms. Every entry starts with four `u16` section lengths (see [script-format.md](script-format.md)) |
| `$1900` | `$4261` | aux, bump-allocated from `$9800` or `$D000`; the address is recorded at `$6F41+2n` | views (sprites), **120 entries** | It runs on through `$1A67`, since the lookup is `base + 3n`; this is the "`$1A00` table". 88 entries are valid and match the `$1500` size table. Loaded by `load_view` / `add_actor`; an empty entry prints "Object not found." |

- **Valid entries:** 81 room scripts (0–80) and 88 views. There are 83 picture
  entries in total, from 1–80 and 82–84.
- **Picture entry 0 is not a picture.** Its 12,810-byte span on disk 2 overlaps
  script 70 and the pictures and scripts of rooms 71 and 72, and it doesn't start
  with a drawing opcode.
- **Every other resource is disjoint from the rest.** This is the main check
  that the format above is right.

## Unresolved

- Where the engine stores the number of rooms.

The formats themselves are in [picture-format.md](picture-format.md),
[view-format.md](view-format.md), [script-format.md](script-format.md),
[text-and-parser.md](text-and-parser.md) (objects, vocabulary, font) and
[save-format.md](save-format.md).

Run `cargo run --release --bin kqextract -- extracted/disk1.do extracted/disk2.do extracted/disk3.do --out extracted/res`
to reproduce the extraction. `extracted/res/manifest.txt` lists every entry.
