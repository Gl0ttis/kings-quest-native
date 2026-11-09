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
