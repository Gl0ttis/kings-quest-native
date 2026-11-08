# Disk layout (IBM PCjr, 1984)

The game is one self-booting 5.25-inch disk, the release published by IBM for the
PCjr. Its boot sector reads "(c)Copyright Sierra On-Line Inc 1984 (c)Copyright IBM
Corp. 1984". The engine runs only on a PCjr:
- it sets BIOS video mode 9 (320×200, 16 colours) and switches pages with `INT 10h`
  AH=05h, which only the PCjr BIOS has;
- it plays music on the SN76496 sound chip at port `C0h`, from an `INT 1Ch` timer
  handler.

It has no machine check and no CGA path.

[src/disk.rs](../src/disk.rs) implements this document, and `kqextract-pcjr` checks it on
every resource. Addresses are in the engine's code segment (segment `0060h`) unless
marked otherwise.

## The disk and its copy protection (confirmed)

- **Format.** 40 tracks × 2 sides at 300 rpm, 250 kbit/s MFM. All but six track-sides
  have 9 sectors of 512 bytes. "Sector *n*" below is the *n*th 512-byte sector in the
  usual order (track by track, side 0 before side 1), from 0. The engine converts it
  to track, side and sector the same way (boot sector `$9D`, engine `$4DA3`).
- **The protection.** Track 0 side 1 to track 3 side 0 (six track-sides) each hold
  **5 sectors of 1,024 bytes** (size code 3) instead. They are the engine, 30 KB,
  which the boot sector reads from there. A sector copier of the time couldn't
  reproduce them. Nothing else on the disk is unusual: every sector's CRC is good,
  and there are no deleted-data marks.
- **No filesystem.** Sector 1 starts like a DOS FAT (`FD FF FF`) and sector 2 repeats
  the boot sector. Nothing uses them. The sectors no resource uses hold the format
  filler `F6`.
