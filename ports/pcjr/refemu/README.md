# refemu-pcjr: reference IBM PCjr (measuring tool only)

This crate is a minimal IBM PCjr used to **measure the original game**: what state each
pass of the game cycle produces, how long it takes, and what is on screen. The port in
the parent crate does not use it; like the Apple port's refemu, it only provides
evidence for checking the port.

## What it emulates

- **CPU:** an 8086 core with 8088 cycle counts (see "Timing").
- **Memory:** 128 KB, shared with the video as on the PCjr. The Video Gate Array's CRT
  page (what is shown) and CPU page (the B800 window) are set through `INT 10h` AH=05h
  or port 3DFh.
- **The timer:** IRQ0 every 262,144 cycles (18.2 Hz), through `INT 08h` to the game's
  `INT 1Ch` handler, which plays the music and reads the keyboard.
- **The disk:** read from the user's image, sector by sector, with the size the
  diskette-parameter table asks for. So the original's 1 KB-sector engine tracks load
  as they do on hardware from an `.mfm` dump. The crack's `.img` works too.
- **No IBM ROM.** The BIOS services the game calls are answered natively. Any other
  call stops the run with a note naming it.
  - `INT 10h`: AH=00h (mode), 01h, 02h, 03h, 05h (page registers), 06h (scroll),
    09h, 0Eh (characters).
  - `INT 13h`: read, write, format.
  - `INT 16h`: AH=00h, 01h, 04h.
  - `INT 1Ah`: AH=00h.
- **Text:** characters are drawn by the BIOS from its ROM font, which isn't here. The
  emulator records the character in each of the 25 × 40 cells instead, and leaves the
  cell blank on screen. The frames carry those codes.
- **Sound:** every write to the SN76496 (port C0h) is recorded with its cycle.
- **Disk swaps:** the Master (the user's image, write-protected) and a Play diskette
  (`--play`). When the game is held at "Please insert your Play / Master diskette in
  the drive and press Enter", the emulator puts that disk in and presses Enter, as a
  player would.
- **Keyboard:** keys from a plan or a key log, returned by `INT 16h`. A game held at a
  prompt for 3 million cycles (no pass, and a blocking `INT 16h` read with nothing typed)
  gets RETURN, as refemu does for the Apple. So do the title screens, which only poll.
  The game's timer handler polls the keyboard every tick, so polling alone isn't a
  prompt.

## Timing

Each instruction costs:
- its Intel count, plus effective-address time and 4 cycles per word on the 8-bit bus;
- plus 4 cycles for each instruction byte fetched. Intel's counts assume a full prefetch
  queue, so the 8088's fetches are left out of them.
- plus, optionally, the Video Gate Array's wait states on every shared-RAM access
  (`--wait-states`, default 0).

The BIOS services, answered natively, are given estimated times.

**Against 86Box** ([../tools/86box/](../tools/86box/README.md)), with the original from
power-on, Up pressed in room 1:

| | refemu-pcjr | 86Box |
|---|---|---|
| Walking up (one row a pass) | 13.8 rows a second | 12.5 |
| Up to leaving room 1 | 6.5 s | about 6.6 s |
| Up to room 16 shown | 10.0 s | 9.1 s |
| Of which loading and drawing | 3.5 s | about 2.4 s |

Two simpler rules were tried. An instruction taking the longer of its count and its bus
time made walking 19.2 rows a second. Count plus 4 cycles for every byte moved fitted
walking exactly but loaded the room in 4.2 s, because it counts the data transfers twice.
Closer agreement needs a cycle-level 8088 (the prefetch queue, string-instruction
timing).

**86Box is not the last word.** It adds no PCjr wait states for this game (only through
the B800 window, which the game doesn't use), and a real PCjr does. The real figure needs
real hardware. Until then the cycle counts are an estimate, good to about 10% against
86Box.

## The game cycle

One pass is one call of `$14F2`. It moves and animates the actors, then runs the cycle
rules: the global script's (repeating while asked), then the room's. The input wait
(`$13CE`) calls it over and over while the player types. The main loop (`$D1C`) comes
round only after a typed line, to run the parser rules. So, unlike the Apple version,
the parser rules don't run inside a pass.

## Usage

```
cargo run --release -p kq1-pcjr-refemu -- original/<disk>.mfm \
    --plan "w30,R,w60,U,w40,>look,w40" --passes 300 --trace trace.csv --frames frames.bin
```

**The plan** runs in passes:

| Item | Meaning |
|---|---|
| `wN` | wait N passes |
| `U` `D` `L` `R` | an arrow key (scan code, no character) |
| `S` | the last arrow key again: the ego stops |
| `>text` | type the text and RETURN |
| `kHH` | a character, by its code |
| `sHHHH` | a key as `INT 16h` returns it (scan code << 8 \| character) |
| `p…` | the item after the `p` (a key or `>text`), typed once the game waits for a key: for prompts, which hold the game cycle |
| `@play`, `@master` | put the Play diskette or the Master in the drive |
| `p@play`, `p@master` | at the next prompt, put that disk in the drive and press Enter (what "Please insert your Play diskette in the drive and press Enter" asks) |

`--plan @FILE` reads the plan from a file: items separated by commas or lines, with
`#` comments. The PCjr's routes are plan files (`routes/*.plan`): a key log with pass
numbers can't say "at the next prompt", and the prompts hold the game cycle.

**Other options:**
- `--keys FILE`: a key log, lines of "PASS KEY-HEX", each pressed at the start of that
  pass.
- `--passes N`: stop after N passes (default 400).
- `--ticks N`: stop after N timer ticks.
- `--play FILE`: the Play diskette, as a 360K sector image. It's read if it exists and
  written back if the game wrote to it. The Master is write-protected, as the original
  was, so saving to it fails as it did ("Games may only be saved on your Play
  diskette").
- `--wait-states N`: Video Gate Array wait states per access to the shared RAM (default
  0, as 86Box; IBM gives 2 on average for real hardware). See "Timing".
- `--sector-cycles N`: CPU cycles per sector read or written (default 100,000, about
  21 ms: a 300 rpm drive with 9 sectors a track).
- `--cost FILE`: per pass, the execution-unit cycles, memory accesses, disk cycles and
  accesses through the B800 window, for calibrating.
- `--pictures DIR`: once the engine has started, draw every room's picture with the
  engine's own routines (clear `$615`, frame `$353F`, picture `$3534`) and write the
  buffers to `DIR/NNN.bin` (168 rows of 160 bytes, priority << 4 | colour). They are
  the reference for the port's painter (test `every_picture_matches_the_engine`).
- `--shot FILE.png`: the screen at the end.
- `--profile`: where the CPU spent its time, and how often the main routines were
  entered.
- `REFEMU_DUMP=p,…`: print actor 0 and v0–v47 at those passes.
- `REFEMU_KEYLOG=1`: log each key the game reads, each automatic RETURN, each disk
  swap, and every tenth pass with its cycle.

**Output:**
- **`--trace`:** a CSV per pass, in the Apple refemu's columns: pass, cycles, cycles
  in the pass, room (v1), ego x, y and direction, score (v19), and a hash of v0–v222.
- **`--frames`:** the screen at the start of each pass, written when it changes. Each
  record is the pass (u32), then 200 rows of 320 4-bit colour numbers (one byte each),
  then the 25 × 40 text cells.
- **A summary:** cycles per pass, the BIOS calls made, ports nothing models, and the
  text on screen.
