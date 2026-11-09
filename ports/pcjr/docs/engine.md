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
  the queued messages (every v24 + 1 ticks); the joystick; the keys, the first typed
  character into an 80-key ring.

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

## The timer in the port

See [src/engine/clock.rs](../src/engine/clock.rs). The port passes the same
**checkpoints** as the original (the routine entries and loop heads in
`clock::CHECKPOINTS`), and fires the timer's ticks, the key presses and the disk swaps
that refemu-pcjr logged (`--events`) in the same intervals: a tick that interrupted a
disk read at the read; code reading the keyboard after the keys pressed before it; a
busy wait (on v23, on v15) ticks one at a time until it is over; the rest at the next
checkpoint. The replay example ([examples/replay.rs](../examples/replay.rs)) compares
the checkpoint sequences and the per-pass state.

## Routine index

| Original | Port |
|---|---|
| `$0808` timer, `$0A9D` printing, `$08CE` keys | `timer.rs`: `int1c`, `print_chunk`, `key_tick` |
| `$0A30` restart | `save.rs`: `restart` |
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