- **The images people have.** Two kinds:
  - **The original as dumped.** An HxC `.mfm` image (each track's raw bitstream) is
    read directly: [src/mfm.rs](../src/mfm.rs) decodes it to sectors and checks
    every CRC. The verified dump checked here, "[cp] [!]", has MD5
    `95c63bfd099cdefad07f7daea078ece6`. Its `.86f` twin is not read yet.
  - **The crack.** A 360K sector image ("[cr]", MD5
    `bec03858f1d4ae20a74d37c84f52a49f`). It has the engine copied, byte for byte, to
    sectors 648–707, and the boot sector's loading code patched to read it from there:
    11 bytes before `$100`, the disk parameters and the read call. Every resource
    sector is the same as the original's, and so is the engine. The test
    `the_original_and_the_crack_hold_the_same_game` checks this.

## Boot (confirmed)

1. **Boot sector.** The BIOS loads it to `0000:7C00`. It:
   - sets the equipment word to colour 80×25 and BIOS mode 4;
   - points the diskette-parameter vector (`INT 1Eh`) at its own table, which says
     1,024-byte sectors, 5 to a track (`DF 02 25 03 05 …` at `$54`);
   - copies itself to `0060:0000` and continues there (`$90`).
2. **The engine.** The boot sector reads 30 sectors of 1,024 bytes from track 0 side 1
   (its read routine `$9D`, set for 5 sectors a track) to `0060:0200`, and jumps to
   `$50D`.
3. **Start-up.** The engine at `$50D`:
   - points the parameter vector at its own table at `$500` (512-byte sectors, 9 to a
     track);
   - switches the boot sector's read routine to that layout by patching its
     immediates at `$A2`, `$C0`, `$D3`, `$FA` (5 → 9 sectors, 6 → 10);
   - sets DS = ES = SS = CS + `$4E3`, with the stack in that segment;
   - stores DS at `cs:$50B`;
   - continues at `$C4C`.

So engine address *a* is offset `a − 0x200` in the engine's 30 KB.

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

**The table's first half is in the boot sector and its second half in the engine**
(from `$200`). So reading the resources needs the engine, which on the original is in
the 1 KB sectors.

## Resources

### Rooms: script and picture (confirmed)

Each room's resource is its **script** followed directly by its **picture**.

- **The script** has the Apple version's layout (ports/apple2/docs/script-format.md):
  - an 8-byte header of four `u16` section lengths, then the sections: actions, cycle
    rules, parser rules, messages;
  - a length of 1 means an empty section (`$4D74`).

- **The action opcodes** are the Apple version's, `$01`–`$43`. The interpreter
  (`$1ABB`) jumps through a table at `$23F3` with opcode − 1, and `$00` does nothing.
  The operand counts were read from each handler. Handlers fetch operands from the
  script pointer at `[bp+6]`, directly or through a helper that returns the advanced
  pointer. Three differ from the Apple's:
  - `$05` takes 1 (a room number, read by the room loader `$4B35`); the Apple's 0;
  - `$25` takes 1 (`$201F` → `$44FE`); the Apple's 0;
  - `$31` takes none (`$21BD`); the Apple's 2.

  With these ([src/script.rs](../src/script.rs)), every script but room 64's parses
  completely. What each opcode does on the PCjr is still to compare with the Apple
  version.
- **Room 64** shows the Apple disk's defect too. Its header gives section lengths 68,
  59, 1 and 103, but the parser section's real length is 0 and the messages start a
  byte early. The PCjr loader counts a length of 1 as an empty section, so it takes
  the script as 238 bytes, and the room's first message loses its "S" as on the Apple
  disks. **The picture follows at byte 239, but the engine starts it at byte 238**,
  on a stray `65`. The picture interpreter (`$354D`) stops at once on a first byte
  below `$F0`. If nothing else handles this, the original draws no picture in room 64.
  To confirm with the measuring tool.
- **The picture** starts where the script ends. All 79 room pictures that follow a
  well-formed script draw cleanly to their `$FF` end with the Apple port's picture
  decoder (`kqextract-pcjr`, pictures in `extracted/pics/`). The picture commands
  are the same. The PCjr interpreter is at `$3534`: commands `$F0`–`$FE` through 15
  handlers at `DS:$199A`, with the picture at linear `$F000`. The picture's colour
  numbers are IBM colours (below).
- **Picture size: 160 × 168, all of it shown.** The engine keeps the picture in two
  PCjr video pages: the visual picture at `1800:0` (page 6, the one displayed) and the
  priorities at `1000:0` (page 4). Mode 9's layout is four interleaved banks of
  `0x2000`, 160 bytes a row. A picture pixel is one byte holding its colour in both
  halves, two of mode 9's 320 pixels. The row table at `DS:$1828` has exactly 168
  rows, so pictures are drawn in full. The Apple version keeps x 10–149 and y 3–162
  (double hi-res can't show more), so it crops these same pictures by 20 columns and
  8 rows. The fill (`$6F7`) spreads over bytes still `FF` (visual) or `11`
  (priority). Below the picture, mode 9's last 32 rows (four text rows) hold the
  status and input lines.
- **Rooms 81–84** are empty stubs: four section lengths of 1, then `FF FF FF FF`.

### Views (confirmed)

A view is a `u16` total length, then:

| Part | Layout |
|---|---|
| View | `[0]` loop count, then a fixed table of **8 `u16` loop sizes** (unused slots 0) |
| Loop | `[0..4]` header, as in the Apple version (x step, y step, delay, cel count), then a `u16` **size** per cel |
| Cel | `[0]` height, `[1]` width, then the pixel stream |

The length counts the bytes after itself. Each loop and cel starts where the previous
one ends. The Apple version keeps offsets where this keeps sizes, and has no fixed slot
table. The cel streams use the Apple encoding.

[src/view.rs](../src/view.rs) reads them. All 88 views decode:
- every cel's stream ends within its listed size;
- every loop ends exactly at its listed size.

That's 138 loops and 722 cels; the Apple version has 674 cels. The other 23 entries
are empty, `01 00 FF`.

## Colours (confirmed)

The engine never sets the PCjr's palette registers, so colours 0–15 are the standard
IBM colours ([src/color.rs](../src/color.rs)). Pictures and views both use these
numbers. The Apple version maps picture colours through a table when it draws
(`COLOR_TABLE` in ports/apple2/src/pic.rs: the PC order, to the nearest double hi-res
colours), and its views were converted when it was made. The ego's colour `4` (red)
here is the Apple's `1`, which is `COLOR_TABLE[4]`.

### Sounds (confirmed)

**40 sounds, four to a resource.** The ten resources at room-table numbers 90–99 each
hold four sounds: an 8-byte header of four `u16` sizes, then the four streams.
- **Numbering.** Sound *n* (from 1) is stream `(n − 1) mod 4` of resource `(n − 1) /
  4` (`$41D5` loads it; the stream pointers go in a table at `$DD4`).
- **A stream** is records of: a count *k*; *k* bytes written to the SN76496 at port
  `C0h`; and a duration in 18.2 Hz BIOS timer ticks. A duration of `0` ends the sound
  and starts the next queued one; `FF` starts it again (`$853`–`$892`).
- **Playing.** The `INT 1Ch` handler (`$808`) plays one record whenever its countdown
  runs out. Before sound starts, `$B15` sets bits 5–6 of port `61h`, the PCjr's
  multiplexer, so the chip reaches the speaker. Silence is the 5 bytes at `$D24`
  (`$B1C`). A flag at `$E26` turns sound off, but the timing keeps running.
- **Contents.** Every record sets all four channels: volume and tone for the three
  tones, then the noise channel's volume and control.

[src/sound.rs](../src/sound.rs) parses all 40 streams, each ending on its end marker at
exactly its size. It also synthesises the chip (3.579545 MHz clock, three square
tones, a 15-bit noise register, 2 dB volume steps), and `kqextract-pcjr` writes
each sound to `extracted/sounds/NN.wav`. The synthesis is still to check against
real hardware recordings; the chip writes and their timing are exact.

## The rule matcher (confirmed)

`$47EE` matches the cycle rules (pointer `$FB6`, mode flag `[$1F13]` = 0), and `$47E1`
the parser rules (`$FB8`, flag 1). The rule format is the Apple version's
(ports/apple2/docs/script-format.md):
- **The rule bytes** are the same: `FF`, `FE`, `FC`, `FB`, `FD`.
- **Conditions** are (variable, value) pairs, with bit 7 meaning "not equal"
  (`$48CD`). Variables are at `DS:$1003`.
- **The ego rectangle** (`$48B7`) is taken from actor 0's x and y at `$11F9`, which is
  offset `$18`/`$19`, as in the Apple version.
- **On failure**, the matcher scans forward to the next `FE`, and a following `FE`
  offset after an action is a jump.

The differences are in the numbering:
- **Word ids** are the PCjr vocabulary's (below), not the Apple's.
- **Variable numbers** are often one lower than the Apple version's in the same rule
  (room 1's `3C`/`3B` are the Apple's `3D`/`3C`). The Apple conversion renumbered
  them. To map in full.
- **Words in parser rules.** A rule's verb or noun matches the typed one, or `FA`
  (`$4842`). With no noun typed, the parser sets the noun to 2 (`$E7A`). The Apple
  version instead treats noun 1 in a rule as any noun. To check against the data.

## The vocabulary (confirmed)

The Apple format (a word, NUL, then position, type and id; ending with an empty word),
with **every byte rotated right one bit**. It's in the engine's data segment (DS =
CS + `$4E3`) at `$16`, the start of the parser's word list (`[$CD6]`, walked from
`$E10`).

[src/vocab.rs](../src/vocab.rs) decodes all 358 words; the Apple version has 345.
- **Only on the PCjr:** "say", "copy", "number", "under", "handle", and "not".
- **The original authors' names**, words with no use in play: "arthur",
  "abraham", "charles", "tingley", "ken", "macneill", "doug", "greg", "rowland",
  "chris", "teresa", "hal", "schwab".
- **Typos** the Apple version fixed: "swiming", "torchh".
- **Only on the Apple:** "condor", "scepter", "gingerbread", "duck", "torch",
  "burn", "leave", "message", "restart", "init".

### Saves (seen; to confirm)

Saves go to a separate "Play diskette" that the game formats itself (`$3E1A`, `INT 13h`
AH=05h). It writes the state from sector `0x282 + 3 × slot` (`$3B15`), and the
whole of the state at `$1003`, `$1361` and `$11E1` (`$3AD4`).
