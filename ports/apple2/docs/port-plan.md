# Port plan: native King's Quest (Apple II, 1984)

## Goal and constraints

The goal is a native Windows x64 program, written in Rust, that plays the 1984
Apple II King's Quest as the original does.

- **No emulation.** No 6502 emulation and no instruction-by-instruction
  translation. Each engine subsystem is reimplemented from its documented
  behaviour.
- **The original disks supply the game.** The port ships an engine only. At run
  time it reads the user's own disk images (`.nib`, or the decoded
  `.do`/`.po`). It never embeds or redistributes pictures, sprites, scripts or
  text.
- **The game logic runs unchanged.** The 81 original scripts drive the game
  through a native interpreter. No game logic is rewritten by hand.
- **Faithful by default.** Byte-level quirks the scripts depend on are
  reproduced. Where the original behaviour is undefined or depends on hardware
  timing, the choice is documented and made deliberately (see
  [Decisions](#decisions)).

## Where we are

The engine is complete enough to play the whole game. A maximum-score playthrough
(158 points, 17,332 passes from power-on to the coronation) runs identically in the
port and in the original, pass by pass (see [route.md](route.md) and M6 below).

All the static content is decoded, checked, and has a Rust reader:

| Area | Status | Code | Docs |
|---|---|---|---|
| Disk decode (6-and-2 GCR, checksums) | Done; all 2,240 sectors valid | `nib.rs`, `nibdecode` | README |
| Resource layout and directories | Done; 240 resources, no overlaps | `kq.rs`, `kqextract` | [disk-layout.md](disk-layout.md) |
| Pictures | Done; 83 render correctly | `pic.rs`, `kqpic` | [picture-format.md](picture-format.md) |
| Views (sprites) | Done; 69 views / 533 cels render correctly | `view.rs`, `kqview` | [view-format.md](view-format.md) |
| Script structure and opcodes | Done; 81 scripts, 3,316 rules and 2,793 actions parse; 67 of 68 opcodes named | `script.rs`, `kqscript` | [script-format.md](script-format.md) |
| Vocabulary | Done; 345 entries | `vocab.rs` | [script-format.md](script-format.md) |
| Movement and animation | Done | `engine/cycle.rs` | [motion.md](motion.md) |
| Script VM, text, parser, save | Done | `engine/` | [text-and-parser.md](text-and-parser.md), [save-format.md](save-format.md) |

The only third-party dependency is `minifb`, for the window in `kqplay`.

## What is still unknown

The unknowns that blocked M2–M4 (the room entry sequence, the parser, text output,
opcode `$2B`) are resolved and implemented. What remains is detail. Everything the
traced runs exercise matches the original, so none of these is known to cause a
difference.

| Unknown | Where to look |
|---|---|
| Collision rules for priorities 1 and 15: which flag combinations enable water and the conditional barriers, beyond those the scripts use | aux `$12C7`–`$1373` |
| The erase and redraw order for overlapping actors in every case | engine `$4DD7`–`$4E69` |
| Cycle modes 6 and 7, and how `$73` (from `ego_special_view`) is used | `$6117` table |
| The purpose of `v31` | text and parser code |

**Sound.** The only sound is the Apple ROM bell (`$FBDD`, called at `$6618`): single
beeps for the "more" pause, and a double beep (`$6625`) for save and restore disk errors.
`src/sound.rs` rebuilds it from the ROM routine's cycle counts (0.0114 s of silence,
then 192 speaker toggles 546 cycles apart: a 935 Hz square wave for 0.1 s), and
`kqplay` plays it through Windows and takes the original's time for it.

## Architecture

All of this stays one crate (`kq1-apple2`, in `ports/apple2` of the workspace), with a library and a few binaries. The
library keeps the engine's logic separate from the platform, so it can run
headless in tests.

```
disk images ──► kq (resources) ──► pic / view / script / vocab   (done)
                                         │
                       ┌─────────────────┴─────────────────┐
                       ▼                                   ▼
                 world state                         script VM
       (vars, objects $1A80, actors,          (byte-level rule matcher,
        picture buffer, view slots)            action interpreter, 68 ops)
                       │                                   │
                       └──────────► game cycle ◄───────────┘
                         (input → actors → timers → rules)
                                         │
                  ┌──────────────────────┼──────────────────────┐
                  ▼                      ▼                      ▼
             renderer               text / parser            save / load
       (140×160 buffer → RGB,       (input line,           (files; disk 4
        double-wide pixels)          messages)              optional)
                  │                      │
                  └──────── platform (window, keyboard, timer) ────────┘
```

**Design rules:**

- **The world state is plain data.** A struct holds the 256 variables, the
  150 object records, the actor records, the picture buffer and the loaded
  views. This makes a save just a copy of that data, and lets the tests check
  it.
- **The script VM works on the raw script bytes, not on a parsed tree.** The
  rule matcher's skip-by-scanning behaviour depends on the byte layout. The
  existing parser stays as a disassembler and validator only.
- **Opcodes are matched exhaustively.** Each opcode is one function, named
  from `op_name` and checked against `OPERANDS`. Adding behaviour is a
  per-opcode task, and an unimplemented opcode fails loudly in debug builds.
- **The engine is deterministic.** Given the disk images, a starting state and
  a list of key presses per cycle, a run always produces the same states. This
  is the basis for testing (below).
- **The platform layer is thin and replaceable.** It provides a window, a
  keyboard and a frame clock. I'd use one small crate for the window and frame
  buffer, such as `minifb` or `pixels` + `winit`. Everything else stays
  dependency-free.

## Milestones

Each milestone ends with something you can see or run, and with tests.

### M1: Room viewer (done 2026-11-07)

The engine core is in [src/engine](../src/engine). `kqroom` enters each room the way the game does: start at `v1 = 0`, let the global script call `new_room 1`, then leave room 1 through `clear_actors` and `new_room`. It runs the global and room cycle rules until the room script sets `v25`, then composes the screen. All 80 rooms render with their scripted objects, and the engine logs no warnings. Movement, input and text are not in it yet.

M1 needed most of M2's core, because rooms are set up entirely by scripts and the engine draws nothing itself. This version already has the byte-level rule matcher, all non-UI opcodes, actor binding, drawing with the spiral nudge, add-to-picture, collision and draw order.

The original plan for M1 was:

Load any room and draw it the way the engine does: the room's picture, then
each object stored in that room, drawn at its stored position with its view,
loop and cel, using priority masking. This proves the picture and sprite code
work together.

- **Done when:** a `kqroom` tool renders all 80 rooms with their objects, and
  spot-checks against expected scenes pass.

### M2: Script VM, running without a screen (done with M1 and M3)

The VM, all opcodes, the room entry sequence and per-pass state traces were built as part of M1 and M3. Every room is visited with no unimplemented opcode or matcher error, and the traces are compared against the original in M6.

The original plan for M2 was:

Build the world state, the byte-level rule matcher, and all 68 opcodes. Opcodes
that touch the screen or text are routed to a recording sink for now.

- Implement the room entry sequence.
- Run the global and room cycle rules each cycle.
- **Done when:** starting at the opening room, a scripted series of
  `new_room` calls visits every room without an unimplemented opcode or a
  matcher error. A trace of variables and objects per cycle can be dumped and
  diffed.

### M3: Moving around (core done 2026-11-07)

[src/engine/cycle.rs](../src/engine/cycle.rs) implements the game cycle from [motion.md](motion.md):
- the direction keys
- the per-actor delay counters and the draw pass
- loop choice
- barriers
- all cycle modes and motion modes (chase, approach, follow with patrol, wander, move-to)
- the exact draw-order sort

`kqplay` runs it in a window at a fixed cycle rate, adjustable with `+`/`-`, and prints messages to the console. These were checked by headless runs:
- walking between rooms 1 → 16 → 9, through the horizon and right-edge exits, with the entry positions from the edge table
- obstacles stopping Graham: a tree base in his own priority band, and priority-15 lines
- walking into room 1's moat, which plays the scripted alligator death and the global "Thank you for playing" ending
- the gnome in room 40 wandering on his own

The RNG carry model, the original's pacing and the `$4165` re-fit, once open here, were done in M6. Still open in M3:
- **The joystick** is supported (2026-11-08): see [motion.md](motion.md#the-joystick-4817-4846).

The original plan for M3 was:

Build the game cycle from [motion.md](motion.md):

- keyboard directions
- delay counters
- loop selection
- step and clamp
- collision with the priority buffer, including the rules for priorities 1
  and 15
- barriers
- cycle modes and motion modes
- the RNG from the engine bytes, with the carry behaviour at each call site
- edge reporting into `v3`, which the room scripts use to change rooms

**Done when:** you can walk the ego between rooms in a window, chasers and
wanderers behave as described, and death sequences triggered by scripts play.

### M4: Text and the parser (core done 2026-11-07)

[src/engine/text.rs](../src/engine/text.rs) and [src/engine/parser.rs](../src/engine/parser.rs) cover:
- the original font and text rows 20–23, with scrolling
- the `> ` prompt and line editing, with the original keys
- message printing with the "more" pause and beep
- the exact parser over the original vocabulary, and the `v28` fallback replies
- the inventory and score screen
- the restart prompt
- `input_number`

`kqplay` takes typed commands. In headless tests, "look at the castle" and "look banners" print the room's descriptions, "castle" alone gets "I do not understand that.", and an unknown word gets `I don't understand "xyzzy".` In room 6, walking to the stump and typing "get pouch" gives 3 points and the pouch shows up in the inventory.

See [text-and-parser.md](text-and-parser.md). Still open: the purpose of `v31`. The exact Apple text-mode characters need a user-supplied ROM dump (found automatically next to the disks).

The original plan for M4 was:

- the input line and the parser: words, phrases, ignored words, verb and noun
  ids, and "I don't understand"
- message windows using the original font from `aux_1D00_font`
- the status line, score and inventory
- the text-screen opcodes (`$3C`–`$3F`)

**Done when:** the game can be played by typing commands. All 345 vocabulary
entries resolve, and every message in the scripts can be displayed.

### M5: Save, restore, restart (done 2026-11-08)

Saves are files with the original's data blocks in the original layout. The original letter prompts and messages are used, and restoring re-enters the room the way the original does. Restart works through the original "start over?" prompt. Three integration tests cover a save and restore round trip, an empty slot, and restart. See [save-format.md](save-format.md). Reading and writing real disk-4 save images is still optional and not done; the shared layout makes it a format adapter.

The original plan for M5 was:

Save the world state to a file, with 26 slots (a–z) to match the original
letters, and restore it. Restart re-runs the opening sequence. Reading and
writing original disk-4 save images is optional, and depends on the save
format work.

- **Done when:** saving and restoring at any point gives an identical state
  trace afterwards.

### M6: Fidelity pass (in progress)

[refemu/](../refemu/README.md) is a reference Apple IIe built only as a measuring tool, with no Apple ROM. It boots the original from the `.nib` files and records cycles per main-loop pass and per-pass state traces. Its first results:
- The original's speed varies by room, from about 7.2 to 11.5 passes per second.
- Movement matches the port pass for pass.
- Pass time is mostly screen updates, cel drawing and rule matching.

**Done in M6 so far:**

- **Original speed.** [src/engine/cost.rs](../src/engine/cost.rs) gives each pass the original's duration, from the work the engine counts:
  - rule bytes scanned
  - sprite data nibbles decoded
  - screen pixels updated
  - characters drawn
  - sectors read
  - picture pixels plotted
  - room changes

  The costs per event were refitted on 2026-11-08 over the whole maximum-score run, port counts against the original's measured cycles. Over 17,292 passes (leaving out the passes where the original waits for a key), the median error is 1.1% and 90% of passes are within 3.2%. The heaviest 10% of passes (busy scenes like the condor and the leprechauns) have a 0.7% median error, and the whole 3,615-second run comes out 0.4% short. `kqplay` paces passes this way by default; `--rate N` gives a fixed rate, and `--fast-loads` drops the loading time. With this, a room change takes about 13 seconds, as on a real Apple IIe. The test `speed_model_matches_original` keeps it there.

  Before the refit, the port's counts were brought in line with the original's, pass by pass:
  - the prompt clears its line by drawing 40 spaces, and is drawn at the start of the next pass
  - each carriage return draws one more character, and a scroll redraws the bottom row
  - the update rectangle covers the old cel as well as the new one, in both width and height
  - erasing an object (for example when Graham takes it) redraws the other sprites

  What's left: disk reads vary with the head position and rotation, which isn't modelled (a median of 3.9% on loading passes), and the original's swap prompts ("Please insert side B") have no counterpart in the port.
- **Random numbers.** The carry is modelled at every call site, and this is confirmed against the original (see [motion.md](motion.md#random-numbers-65f1-confirmed)).
- **Start-up.** The title sequence (`$432F`: the Sierra logo, then the title and credits, each shown for about 3.8 s or until a key is pressed) and the initialisation (`$45A1`) are implemented. Initialisation runs the global rules once before the first pass, so room 1 starts with the pass counter at 0, as in the original. Restart re-runs the initialisation and abandons the rest of the pass.
- **The ego re-fit (`$4165`)** is implemented (see [motion.md](motion.md)).
- **Picture fill.** The fill is now a literal transcription, including a quirk in its leftward scan. The original's whole picture buffer matches the port's byte for byte in rooms 1 and 16, and the screen colours match on every dot.
- **Trace comparison.** For 300 passes, from start-up through two room changes, the port and the original agree on room, ego position, direction and all 223 variables on every pass. One bug was found and fixed along the way: `draw` must redraw every sprite at once (`$4E42`), which updates `v7`. [tests/reference_trace.rs](../tests/reference_trace.rs) checks this against traces recorded from the original, and a second case covers falling into the moat (85 passes).

- **Maximum-score playthrough.** [routes/max_score.route](../routes/max_score.route), derived from the room scripts and played by the `kqroute` autopilot, scores 158 of 158. Its key log replayed in the original gives the same trace on all 17,332 passes (`max_score_playthrough`). Following it found four engine fixes: per-actor background save and restore, the self-modified `JSR` operands that the RNG reads, the `$F2` width that sets the condor's turning point, and arrival clobbering the motion registers. See [route.md](route.md).
- **The parser's self-modified operand (`$523F`)** is tracked, and its value matches the original after each typed line ([motion.md](motion.md)).
- **Death and restart.** Dying in the moat, pressing 5 and answering Y, then walking through rooms 16 and 9 matches the original for 375 passes (`restart_after_death`). This found one bug: a restart chosen from a typed command went on to set `v28` and `v13`, which the original never does, because its restart resets the stack.

- **Script 064 is closed.** Room 64, the only room whose script has the defect, can't be reached in normal play (see [script-format.md](script-format.md#known-data-defect-script-064)). The corrected boundary stays, and no compatibility switch is needed.
- **The text window on room entry.** `show_overlay_picture` clears the text rows and starts the prompt on row 20, as in the original. The port had kept the old text on screen.
- **refemu fixes.** Its automatic RETURN no longer fires during long room loads, or twice for one pause. Those extra RETURNs had been typed into the original as empty lines.

- **Human playtest (2026-11-08).** The whole max-score replay was watched in `kqplay` at the original's speed, start to coronation, with no text, sprite or colour problems seen. Findings and what came of them:
  - *The game froze while the window was dragged.* Windows stops a window's thread during a drag or resize. The engine now runs on its own thread. A hook in front of minifb's window procedure does all the painting, and a timer keeps it painting during a drag or resize. Verified by holding the title bar: room 1 kept drawing and Graham appeared.
  - *Full screen.* Alt+Enter (or `--fullscreen`) toggles a borderless full-screen window, with the picture kept in shape between black bars. The first version drew the picture at window size in a corner. minifb missed the size change because `--fullscreen` resized the window before minifb's first update, and its stretch was wrong under display scaling anyway. Alt+Enter also typed a RETURN, because minifb turns Alt+key into characters. The hook now catches Alt+Enter itself and drops every Alt+key character. kqplay is per-monitor DPI aware, so sizes are in real pixels (on a 4K monitor at 150%, the picture is 3150×2160, sharp), and the window starts at `--scale` (default 2) times the frame, times the display scale.
  - *Load pauses showed the finished room.* The original plots pictures straight to the screen, so during a room change it shows the old room while the disk turns, then the screen goes white and the new picture appears line by line and fill by fill; the sprites come at the end. refemu screenshots of the change from room 56 to 57 at 1.0M, 2.7M, 5.0M and 10.3M cycles into the pass show exactly that. The port showed each pass's frame at the start of its time, so a load showed the finished room for 10 s. Now each pass's frame is shown when its time is up, and a pass that draws a picture replays the drawing: the engine records each draw (`World::pic_draws`: the work done before it, the buffer drawn onto, and the plot count), and `pic::draw_partial` gives the buffer after any number of plots. Text rows stay blank meanwhile, as in the original. At matching points the port's picture is within a few fills of the original's, since lines and fills share one plot cost. The title pictures are not replayed yet. With fast loads the drawing is instant.
  - *Slowdown with several moving sprites (the condor, the goat).* This is the original: Graham moves a fixed step per pass, and passes take longer as more cels are decoded and copied. In the original's trace, passes with little sprite work run at 11.3 per second, with 300+ cel nibbles at 5.9, and with 600+ at 3.7. The giant's room averages 3.8. The speed model follows this to within about 1%.
  - *The giant stands again after falling asleep.* This is the original: room 58's exit rule (A01AC) resets `v186`, so on return the giant is drawn awake (actor 37, without his chest) and cycles in place. The original's screen at pass 15200 matches the port's.
  - *No sound.* The console bell made no sound in a terminal. `kqplay` now plays the rebuilt bell (see Sound above). F8 or `--mute` turns it off.
  - *Skipping the load pauses.* F9 or `--fast-loads` leaves out the disk reads, room-change bookkeeping and picture drawing from the pacing (about 10 s of each room change) and keeps the original's pace for everything else. The game plays the same, because its state depends only on passes, and the Apple has no clock. `--speed` scales everything, walking included.

**Optional items, done 2026-11-08:**

- **Joystick.** The original's calibration and per-pass reading, from the
  disassembly (`$4817`, `$4846`); any Windows game controller in `kqplay`. Buttons 0
  and 1 are the Apple's two buttons (swim, and jump or duck, through the global
  script). tests/joystick.rs.
- **Original save disks.** `kqplay --savedisk FILE`; saves move both ways between the
  port and the original. Checked against the original in refemu, which now emulates
  disk writes. This found a restore bug in the port (the carry into the room-entry
  random number). See [save-format.md](save-format.md#save-disks-confirmed-with-refemu)
  and tests/save_disk.rs.
- **NTSC colour fringing.** `--ntsc` or F7: each dot takes the colour of the 4-dot
  window around it at its colour phase. Display only; off by default.
- **Speck fill.** `--fill-specks` or F6: picture pixels the fills miss (still the
  buffer's initial white) take the colour at least two of their neighbours share, if no
  neighbour is white. On the castle screen (room 2) it changes exactly the 3 specks
  seen in play. Display only; off by default.
- **Apple text-mode characters.** With a user-supplied IIe video ROM dump
  (`--charrom FILE`, or found next to the disk images by its part number), the text
  screen uses the Apple's own characters. Checked with 342-0265-A; the port ships no ROM.
- **The title pictures** are drawn as the original draws them, like room changes
  (refemu screenshots show the title scroll part drawn).

- **Playing from `.nib` images.** `Disks::from_files` (src/kq.rs) takes `.nib` or
  `.do`/`.dsk`, decodes `.nib` images itself and orders them by volume (disk 1-3), and
  refuses the save disk or damaged images. Every tool uses it; `kqplay` needs no
  extraction step. The decoded images are byte-identical to extracted/disk1-3.do, and
  the max-score replay from the `.nib` files matches the original on every pass.

- **The start-up screen.** As the original: black while DOS boots and loads the engine,
  then a white screen with "Please wait..." (row 9, column 13, `$4537`) while the
  engine loads its data, then the title screens. Every phase uses times measured in
  refemu (`REFEMU_PCLOG`), so the first pass begins 33.42 s after power-on, as on the
  Apple; fast loads skip the loading phases. The title's own time was 4.2 s short
  before (only its sector reads were counted), and its disk and drawing work was
  charged again to the first pass (23.4M cycles against the original's 13.3M; now
  12.9M).

The original plan for M6 was:

- **Speed:** decide the cycle rate (see [Decisions](#decisions)).
- **Script 064:** apply the chosen policy.
- **Hardware edge cases:** confirm the screen-edge clamps, the horizon, and
  where the ego starts after death and restart.
- **Colours:** compare against a reference for the double hi-res palette.
- **Full playthrough:** play to the end with maximum score (158 points, if the
  engine's maximum matches) using a recorded input script.

## Testing

- **Data tests (existing).** Every resource decodes, and every structural
  check passes. Keep these as `cargo test` cases that run when the disk images
  are present, and skip them otherwise.
- **Golden images.** Hash the rendered picture and priority buffers for all 83
  pictures, and the decoded cels. These catch regressions in the renderers.
- **Opcode unit tests.** For each opcode, run a tiny hand-written script on a
  known state and check the resulting state. The handler addresses in
  script-format.md give the expected effects.
- **Replay tests.** Record the key presses for each cycle, replay them headless,
  and compare the state at checkpoints. A walkthrough recording becomes a
  regression test for the whole game.
- **Optional reference traces.** For the hard cases (collision, the RNG, timing
  and the parser), the clearest evidence would come from recording the
  variables and actor positions each cycle while the original runs on an Apple
  II emulator, then comparing with the port's trace. This uses an emulator only
  as an external measuring tool and doesn't affect the no-emulation rule for
  the port. It needs an emulator with memory watch or trace support on your
  machine.

## Decisions

All four recommendations below were accepted on 2026-11-07.

1. **Game speed.** The original has no speed control. A cycle takes as long as
   the 6502 needs, so busy screens run slower, and the script timers count
   cycles. A native port can:
   - **(a)** run at a fixed rate, measured as a typical cycle rate on real
     hardware or an emulator, or
   - **(b)** estimate each cycle's original duration from the work it did
     (actors drawn, pixels plotted) to recreate the original slowdowns.
   
   **Recommendation:** (a) first, with a speed setting. Consider (b) only if
   the timing of particular puzzles needs it.
2. **Script 064's header defect.**
   - **(a)** Reproduce it: the parser rules start one byte late and the first
     message loses its "S". This means modelling what lies past the end of the
     script in memory, which depends on the room loaded before it.
   - **(b)** Use the corrected section boundary.
   
   **Recommendation:** (b), with a compatibility switch that turns on (a) once
   the memory layout after the script is understood.

   **Outcome (2026-11-08):** (b). Room 64 turned out to be unreachable in normal play,
   so (a) would never show and the switch isn't needed.
3. **Save format.** Choose between the port's own save files, which are
   simplest, and saves compatible with original disk-4 images, which needs the
   save-format work.
   
   **Recommendation:** the port's own files first.
4. **Display.** Choose the scaling and aspect ratio. Each logical pixel is 4
   double hi-res dots wide and one scanline tall, so drawing it as 2×1 gives
   about the original 4:3 shape. Also choose whether to offer an option that
   imitates NTSC colour fringing.

## Risks

| Risk | Mitigation |
|---|---|
| The scripts depend on undocumented engine quirks (scanning, 8-bit wraparound, carry-dependent RNG) | The VM runs on raw bytes, and quirks are reproduced on purpose and tested. Reference traces can be used for confirmation. |
| Timing-sensitive puzzles behave differently at a fixed speed | Decision 1; replay tests that measure in cycles, not seconds |
| Gaps in understanding found during implementation | Each milestone starts with a short reverse-engineering pass on its unknowns, recorded in docs/ |
| Only one dump of the disks exists | Everything is validated against internal consistency, so a second source is optional |
| Legal: game content | The port reads the user's own disk images and never bundles content |
