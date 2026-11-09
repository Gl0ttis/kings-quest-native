# The engine (IBM PC, 1984, CGA)

The PC engine is the PCjr engine's design, built differently: the same routines, laid
out at other addresses, with the data segment's variables `$606` bytes further on
(ports/pcjr/docs/engine.md describes the design). The port's engine will be written as
the PCjr port's was: **a native reimplementation from the engine's documented
behaviour, never a transcription of its instructions.** Each routine's behaviour is
established, written down here or in the PCjr's engine.md, and implemented in Rust in
the port's own terms. The PC engine's addresses below are for citing what each part
reimplements, and for the measuring tool, which counts checkpoints in the original's
code.

Where the PC behaves differently from the PCjr (its screen, its sound, the scripts'
revision), the difference is found by comparing the two, pass by pass, in refemu-pc,
and described here before it is implemented.

## Finding the PC's addresses

`tools/addrmap.py` maps the PCjr engine's addresses to the PC's. It disassembles both
code segments (from `kqextract-pcjr` and `kqextract-pc`), reduces each instruction to its
shape (addresses, displacements and jump targets left out), and finds where the run of
shapes starting at a PCjr address starts in the PC engine, if it does exactly once.
Addresses with no unique run are looked for near where their neighbours went. Of the
207 code addresses the PCjr port cites, 162 map exactly and 12 by their neighbours.
Most of the rest are the PCjr's hardware: its screen position (`$0554`), joystick
(`$0563`), video pages (`$064C`, `$0679`), sound chip (`$0844`) and picture pages.

## The checkpoints

The port's clock (ports/pcjr/src/engine/clock.rs) replays the timer's ticks against a
count of checkpoints passed in the original's code: routine entries and loop heads. The
PC's:

| PCjr | PC | Routine |
|---|---|---|
| `$0B92` | `$12A0` | poll_key_bios: INT 16h, bypassing the ring |
| `$0DC1` | `$2209` | next_parse |
| `$1069` | `$24B1` | read_line |
| `$13CE` | `$2816` | wait_key: one game cycle per loop |
| `$14F2` | `$293A` | game_cycle: one pass |
| `$1665` | `$2AAD` | update_actor |
| `$1A6C` | `$2EB4` | key_ring_pop |
| `$1AAC` | `$2EF4` | run_action |
| `$1ABB` | `$2F03` | run_action: each opcode |
| `$2C3C` | `$3FB7` | countdowns and the draw pass |
| `$34F5` | `$48E3` | draw_room_picture (by hand: its frame is DS:`$1FCF`) |
| `$38B8` | `$4CA3` | save_restore_dialog |
| `$3ABC` | `$078E` | save_slot |
| `$3B99` | `$0867` | load_slot (by hand: slots at sectors `$3F + 3n`) |
| `$3C0A` | `$08D4` | key_poll (INT 16h AH=1) |
| `$3C15` | `$08DF` | key_wait (INT 16h AH=0; by hand, after key_poll) |
| `$3EC2` | `$0785` | wait_enter (by hand) |
| `$47E1` | `$1143` | match_parser_rules |
| `$47EE` | `$1150` | match_cycle_rules |
| `$47F8` | `$115A` | the matcher: each rule |
| `$4A7A` | `$5018` | title_wait |
| `$4D93` | `$1FB6` | read_sectors |

The PC engine also has a wait of its own the PCjr's hasn't: each pass waits once
(`$3FE7`) for the timer to clear a flag (`SS:$1E65`), so a pass is a whole number of
ticks (refemu/README.md, "Timing"). Like the PCjr's busy waits it is not a checkpoint:
its turns depend on the timing.

## Data addresses

`tools/addrmap.py --data` maps the PCjr's data-segment addresses too: wherever a run of
code is found once in each engine, the two instructions' memory operands are paired.
The offset isn't the same throughout. Examples:

| PCjr | PC | |
|---|---|---|
| DS:`$0CD6`, `$0E98` | DS:`$12DA`, `$149C` | `+$604` |
| DS:`$1003` (the variables) | DS:`$1609` | `+$606` |
| DS:`$11E1` (the actors, 30 bytes each) | DS:`$17E7` | `+$606` |
| DS:`$125A` (game over), `$1262`, `$1279` | DS:`$1860`, `$1868`, `$187F` | `+$606` |
| DS:`$174F`, `$1758` (loops by direction) | DS:`$1D55`, `$1D5E` | `+$606` |
| DS:`$1828` (the picture's row table) | DS:`$1E76` | `+$64E` |
| DS:`$1E91` (picture flag) | DS:`$10CD` | moved |

**The picture buffer** is one, at segment `1400h`, in the PCjr pages' layout (row y at
`(y & 3) * 0x2000 + (y >> 2) * 160`): each byte holds the visual picture's dot pattern
(composite.rs, `PATTERN`) in its high nibble and the priority in its low. The PCjr keeps
the two in pages of their own (`1800:0` and `1000:0`).

## Variables

