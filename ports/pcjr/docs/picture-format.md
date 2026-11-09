# Picture format and drawing (IBM PCjr)

The picture data is the same as the Apple version's (ports/apple2/docs/picture-format.md):
the same command bytes, coordinates and colour numbers. Here it is in the form the PCjr
engine draws it. The drawing routines are the PCjr's own, and differ from the Apple
version's in ways that change pixels. [src/pic.rs](../src/pic.rs) models them literally.

**Verified:** all 80 room pictures drawn by `pic::draw_room` are byte-identical, colour
and priority, to the buffers the original engine draws with its own code
(`refemu-pcjr --pictures`; test `every_picture_matches_the_engine`).

## The buffers

The picture is **160 × 168**, all of it shown. Two PCjr video pages hold it:
- the visual picture at `1800:0` (page 6, the one displayed);
- the priorities at `1000:0` (page 4).

Each is in mode 9's layout: four banks of `0x2000`, 160 bytes a row, row *y* at
`(y & 3) * 0x2000 + (y >> 2) * 160` (the table at `DS:$1828`). A picture pixel is one
byte with its colour in both halves (`$640`), two of mode 9's 320 pixels.

Before a room's picture (`$34F5`), the engine:
1. clears the visual page to `FF` and the priority page to `11` (`$615`);
2. draws a frame: the sequence at `DS:$1981`, `F3 00 00 F5 00 00 9F A7 00 00 FF`. This
   is a black, priority-0 rectangle round the edge, and fills stop at it;
3. draws the room's picture (`$3534`), read from linear `$F000`.

Overlays (script op `$25`) draw over the current picture without clearing.

## The interpreter (`$353F`)

The interpreter:
- reads a command byte; `FF` ends the picture, and any byte below `F0` stops it;
- dispatches `F0`–`FE` through 15 handlers at `DS:$199A`;
- has each handler return the next command byte;
- clears the `F0` toggle at the start of every picture.

**Points** (`$3684`): x then y. An x above `EF` is the next command; x is clamped to 159
and y to 167, where the Apple version discards points outside its window.

| Command | Handler | Does |
|---|---|---|
| `F0` | `$358F` | toggle `[$198D]` |
| `F1` / `F2` / `F3` | `$356D` / `$3576` / `$357F` | set the visual colour / the priority / both |
| `F4` / `F8` | `$35A9` / `$36A9` | a point, then alternate y and x moves (lines) |
| `F5` / `F9` | `$3597` / `$36A0` | a point, then alternate x and y moves |
| `F6` / `FA` | `$35DF` / `$36B2` | a point, then absolute line ends |
| `F7` / `FB` | `$3602` / `$36BB` | a point, then relative steps (x: bits 4–6, sign bit 7; y: bits 0–2, sign bit 3) |
| `FE` / `FD` / `FC` | `$364D` / `$365D` / `$3666` | a visual colour / a priority / both, then fill seeds |

## Plotting (`$6BA`)

- `F4`–`F7` write the colour and the priority.
- `F8`–`FB` write one of them: the colour if the `F0` toggle is set, the priority if
  not.

## Two line routines

- **The absolute line (`F6`, `$36C4`)** steps a Bresenham-style error term in 8-bit
  registers, which can wrap. With *n* the larger of the x and y distances, it plots
  *n* points (one if *n* is 0) after the start.
- **The other lines (`F4`, `F5`, `F7`, `$37AC`)** step along the longer axis by 1 and
  compute the other offset as `i × minor / major`. They round up when twice the 8-bit
  remainder exceeds the divisor. Neither the start nor a zero-length line is plotted.

## The fill (`$6DC`, `$6F7`)

The fill works on one page:
- **`FE` and `FC`** fill the visual page, through bytes still `FF`. `FC` also writes the
  priority over the same runs.
- **`FD`** fills the priority page, through bytes still `11`.
- **No-ops:** a fill whose colour is the boundary value does nothing, and so does a seed
  on a byte that isn't fillable.

It's a scanline flood fill:
1. Find the run of fillable bytes through the seed (`$794`, at most 255 each way).
2. Fill the run.
3. Queue the bytes below and above the run's **left end**, each with the run's width.
   Skip either one if it is the entry already on top of the queue.
4. Take queued entries newest first. If the entry's byte is fillable, take its run.
   If not, look for the next fillable byte within the queued width (`$7B8`) and take
   that run. If neither, drop the entry.
5. Stop at the empty marker.

The queue lives on the stack, with a `(0, 0)` marker at the bottom.

## Room 64

Its script header is off by one (the Apple disks share the defect). The engine
starts the picture on a stray `65` byte, so the interpreter stops at once: **on the PCjr,
room 64 shows only the white picture with its black frame**. The Apple version draws
its picture from a separate resource, so it shows the clouds.
