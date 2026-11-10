# Disk layout (IBM PC, 1984, CGA)

The game is one self-booting 5.25-inch disk, Sierra's own release for the IBM PC with
the Color/Graphics Adapter. Its boot sector reads "BOOT v1.1 (c) Copyright Sierra
On-Line, Inc. 1984". Unlike the PCjr's disk, it is a DOS-format disk with a FAT
filesystem, though it doesn't need DOS: its boot sector loads the files itself.

A second edition exists, the "CGA+RGB" disk ("BOOT v1.2"): its engine offers RGB
colour besides composite. It isn't this port's. The disk reader refuses it with a
message saying so.

[src/disk.rs](../src/disk.rs) implements this document, and `kqextract-pc` checks it on
every resource. Addresses are in the engine's code segment (segment `0060h`) unless
marked otherwise.

## The disk and its copy protection (confirmed)

- **Format.** 40 tracks × 2 sides at 300 rpm, 250 kbit/s MFM, 9 sectors of 512 bytes,
  formatted as a DOS 360K disk:
  - the boot sector (sector 0);
  - two FATs of 2 sectors (sectors 1-4);
  - the root directory's 7 sectors (5-11);
  - clusters of 2 sectors from sector 12.

  "Sector *n*" below is the *n*th 512-byte sector in the usual order (track by track,
  side 0 before side 1), from 0. The engine converts it to track, side and sector the
  same way (`$1FC6`).
- **The files.** The original's root directory holds three:

  | File | Size | Sectors | What |
  |---|---|---|---|
  | `LOAD` | 3,158 | 12-19 | the loader ("LOADER v1.15") |
  | `MAIN.EXE` | 30,448 | 20-79 | the engine, an EXE |
  | `290K` | 296,960 | 140-719 | the data area, the resources in it |

  `290K` spans the whole rest of the disk. The engine reads the resources by sector
  number, not through the file, so the file is there only to mark the space as used.
- **The protection.** Track 6 side 0's first sector (sector 108, outside every file)
  can't be read: its ID field says 256 bytes (size code 1) and fails its CRC. The
  original's `MAIN.EXE` starts at code in its last 773 bytes (its header's CS:IP is
  `071F:0000`), after which it runs the rest of the program from 0. Every other sector's
  CRC is good. The CGA+RGB edition's protection sector is on the same track, with a
  good ID and a bad data CRC.