The scripts are a revision whose variables are mostly renumbered (disk-layout.md).
`tools/varmap.py` gives the PC's number for a PCjr variable, from the pairs of bytes
that differ where each room's two scripts align. The ones the routes use:

| PCjr | PC | PCjr | PC |
|---|---|---|---|
| 1-48 | unchanged | 133 | 136 |
| 51, 53, 55 | 53, 55, 57 | 147 | 49 |
| 101 | 103 | 149 | 151 |
| 107 | 109 | 165, 178, 184, 186 | 167, 180, 186, 188 |
| 110 | 112 | 210, 212 | 213, 215 |

Mostly 2 more from 49 and 3 more from about 126, with some moved (147 to 49, 120 to 50,
152 to 124). 212 has too few pairs for the tool's rule on its own (10 of 15 for 215);
its neighbour 210's 213 settles it.

## How the PC differs (found pass by pass)

Each found by replaying a reference route in the port and comparing every pass with the
original's (tests/engine.rs), then implemented in the port.

- **Start-up:** before its start routine (`$2094`), the engine reads its resource
  table, 2 sectors from sector 10 (`$5A`). The PCjr's is part of its engine.
- **v5, the colour under the ego** on priority-0 ground: the PC's picture buffer holds
  each colour's dot pattern (composite.rs, `PATTERN`), so v5 is the pattern (light
  green, 10, is 11).
- **The step's direction table** is at CS:`$430A`, before the step routine (`$431C`);
  read at the PCjr's address it is code, and direction 0 steps.
- **The vocabulary** is stored plainly at DS:`$0004`, up to its end marker `{`: the
  PCjr's, decoded (that one is rotated a bit and decoded at start), 3,259 bytes.
- **Making a Play diskette** (`$0488`): the y/n is read from the BIOS (`$04B3`); each
  "insert your ... diskette" waits through the Enter wait (`$0785`, a checkpoint), where
  the PCjr reads the BIOS. The copy is the data area, sectors 141-719, in three passes
  of 65 + 128 sectors (`$04DE`, `$0552`), the second part read into segment 1000h, over
  the picture buffer. The check that the Master is in the drive (`$05E8`) compares its
  sectors 10-11 with the resource table at CS:0088, all 1,024 bytes.
- **The save slots** are at sector 3Fh + 3n, the initial state (slot FF, taken as 1Ah)
  at 8Dh, the data area's first sector (`$07DA`). Before a real slot, sector 0 is
  written with DS:0000-01FF, whose first word is the stack pointer the start-up keeps
  there (D8F0h). The block is the PCjr's layout at the PC's addresses (1,306 bytes from
  DS:10D2, 1609, 1967 and 17E7), and the 3 sectors' rest is whatever followed it in
  memory, never read back. The screen position saved (DS:1881) is the game's own
  (`$19D3`), not the BIOS's.
- **The joystick** (`$1383`, every other tick, `$141B` the read): with none, v16 is
  cleared unless v31 is 1, and nothing else changes (the PCjr sets a time-out flag,
  here DS:187D, which stays 0).
- **The screen** compared is the CGA's picture area in its 640-dot mode: each picture
  pixel its colour's dot pattern, two to a byte (`World::frame`, refemu-pc's
  `Runner::frame`).
- **An actor's rectangle copied to the screen** (`$1875`) starts at an even pixel (an
  odd left edge one back, the width 2 wider), and isn't clipped: past the right edge it
  reads on into the buffer, row y + 4 (its layout's next 160 bytes), and writes on into
  the screen, line y + 2 (the CGA's next 80 bytes). A cel drawn past the right edge
  lands in the buffer the same way, at the start of row y + 4.
- **Op 0A's record** before a message, the cursor to the start of row 24, is at
  DS:1E26.
- **The title's lines** are in the engine's code after the calls that print them
  (`$4EA7`): "(c)Copyright SIERRA On-Line Inc 1984" (CS:4EAD; the PCjr's adds IBM's),
  "Press any key to play" (CS:4F06), and the credits' records from CS:4F36 (colour,
  column, text), all in colour 0Fh XORed (a colour of 10h or more is DS:1167's, 0Fh).
- **Moving the screen:** Ctrl+Left and Ctrl+Right (scans 73h, 74h) move the picture a
  character right or left (`$1480`: the screen position DS:1881, the CRTC's register 2,
  within 21h-2Fh), during play (`$1C05`; any other scan past the keypad ends the key
  read) and at the text screen's key wait (`$0B0F`), which has no colour key (the
  PCjr's takes `c` and two hex digits). Code 8Bh (Ctrl+V) does nothing (the PCjr's turns
  a debug mode on).
- **The text screen** (`$0A3E`) is BIOS mode 1, not cleared, page 7, at the text
  screen's position (DS:1132); back to graphics (`$194C`) the picture is copied back and
  the position is the graphics screen's (DS:112F). Its clear writes to 1F80:0000, where
  the PCjr's page 7 is: on the PC that is ordinary memory. The screen position is the
  game's own throughout (DS:1881); the PCjr's comes from its BIOS (0040:0089).
