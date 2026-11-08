# Picture format

Room pictures are the resources listed in the `$1700` directory (see
[disk-layout.md](disk-layout.md)). They are vector drawing programs. The
interpreter lives in the aux-memory code that loads from `fixed/aux_09C0_code`.
All addresses below are aux addresses.

[src/pic.rs](../src/pic.rs) implements the format. Running `kqpic` renders all 83
pictures to `extracted/pics/`: the visual image, the priority image and a contact
sheet. Every picture parses cleanly to its `$FF` terminator, and every render is a
recognisable scene.

## How the engine draws a picture (confirmed)

- **Loading.** The picture resource loads into aux `$9800`. It is then copied to
  aux `$D000` (aux entry 16, `$1396`). The interpreter (aux entry 10, `$0B83`)
  reads it from the start through `$AA`/`$AB`, so the data begins directly with
  an opcode. Main-memory code calls aux routines through `$C314` (XFER), using
  the entry table at `$09F1`.
- **Buffer.** The picture is drawn into a 140×160 byte buffer at aux `$4000`,
  140 bytes per row (`$0F01`). The low nibble of each byte is the visual colour,
  stored as an Apple double hi-res colour value; the high nibble is the
  priority. The buffer starts as `$1F` everywhere (`$0EE1`): visual 15 (white),
  priority 1.
- **Coordinates.** Picture coordinates are offset by (10, 3). Valid points run
  from x 10–149 and y 3–162; anything outside is silently clipped (`$1079`).
- **Screen.** Each plot also writes to the double hi-res screen. One logical
  pixel is 4 dots wide and one scanline tall.

## Drawing state

| Variable | Meaning | Initial value |
|---|---|---|
| `$C3` | colour byte: (priority << 4) \| visual | `$10` |
| `$C4` | write mask: `$0F` = visual, `$F0` = priority, `$FF` = both | — |
| `$C0` | alternate mask, used by F8–FB and toggled by F0 | `$F0` |
| `$F0`, `$F1` | current point | — |
| `$C5`, `$C6` | target point | — |

## Opcodes (dispatch table at `$16C2`)

`$FF` ends the picture. A byte below `$F0` where an opcode is expected also ends
it. Commands that take a list of arguments keep reading until the next byte of
`$F0` or above, and that byte is then run as the next opcode.

| Op | Arguments | Action | Mask |
|---|---|---|---|
| F0 | — | `$C0 ^= $FF` | — |
| F1 | c | visual = `$16E0[c]` | — |
| F2 | p | priority = p | — |
| F3 | c p | visual and priority | — |
| F4 / F8 | x y, then y x y … | corner lines: change y, then x, alternately | FF / `$C0` |
| F5 / F9 | x y, then x y x … | corner lines: change x, then y, alternately | FF / `$C0` |
| F6 / FA | x y, then (x y)… | absolute polyline | FF / `$C0` |
| F7 / FB | x y, then d… | relative polyline (see below) | FF / `$C0` |
| FC | c p, then (x y)… | flood fill, visual and priority | FF |
| FD | p, then (x y)… | flood fill, priority only | F0 |
| FE | c, then (x y)… | flood fill, visual only | 0F |

- **Relative step byte (`$0CC4`).** Bit 7 makes the x step negative, and bits
  6–4 give its size. Bit 3 makes the y step negative, and bits 2–0 give its
  size. The step is added to the target point using 8-bit wrapping arithmetic.
- **Line (`$0F63`).** The first point of a polyline is plotted once. Vertical and
  horizontal lines step one pixel at a time. Other lines use a DDA: the major
  axis runs `max(dx, dy)` steps, and two 8-bit accumulators start at half the
  major length. The ADC carry is discarded before the compare, so very long
  lines wrap the same way as on the Apple.
- **Fill (`$0D47`).** A stack-based scanline fill (`$0D7E`–`$0E8E`). Visual and
  combined fills spread over pixels whose visual nibble is still 15. A
  priority-only fill spreads over pixels whose priority is still 1. A fill whose
  colour equals that background value does nothing.
  - **Stack entries.** Each entry is 5 bytes: right end, left, cursor, y, and how
    it was pushed. A span is filled rightward from its cursor, then leftward from
    cursor − 1.
  - **Children.** Next the row above is scanned from the cursor, and the row below
    from the left end. A child entry is pushed at each fillable pixel found. When
    a child finishes, the parent resumes after the child's right end.
  - **A quirk.** The leftward scan ends with `DEY; BPL`, so it continues only while
    the x index stays below 128. From x index 129 up, it fills one pixel and
    stops. That leaves the odd fillable pixel unfilled near the right edge; room
    1's picture has one at (147, 157).
  - **In the port.** The fill is a literal transcription, quirk included. A
    complete 4-connected fill differs from the original in exactly such pixels.

## Colours (confirmed against the original's screen)

`$16E0` maps picture colour indices 0–15 to the double hi-res values
`00 02 04 0E 01 03 08 0A 05 06 0C 07 0B 03 0D 0F`. The plot routine rotates each
value right by one bit before building the screen pattern (`$0F3D`). Read
lowest bit first, the rotated pattern names these Apple colours:

| Index | Apple colour | Index | Apple colour |
|---|---|---|---|
| 0 | black | 8 | grey 1 |
| 1 | dark blue | 9 | medium blue |
| 2 | dark green | 10 | green |
| 3 | aqua | 11 | light blue |
| 4 | magenta | 12 | pink |
| 5 | purple | 13 | purple |
| 6 | brown | 14 | yellow |
| 7 | grey 2 | 15 | white |

This is the PC 16-colour order, with the nearest Apple colour standing in where
there is no exact match (light magenta becomes purple). The fact that all 16
entries fit that order is the evidence that this reading is right. `dhgr_rgb`
uses Apple IIgs RGB values for the Apple colours; real NTSC output varies.

**Checking the mapping.** The emulator ([refemu](../refemu/README.md)) reads the
original's double hi-res memory four dots at a time, in Apple IIe Technical
Reference order. Its screenshot of room 1 matches the port's rendering on all
89,600 dots of the picture area.

**Checking the buffer.** The original's picture buffer (aux `$4000`, colour and
priority, with the sprites drawn in) matches the port's composed buffer byte for
byte. That holds for room 1 at start-up and for room 16 with Graham walking in
it, 22,400 bytes each.

## Picture inventory

- **1–80:** room backgrounds.
- **82:** Sierra logo.
- **83:** King's Quest title scroll.
- **84:** a beanstalk on a blank background. It is probably drawn over another
  room picture.
- **81:** its directory entry is invalid. **0:** not a picture.

Most pictures are followed by 1–24 unused bytes after the `$FF`.

## Still open

- Which routine draws picture 84, and when (`$5FA4` draws a second picture from
  aux `$D800`).
- The exact NTSC colours, if an NTSC-style display option is wanted. The RGB
  values per Apple colour are a display choice; the colours themselves are
  confirmed.
