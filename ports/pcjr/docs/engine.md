# The PCjr engine and the port's reimplementation

The original engine is a C program with hand-written assembly (the rule matcher, the
loaders, the drawing), in segment 0060h with its data segment at 0543h (DS = CS + 4E3h,
so DS:0 is CS:4E30). The port, in [src/engine/](../src/engine/mod.rs), reimplements it
routine by routine with native state, as the Apple port does its 6502 engine. Each
function's comment names the routine (`/// $14F2: ...`).

## How the original runs

- **Start** (`$0C5A`): mode 9, the vocabulary decoded (`$0C1F`), the timer handler
  installed, the title (`$48E8`, first time only), the global script (room 0) loaded,
  and its cycle rules run once, which go to room 1.
- **The main loop** (`$0D16`): the parser yields the next (verb, noun) to try (`$0DC1`),
  reading a line when it has none; a game cycle; the global parser rules (again while
  they ask), the save or restore dialog if asked for, the room's parser rules.
- **Reading a line** (`$1069`) calls the input wait (`$13CE`), which runs **game
  cycles** (`$14F2`) until there is a key in the ring, the prompt to draw, or a canned
  line. So most passes happen there, while the player types. The timers v2 and v9-v12
  count down once a loop, after the cycle.
- **A game cycle** (`$14F2`): the ego's direction from the keypad; each actor's update
  (`$1665`: motion, loop, barriers, animation; the others in slot order, then the ego);
  the countdowns and the redraw (`$2C3C`); the cycle rules, the global script's (again
  while they ask) then the room's; v17 cleared.
- **The timer** (INT 1Ch, `$0808`, 18.2 times a second): the music; the next word of
  the queued messages (every v24 + 1 ticks); the joystick, when no key waits (`$0563`:
  its direction for the next cycle, when the keypad gives none; button 1 holds v16 as
  the `-` key toggles it, button 2 is the `0` key's jump, v17; a clock for play
  supplies the stick, `Clock::joystick`); the keys, the first typed character into an
  80-key ring.

## Differences from the Apple version

Found reading the code; each is reproduced.

- **Messages print from the timer**, a word at a time (op 0A only queues them, and sets
  v23 while they print). Opcodes that load from the disk wait for the printing first.
  A message has no "more" pause.
- **The parser is an iterator** over the (verb, noun) pairs of a line, one game cycle
  per pair tried, until a rule matches (`FA` is the wildcard, for the verb or the
  noun). v28 says how the last line went (1 no verb, 2 no rule, 3 a rule).
- **Opcodes:** 05 takes the room whose picture to load, and doesn't reset the ego; 0F
  plays a sound; 25 overlays room n's picture; 2C loads a group of four sounds; 31 has
  no operands and shows a "time penalty" line from v20; 33 and 32 are carried out by
  the main loop; 38's new loop takes effect at the actor's next update; 3C's operands
  are the text colours; 40 loads another room's script as a third, unused, script; 43
  is "game over". 1C and 41 differ (only 41 refits the ego). Several opcodes don't
  clamp or clear what the Apple's do (18, 19, 1E, 20-23, 28).
- **Motion:** the chase is not random when stuck but a detour state machine shared by
  every actor (`$2648`, `$2958`); once any chaser is stopped by a barrier, chasers
  follow the barrier while it stands; the chase window is (−slack, +slack], the
  move-to's [−step, +step]; the slack isn't reset between calls. Two actors other than
  the ego block each other; the strike reaches further. The wanderer's and the room
  change's random numbers are bytes of the engine's own code.
- **Moving and drawing:** the edges are 28h ≤ y < A6h, 1 < x, right end < 9Eh; an edge
  undoes the step and is still reported if the picture then blocks. For the ego, v4 is
  set when every pixel under its feet has priority 0, and v5 is then the colour under
  its right end. View 6Bh is blocked by priority-1 pixels. The draw order's buffer
  holds three pairs the sort never refills, which make the very first sort put slot 0
  four times in front. A permanent cel's look-down for priority 15 drifts a column left
  each row.
