# refemu-pc: reference IBM PC with the CGA (measuring tool only)

This crate is a minimal IBM PC with the Color/Graphics Adapter, used to **measure the
original game**: what state each pass of the game cycle produces, how long it takes, and
what is on screen. The port in the parent crate does not use it. It is forked from
refemu-pcjr ([../../pcjr/refemu/](../../pcjr/refemu/README.md)), whose 8086 core it
copies.

## What it emulates

- **CPU:** the 8086 core of refemu-pcjr, with 8088 timing (see "Timing").
- **Memory:** 256 KB, and the CGA's 16 KB at B800.
- **The CGA:** its registers: the mode (3D8h), the colour (3D9h), the CRTC (3D4h/3D5h)
  and the status (3DAh, from the frame timing). The screen is its memory, shown in the
  640-dot mode the game sets, through the composite model (`--shot`).
- **The timer:** IRQ0 every 262,144 cycles (18.2 Hz), through `INT 08h` to the game's
  `INT 1Ch` handler, which plays the sounds, reads the keyboard and paces the game.
  Writes to the timer's channel 2 and to port 61h (the speaker) are recorded with their
  cycle.
- **The disk:** read from the user's image, sector by sector, with the timing of a
  real drive (see "Timing").
- **No IBM ROM.** The BIOS services the game calls are answered natively. Any other
  call stops the run with a note naming it.
  - `INT 10h`: AH=00h (mode), 01h, 02h, 03h, 05h (page), 06h (scroll), 09h, 0Bh, 0Eh
    (characters), 0Fh.
  - `INT 11h`, `INT 12h`: the equipment and the memory size.
  - `INT 13h`: read, write, format.
  - `INT 16h`: AH=00h, 01h.
  - `INT 1Ah`: AH=00h.
- **Text:** characters are drawn by the BIOS from its ROM font, which isn't here. The
  emulator records the character in each of the 25 × 40 cells instead, and leaves the
  cell blank on screen.
- **Keyboard:** keys from a plan, returned by `INT 16h`. A game held at a prompt (a
  blocking read with nothing typed, or the title screens' polling) for 3 million cycles
  gets Enter.

## Start-up

The original's start-up is the boot sector, which loads `LOAD`, which loads `MAIN.EXE`,
whose header starts it at the copy protection's code (its last 773 bytes). **That code
can't run without IBM's ROM:** after reading the protection's sector through `INT 13h`,
it programs the disk controller directly (a READ TRACK) and calls into the BIOS's own
diskette routines (`F000:E9B4`, `F000:ED44`), and it reads a byte of the BIOS's disk
table (`F000:EFC8`). Whatever it finds, it then jumps to the start of the program, CS:IP
0, which is where the crack's header starts it.

So refemu-pc starts the engine as the loader does, at that point: `MAIN.EXE`'s program
at segment 0080h (the file, header and all, at 0060h), its relocation applied, CS:IP
`0080:0000`, its header's stack. The engine's first instruction copies it to
`0060:0000`. The data segment then comes out at CS + `$511`, where the engine's tables
are (`kq1-pc`'s docs/disk-layout.md). The boot, the loader and the protection aren't
measured.

## Timing

Each instruction costs:
- its Intel count, plus effective-address time and 4 cycles per word on the 8-bit bus,
  as in refemu-pcjr;
- plus **1.5 cycles for each instruction byte fetched** (`--fetch-cycles`);
- plus **4 wait states for each access to the CGA's memory** (`--cga-wait`).

A disk read takes what a 5.25-inch drive takes, worked out from the cycle count:
- the motor's start, 250 ms, if it had stopped (2 s after its last use);
- the head's steps, 6 ms a track, and 15 ms to settle (the game's own disk table);
- the wait for the first sector to come round (300 rpm, the sectors at their places on
  the track);
- the sectors passing the head.

The BIOS services, answered natively, are given estimated times.

**The game paces itself on the timer:** each pass waits once (`$3FE7`) until the timer
interrupt clears a flag (`SS:$1E65`). So a pass lasts a whole number of ticks, one when
its work fits in a tick (about 262,000 cycles), two when it doesn't. This makes the
walking speed a sharp test of the timing.

**Against 86Box** ([../tools/86box/](../tools/86box/README.md)), with the original, Up
pressed in room 1:

| | refemu-pc | 86Box |
|---|---|---|
| Walking up (one row a pass) | 18.2 rows a second | 18.1 |
| Room 16's picture drawn | 2.39 s | 2.52 s |
| Up to room 16's screen cleared | 7.05 s | 6.51 s |

The two figures were fitted to the first two. With refemu-pcjr's 4 cycles a fetched
byte and no CGA wait states, the walk's passes didn't fit in a tick: 9.1 rows a
second. No flat charge a byte fits both the walk and the drawing on its own (1 cycle:
walking right, drawing 17% fast; 2 cycles: drawing right, walking 13% slow). The
CGA's wait states, which cost the drawing more than the walk, close most of the gap.
A model of the 8088's prefetch queue (bytes fetched while the bus is free, the queue
emptied by a jump) was tried in place of the flat charge, and was further off on both:
12.2 rows a second and 2.9 s.

## The game cycle

One pass is one call of `$293A` (the PCjr's `$14F2`, the same code with its data
`$606` on): the actors move and animate, then the cycle rules run. The variables are at
DS:`$1609`, the actor table at DS:`$17E7` (30 bytes an actor).

## The routes

Two tools on refemu-pc's runner (`src/runner.rs`, refemu-pcjr's with the PC's
addresses) record the reference the port is replayed against: the event log (the
timer's ticks, the keys and the disk swaps against the checkpoints, kq1-pc's
`checkpoints.rs`), the trace, and the screens.

- **`kqplan-pc DISK PLAN`** plays a plan (`../routes/*.plan`): keys at given passes,
  answers at prompts, the disk swaps answered as a player would.
- **`kqroute-pc DISK ROUTE`** plays a route (`../routes/*.route`) with refemu-pcjr's
  autopilot, which reads the original's memory (the PC's data addresses).
- **`kqframes-pc DISK EVENTS --hashes OUT`** follows a route's event log exactly and
  writes the screen's hash at every pass (the route's `.frames`); the followed run's
  trace is checked against the recorded one's.

The references are numbers only: the event log (`T`, `K`, `D` and `P` lines: the tick,
key, disk swap or pass and the checkpoint count), the trace (refemu-pcjr's columns) and
the frame hashes (FNV-1a of the CGA's picture area and the text).

The runner answers "Please insert your Play / Master diskette" from the message the game
last wrote (`last_row`): the game clears its message box by writing memory, which the
emulator's record of the characters doesn't see. Making a Play diskette copies the disk
with no pass of the game cycle, many swaps over (minutes of the PC's time).

## Running

```
cargo run --release -p kq1-pc-refemu --bin refemu-pc -- ../original/<disk>.mfm --plan "w20,U,w300" --trace walk.csv --shot room.png
```

See `src/main.rs` for the options: the plan (refemu-pcjr's items), the trace (its
columns), the screenshot (`--signal` for the signal's colours), `--profile`, and the
debug switches `REFEMU_TIMELINE` and `REFEMU_WORK`.
