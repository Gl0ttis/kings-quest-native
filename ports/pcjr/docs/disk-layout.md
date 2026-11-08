# Disk layout (IBM PCjr, 1984)

The game is one self-booting 5.25-inch disk, the release published by IBM for the
PCjr. Its boot sector reads "(c)Copyright Sierra On-Line Inc 1984 (c)Copyright IBM
Corp. 1984". The engine runs only on a PCjr:
- it sets BIOS video mode 9 (320×200, 16 colours) and switches pages with `INT 10h`
  AH=05h, which only the PCjr BIOS has;
- it plays music on the SN76496 sound chip at port `C0h`, from an `INT 1Ch` timer
  handler.

It has no machine check and no CGA path.

[src/disk.rs](../src/disk.rs) implements this document, and `kqextract` checks it on
every resource. Addresses are in the engine's code segment (segment `0060h`) unless
marked otherwise.

## The image (confirmed)

- **Format.** 368,640 bytes: 40 tracks × 2 sides × 9 sectors × 512 bytes. Sectors are
  in the usual order: track by track, side 0 before side 1. "Sector *n*" below is the
  *n*th sector of the image, from 0. The engine converts it to track, side and
  sector the same way (boot sector `$9D`, engine `$4DA3`).
- **No filesystem.** Sector 1 starts like a DOS FAT (`FD FF FF`) and sector 2 repeats
  the boot sector. Nothing uses them. Sectors 5–105 hold the format filler `F6`.
- **The image holds sector contents only.** Whether the original disk had copy
  protection can't be seen in it. The dump checked here has MD5
  `bec03858f1d4ae20a74d37c84f52a49f`.

## Boot (confirmed)

1. **Boot sector.** The BIOS loads it to `0000:7C00`. It:
   - sets the equipment word to colour 80×25 and BIOS mode 4;
   - points the diskette-parameter vector (`INT 1Eh`) at its own table;
   - copies itself to `0060:0000` and continues there (`$90`).
2. **The engine.** The boot sector reads 60 sectors from sector 648 to `0060:0200`, and
   jumps to `$50D`.
3. **Start-up.** The engine at `$50D`:
   - patches the boot sector's read routine for its own use (the immediates at
     `$A2`, `$C0`, `$D3`, `$FA`);
   - sets DS = ES = SS = CS + `$4E3`, with the stack in that segment;
   - stores DS at `cs:$50B`;
   - continues at `$C4C`.

So engine address *a* is image offset `a − 0x200 + 0x51000` (sector 648 = `0x51000`).

## The resource table (confirmed)

The engine finds every resource through 4-byte entries at the start of its code
segment, from `$100` to `$4FB`. The first `$100` bytes there are the boot code, and the
loaded engine starts at `$200`. The lookup (`$4AFC`) reads the entry at
`CS:[base + 0x100 + 4n]`.

| Resources | Entries | Base | Numbers |
|---|---|---|---|
| Rooms (script + picture) | `$100 + 4r` | 0 (`$4D06`) | 0–84; 0 is the global script, 81–84 are empty |
| Sound-chip data (sounds; to confirm) | `$268 + 4s` | in the room table, numbers 90–99 | 10 |
| Views | `$300 + 4v` | `$200` (`$4BBF`) | 0–110 |

Unused entries are filled with `*` (`2A 2A 2A 2A`). After the table, at `$4FE`, is the
diskette-parameter table (`DF 02 25 02 09 2A FF 50 F6 00 02`).

**An entry** `b0 b1 b2 b3`:

| Field | Bits |
|---|---|
| Offset in the first sector | `b0`, plus bit 7 of `b1` as bit 8 (0–511) |
| Sectors to read | `(b1:b2 >> 4) & 0x3FF` |
| First sector | `b2:b3 & 0x3FF` |

(`b1:b2` and `b2:b3` are big-endian pairs.) For example, room 4's entry `25 80 40 6A`
is offset 293 in sector 106, read as 4 sectors.

**Reading** (`$4D93`): DX = first sector, CX = sector count, ES:BX = buffer, through
`INT 13h` AH=02h, retried 3 times.

## Resources

### Rooms: script and picture (confirmed)

Each room's resource is its **script** followed directly by its **picture**.

- **The script** has the Apple version's layout (ports/apple2/docs/script-format.md):
  - an 8-byte header of four `u16` section lengths, then the sections: actions, cycle
    rules, parser rules, messages;
  - a length of 1 means an empty section (`$4D74`).

  43 of the 80 room scripts and the global script parse completely with the Apple
  port's script reader. The rest stop at an opcode the Apple's table doesn't have
  (`$44` and up), or at an operand count that differs. So the PCjr engine has some
  opcodes of its own (sound is the likely difference) and some with other operands.
  To be worked out from its interpreter.
- **Room 64** shows the Apple disk's defect too: its header's section lengths don't
  match its rules. The defect is in the data both versions share.
- **The picture** starts where the script ends (8 + the section lengths). All 79 room
  pictures that follow a well-formed script draw cleanly to their `$FF` end with the
  Apple port's picture decoder (`kqextract`, pictures in `extracted/pics/`). The
  picture commands are the same; the colour numbering may differ (below).
- **Rooms 81–84** are empty stubs: four section lengths of 1, then `FF FF FF FF`.

### Views (layout confirmed on view 0; the reader is still to write)

A view is a `u16` total length, then:

| Part | Layout |
|---|---|
| View | `[0]` loop count, then a fixed table of **8 `u16` loop sizes** (unused slots 0) |
| Loop | `[0..4]` header, as in the Apple version (x step, y step, delay, cel count), then a `u16` **size** per cel |
| Cel | `[0]` height, `[1]` width, then the pixel stream |

The Apple version keeps offsets where this keeps sizes, and has no fixed slot table.
The cel streams match the Apple ones apart from the colour numbers. View 0, the ego,
has 4 loops of 8 cels, with heights and widths as in the Apple version, but the
PCjr's colour `4` where the Apple has `1`. The Apple version renumbered the colours
(see `COLOR_TABLE` in ports/apple2/src/pic.rs). Empty views are 1 byte.

### Sounds (to confirm)

The ten resources at numbers 90–99 are SN76496 data. Each starts with four `u16`
offsets, likely one per channel (three tones and noise), and the streams contain the
chip's latch bytes (`9F`/`BF`/`DF`/`FF`: silence a channel; `8x`/`Ax`/`Cx`: tone).

### Saves (seen; to confirm)

Saves go to a separate "Play diskette" that the game formats itself (`$3E1A`, `INT 13h`
AH=05h). It writes the state from sector `0x282 + 3 × slot` (`$3B15`), and the
whole of the state at `$1003`, `$1361` and `$11E1` (`$3AD4`).