- **Room change:** the ego comes in at x 157 / 2 or y 165 / 42 on the edge it left by;
  v14 is set from the parity of a code byte; the actors' positions are written to the
  object numbered by object 0's x (a bug); the prompt is printed.
- **Restart** restores an initial state saved on the Master (sectors 27Fh-281h), then
  clears most of it again: what stays is the object table.
- **Saves** go to a Play diskette, which the game makes itself (op 42): it formats a
  blank diskette and copies the Master's sectors 63-641 to it in three passes, through
  memory (part of each pass through the video pages); then it saves to slot 0,
  restarts, and restores it. Before saving to a slot, it writes the start of its data
  segment over the diskette's boot sector. A save holds the variables, the objects, and
  the actor table with the globals after it, as they are in memory, pointers included.
  The original's restore takes the pointers as they are: the room's script, run again,
  loads its views where they were. So the port keeps the layout of the original's
  resource memory (from DS:1F62 up: a room script or sound group goes at the top and the
  top moves past its sections, `$4D06`; a view likewise, `$4BC2`; clearing the actors
  goes back to the room's script, `$472D`) and writes the same pointers. Its save
  blocks are the original's byte for byte, and the original restores them as its own
  (test `the_play_diskette_is_the_originals`, routes/restore_play.plan; and in 86Box,
  with IBM's BIOS: the port's slot A save, put on a copy of the cracked disk in the
  slot's sectors 282h-284h, which it leaves empty, restored by the original there,
  Graham back where he was saved and the game going on).

## The timer in the port

See [src/engine/clock.rs](../src/engine/clock.rs). The port passes the same
**checkpoints** as the original (the routine entries and loop heads in
`clock::CHECKPOINTS`), and fires the timer's ticks, the key presses and the disk swaps
that refemu-pcjr logged (`--events`) in the same intervals: a tick that interrupted a
disk read at the read; one that interrupted an action writing a variable before the
write (`set 25` as a room starts turns the timer's key reading on); one at the game
cycle's start before the keypad direction is read (`$14F2`-`$14FE`); one that
interrupted a message being queued (`$0A6F`-`$0A85`, or the print op before its first)
before it goes in, since the timer's print countdown runs only while one waits; one in
wait_key's timer countdown
(`$1445`-`$14A0`) before v23 is tested (the last message printed lets the loop return
to the line editor); code reading the keyboard after the keys pressed before it; a
busy wait (on v23, on v15) ticks one at a time until it is over; the rest at the next
checkpoint. The replay example ([examples/replay.rs](../examples/replay.rs)) compares
the checkpoint sequences and the per-pass state; with `KQ_PROBE=FROM-TO` the port
prints the ego and the keys at each pass, and `kqprobe-pcjr` (refemu) prints the same
from the original replaying the same log. `KQ_CPS=FROM-TO` prints the checkpoints
numbered FROM-TO as they pass, in both (the first that differs is where to look).
`kqprobe-pcjr --follow` follows the log as it was recorded, typing a prompt's keys
where they were typed; without it, the keys go in at their checkpoints, which can
type them earlier and move the message printing by a tick.

## Speed

See [src/engine/cost.rs](../src/engine/cost.rs). The original has no speed control: a
pass takes as long as the 8088 needs. The port counts the work the original's time goes
on and turns it into cycles with fitted constants, added to `cycles` at every
checkpoint; the paced clock fires the timer's ticks from them, so the game runs at the
original's speed and its ticks fall where they did.

**What is counted**, found by profiling the original pass by pass (`kqprofile-pcjr`
in refemu: cycles by code address; two passes with the same counts but different
lengths show what was missing):
- the rules tested and the actors updated (their checkpoints, `$47F8` and `$1665`);
- the actors' drawing: background bytes saved and put back, cel pixels written, bytes
  copied to the displayed page, rows looked down for a priority under 15, and the
  packed cel's nibbles, since every draw unpacks its cel again (`$3418`). The cels'
  packed sizes differ from frame to frame; without them the error was three times
  larger;
- the characters the BIOS draws and the text rows it scrolls;
- the timer handler: each tick, each joystick read (none is connected in refemu, so
  the read times out after 41 turns of its loop; with a stick, the turns until both
  axes settle), each byte sent to the sound chip;
- loading: sectors read; for a room picture, the pages cleared before it, the line
  steps (the absolute line's by error terms; the others' by a multiply and a divide
  each), the lines, the fills' bytes, scans, runs and seeds, and after it the colours
  merged into the work page (`$064C`, about 1.9M cycles, the finished picture on screen
  meanwhile).

Busy waits for the timer (the messages printing, `$462F`) aren't work: the clock takes
them to the tick that ends them.

**The fit** (`tools/costfit.py`, on `examples/costdump.rs`'s counts against the
reference traces) uses the max-score and magic-ring routes. Every constant is kept
`>= 0`, since time doesn't run backwards. Measured on the gold-key and giant-sling
routes, which the fit didn't see (`examples/speedcmp.rs`, the model running in the
engine):

| Passes | Median error | 90% within | Total |
|---|---|---|---|
| Ordinary | 1.5–1.6% | 3.6–3.9% | −0.27% to −0.01% |
| Loading (disk, picture) | 0.79–0.81% | 1.8% | −0.17% to −0.08% |
| Busy waits (the clock's) | 3.3–3.9% | 8.7–8.8% | +3.3% to +3.7% |
| The whole run | | | +0.11% (giant sling, 24 min), +0.19% (gold key, 26 min) |

**The drawing** of a room picture is timed on its own, since a front end shows it as it
goes: refemu-pcjr's `kqplotlog-pcjr` logs every byte the original writes to the
displayed page while it draws, by cycle, and `examples/drawcmp.rs` sets the port's
drawing beside it, mark by mark; `tools/drawfit.py` fits the clear before the first
byte, the per-count costs of the lines and fills, and the end, over the max-score run's
146 pictures (and `tools/costfit.py` then fits only the sectors and the merge on the
loading passes). On the giant-sling run's 150, which the fit didn't see, each picture's
drawing takes within 0.7% (median) and 2.5% (90%) of the original's time, and part way
through the share drawn is within 0.3 and 1.1 points of the original's. (With one fixed
cost per picture, fitted only on whole passes, it was 22% too long, and 9 points off
part way.)

**Start-up** is 4.0 s by the model to the first pass, 4.5 s in refemu. Most of the
difference is the title screen's wait, which in refemu is its own automatic RETURN
after an idle stretch; in play it is the player's.

**The reference is refemu's timing**, which is within about 10% of 86Box's (refemu's
README, "Timing").

## Routine index

| Original | Port |
|---|---|
| `$0808` timer, `$0A9D` printing, `$08CE` keys | `timer.rs`: `int1c`, `print_chunk`, `key_tick` |
| `$0A30` restart, `$38B8`-`$3EC2` save, restore, Play diskette | `save.rs` |
| `$3ED9`-`$41CC` the text screen, inventory | `screens.rs` |
| `$0C5A` start, `$0D16` main loop, `$48E8` title | `input.rs`: `game_start`, `main_loop`, `title` |
| `$0DC1` parser, `$0F46`, `$0FF6`, `$0BA6`, `$0BFC` | `input.rs`: `next_parse`, `find_word_of_type`, `read_word`, `vocab_find`, `vocab_match_second` |
| `$1069` line editor, `$13CE` input wait | `input.rs`: `read_line`, `wait_key` |
| `$14F2` game cycle, `$1665` actor update | `actor.rs`: `game_cycle`, `update_actor` |
| `$1AAC` actions, `$47E1`/`$47EE` matcher | `script.rs`: `run_action`, `match_parser_rules`, `match_cycle_rules` |
| `$24BE` motion | `motion.rs` |
| `$2BEF`-`$3477` actors on screen | `sprite.rs` |
| `$34F5`, `$353F` pictures | `screen.rs`, and the painter in `pic.rs` |
| `$4238`-`$47CB` actor and room actions | `actor.rs` |
| `$4AFC`-`$4E0B` loaders | `actor.rs` (`load_*`), `res.rs` |