- **The images people have.** Two kinds:
  - **The original as dumped.** An HxC `.mfm` image (each track's raw bitstream),
    decoded by the shared [mfm decoder](../../../crates/kq1-formats/src/mfm.rs). The
    reader accepts the image only with exactly the protection's sector unreadable. The
    verified dump, "[cp] [!]", has MD5 `ff57aa96097165a26651102444fb9efe`. Its `.86f`
    twin is not read yet.
  - **The crack.** A 360K sector image ("[cr]", MD5
    `4793aac13953e1501872042f3be3b730`). Its `MAIN.EXE` is the original's without the
    last 773 bytes, with its header's CS:IP set to `0000:0000`; its `LOAD` is patched;
    the FATs and the directory are changed to match, and three small files are added
    (`IBC`, `PROTECT`, `PROTECT.NO`, `PROTECT.YES`). The resource table and every
    resource are the same as the original's (`the_original_and_the_crack_hold_the_same_game`).

## Boot (partly confirmed)

1. **Boot sector.** The BIOS loads it to `0000:7C00`. It:
   - points the diskette-parameter vector (`INT 1Eh`) at its own table;
   - reads the FAT and the root directory;
   - checks that the first directory entry is `LOAD`, and loads that file.
2. **The loader** (`LOAD`) loads `MAIN.EXE` and starts it. (Its work isn't read yet.)
3. **The engine's start-up**, from CS:IP 0 (the crack's; the original's first runs its
   tail at `071F:0000`):
   - copies its image to `0060:0000` and continues there (`$17`), so engine address
     *a* is byte *a* of the EXE's image (after its 512-byte header);
   - sets the equipment word to colour 80×25 and BIOS mode 4 (`$2F`);
   - points the parameter vector at its own table at `$7B` (512-byte sectors, 9 to a
     track);
   - sets DS = SS = CS + `$511` (`$3E`, the relocated `$511` less the 32 paragraphs of
     the header), so the data segment starts `$5110` bytes into the code segment;
   - reads **sectors 10-11 to CS:`$88`** (`$5A`, through its read routine `$1FB6`):
     the resource table, over the code that was there;
   - continues at `$2094`.

## The resource table (confirmed)

The table is 1 KB: sectors 10 and 11, in the root directory's area after its used
entries. The engine reads it over its code at `$88`. Its 4-byte entries are in the
PCjr's format (ports/pcjr/docs/disk-layout.md), with all-zero entries unused (the
PCjr's are `****`).

| Resources | Entries | Numbers |
|---|---|---|
| Rooms (script + picture) | `$88 + 4r` | 0-84; 0 is the global script, 81-84 are empty |
| Sounds | `$88 + 4s`, numbers 90-99 | 10 resources of four sounds each |
| Views | `$288 + 4v` (number 128 + v) | 0-110 |

**An entry** `b0 b1 b2 b3`:

| Field | Bits |
|---|---|
| Offset in the first sector | `b0`, plus bit 7 of `b1` as bit 8 (0-511) |
| Sectors to read | `(b1:b2 >> 4) & 0x3FF` |
| First sector | `b2:b3 & 0x3FF` |

For example, room 1's entry `18 80 80 A6` is offset 280 in sector 166, read as 8
sectors.

**Reading** (`$1FB6`): DX = first sector, CX = sector count, ES:BX = buffer, through
`INT 13h` AH=02h, retried 3 times, a track at a time. A read that would cross a 64 KB
boundary for DMA goes through a buffer at `1000:0` (`$2032`).

## Resources (confirmed)

The resources are in the PCjr's formats (ports/pcjr/docs/disk-layout.md):

- **Rooms:** the script, then the picture. All 80 well-formed scripts parse with the
  PCjr's operand counts, and all 79 pictures after them draw cleanly to their `$FF`
  with the PCjr port's painter. Room 64 has the header defect every release's disk
  has.
- **Views:** all 111 entries are byte for byte the PCjr's views (88 of them with cels,
  23 empty).
- **Sounds:** in the PCjr's format, streams of sound-chip writes. Seven of the ten
  resources are byte for byte the PCjr's; all 40 sounds parse. The PC has only its
  speaker; how its engine plays them is still to find.
- **The vocabulary:** 369 words in the Apple version's format, stored plainly (the
  PCjr's is bit-rotated), at DS:`$4`.

**The scripts are a different revision from the PCjr's.** Rooms 27, 54 and 74 are the
same, and so are the empty stubs; the others differ in two ways:
- variable and flag numbers mostly shifted up by 2 or 3 (for example 101 → 103, 68 →
  70, 127 → 130);
- in a few rooms, added or changed content: rooms 21, 35, 42, 46, 51, 75, 76 and 79
  have more actions or messages, and the global script (room 0) differs throughout.

**The pictures** are the PCjr's byte for byte, but for eight rooms': 8, 18, 25, 28, 39,
41, 52 and 65.

## The display (to confirm in the engine; the colours: [src/composite.rs](../src/composite.rs))

The engine sets BIOS mode 4, then writes `1Ah` to the CGA's mode register (`3D8h`,
`$19AA`): 640×200 graphics with the colour burst on, so that on a composite monitor
each 4-dot group shows as one of 16 colours. It writes `27h` to the colour register
(`3D9h`): the foreground is colour 7, not 15. A routine at `$1966` copies the picture
from a buffer at segment `1400h` to the screen at `B800h`, taking each pair of
picture pixels' high nibbles into one byte (two 4-dot groups): 168 rows of 80 bytes,
in CGA's interleaved rows. Like the PCjr's engine, it writes the CRTC's register 2,
the horizontal sync position, from a variable (`[$112F]`, `2Dh` at start).
