# Script format

Scripts are the resources in the `$1800` directory: script 0 is global and
scripts 1–80 belong to rooms. [src/script.rs](../src/script.rs) parses them, and
[src/vocab.rs](../src/vocab.rs) parses the parser vocabulary. Running `kqscript`
writes a disassembly of every script to `extracted/scripts/NNN.txt`, plus
`vocabulary.txt`. All addresses are in the main engine image (`$4000`–`$71FF`).

## Loading (confirmed)

- **Where scripts load (`$429D`).** A script loads to the address held in
  `$6653`, which starts at `$7068`. After the global script loads, `$6653`
  moves past it, so a room script always loads right after the global one.
- **The header** is four little-endian `u16` section lengths. Section 0 starts
  at offset 8, and each section starts where the previous one ends.
- **Section pointers.** They are stored at `$6EE5` for the global script and
  `$7031` for the room script. `$63E1`/`$63E7` copy one set into zero page:
  `$9C` = actions, `$9E` = cycle rules, `$A0` = parser rules, `$A2` = messages.

| Section | Contents | Used by |
|---|---|---|
| 0 | action code | action interpreter `$58AF` |
| 1 | cycle rules | matcher mode 1, every cycle (`$40BA`) |
| 2 | parser rules | matcher mode 0, after a command is typed (`$408E`) |
| 3 | messages: NUL-terminated text, ending with `$FF` | — |

**Running order.** For each kind of rule, the global script runs first and then
the room script. In the cycle pass, the global rules repeat while the last action
returned a non-zero result (`$40C2`).

## Variables (confirmed)

The scripts use one array of byte variables, `v0`–`v255`. Variables below `$20`
are stored at zero page `$D0+v`, and the rest at `$6E06+v`. Rule conditions use
the same rule.

| Variable | Meaning |
|---|---|
| `v1` | current room. `new_room` writes it, and the room-object check at `$0B37` compares against it. |
| `v19` (`$13`) | score: dropping an item subtracts points, and the inventory screen prints it ("Your score is:", `$4655`, called from `$45CF`) |

## Rule matcher (`$62B0`, confirmed)

The matcher reads from a pointer at `$A4`. Skip offsets are relative to the byte
just after the 2-byte offset.

| Byte | Mode | Form | Meaning |
|---|---|---|---|
| FF | both | — | end of rules |
| FE | both | off | go to offset |
| FC | both | conds FE off | if conds fail, go to offset |
| FB | parser | conds FE off | same as FC |
| FB | cycle | conds FE act | if conds pass, run the action at `act` |
| FD | parser | x1 y1 x2 y2 conds FE off | if the ego is outside the rectangle or conds fail, go to offset |
| other | parser | verb noun conds FE act | if the typed words match and conds pass, run `act`; noun 1 matches any noun |
| other | cycle | x1 y1 x2 y2 conds FE act | the first byte is x1; if the ego is in the rectangle and conds pass, run `act` |

- **Conditions** are pairs (variable, value), ending at `FE` (`$638D`). Bit 7 of
  the value means "not equal".
- **The ego rectangle test** (`$6363`) passes when `x1 ≤ ego.x ≤ x2` and
  `y1 ≤ ego.y ≤ y2`. The ego's position is at `$0118`/`$0119`, which is object
  0 in the object table at `$0100`.
- **After an action,** a non-zero result stops matching. Otherwise a following
  `FE off` is taken as a jump.
- **Failure scans byte by byte.** When a rule fails, `$63C2` steps back one byte
  and scans forward to the next `FE`. This depends on the byte layout, so a port
  must run the matcher over the raw bytes, not over the parsed tree.

## Action code (`$58AF`, confirmed)

The action interpreter's PC is at `$A6`, and starts at section 0's base plus the
action offset. It fetches an opcode: `FF` ends the action, and `$00`–`$43`
dispatch through `$6AF3`. An opcode of `$44` or higher is a fatal error (message
at `$6B7B`). Each operand is fetched by `$58F2`. A non-zero result in `$08` stops
the action.

**Operand counts.** I explored every control-flow path through each handler,
including tail branches into other handlers. Every opcode fetches a fixed number
of operands (`script::OPERANDS`), with these notes:

- `$36` takes 2 operands. It fetches one, then branches into the `$08`/`$09`
  handlers, which fetch the second.
- `$43` ("restart?" prompt) either restarts the engine or returns, having
  fetched no operands.

**Validation.** Every section 0 in all 81 scripts splits exactly into 2,793
`$FF`-terminated sequences. The rule sections hold 3,316 rules, and every skip
target lands on a rule boundary.

Some rules start an action in the middle of a sequence to reuse its tail. Every
such entry lands on an opcode boundary except one:

- **Global script, parser rule `$00A4`, entry `$01DB`.** It starts inside the
  junk operands of opcode `$31`. `$31` calls `$56E6`, which fetches two bytes and
  discards them, so execution still reaches an `FF` and ends the action.

### Objects and actors (confirmed layout, partial)

The scripts refer to **objects** by number. An object is an 8-byte record in
the table at `$1A80`: 150 records, loaded from a fixed resource. `$498E` selects
a record into `$49`. Each object is linked to an **actor**, a 31-byte record in
the table at `$0100` that holds the on-screen state. `$4973` selects an actor
into `$94`, and `$49A9` selects object *n* together with its actor (object byte
7 = actor slot). Object 0 is the ego.

| Object byte | Meaning |
|---|---|
| 2 | Bits 0–6: the room the object is stored in (0 = none). Bit 7: carried. |
| 5, 6 | stored x, y |
| 7 | actor slot |

| Actor field | Meaning |
|---|---|
| 0 | room |
| 1, 2 | pointer to view data |
| 3 | loop |
| 5 | view number |
| 7 | number of cels in the loop |
| 8 | cel |
| `$0B` | flag bits: bit 0 = fixed priority (opcodes 2F, 34, 1D, 1E); bits 1–2 set by 3A/3B; bit 3 set by 40 (lets the actor go above y = 41, `$12A1`) |
| `$09` | animation delay, from loop header byte 2 |
| `$0A` | delay countdown; the actor updates when it reaches 0 (`$5D05`) |
| `$0C`, `$0D` | x and y step sizes, from loop header bytes 0 and 1 |
| `$10` | current direction 1–8 (0 = stopped) |
| `$13` | actor slot number |
| `$0E` | cycle mode (see motion.md) |
| `$0F` | 1–8: a fixed direction; `$0C`–`$11`: a motion mode (see motion.md) |
| `$16`, `$17` | pointer to the current cel |
| `$18`, `$19` | x, and y of the bottom edge |
| `$1A`, `$1B` | new x, y: where the actor moves to this cycle (copied to `$18`/`$19` when drawn; a blocked move falls back to `$18`/`$19`) |
| `$1D` | fixed priority (high nibble), used while flag bit 0 is set (`$1374`) |
| `$1E` | drawn on screen |

**Positions are clamped** to `11 ≤ x ≤ 150 − width` (`$5CD7`) and
`3 + height ≤ y ≤ 161` (`$5CEE`).

**Player movement** is controlled by `$67`/`$68` (0 = the player controls the
ego). Move targets are stored at `$69`/`$6A`.

### Opcodes

**Confidence.** The operand counts and the state each handler changes come
straight from the code. The names are based on that and on how the scripts use
each opcode. Some names stay mechanical, but their effects are known (see
[motion.md](motion.md)): actor field 4 = 1 makes it ignore the barriers; flag bit 2
lets it stand on priority 0, bit 1 lets it pass priority 15, and bit 3 lets it go
above the horizon.

| Op | Operands | Name | Effect |
|---|---|---|---|
| 00 | — | nop | — |
| 01 | — | reset_ego | Reset the ego: view 0, the loop kept and the cel taken from field 4, room = `v1`, default motion |
| 02 | v | add_actor | Load view *v* and bind it to the next free actor slot (at most 4) for object *v*, at loop 0, cel 0. The actor appears when it is drawn. |
| 03 | — | clear_actors | Save each in-room actor back to its object (room, position, loop, cel; `$491F`), clear the drawn flags and remove all actors (count = 0) |
| 04 | r | new_room | `v1 = r`, then load room *r*; stops the action |
| 05 | — | redraw_room | Load the picture for room `v1` into aux `$D800` (`show_overlay_picture` draws it), then reset the ego |
| 06 | — | show_overlay_picture | Clear the screen and draw the picture held at aux `$D800` |
| 07 | o | draw | Put object *o* on screen in room `v1`, nudging it in a spiral (`$655A`) until it fits; then sets its stored room to `v0`, which is always 0. Room bytes are written for real when `clear_actors` saves the actors. |
| 08 / 09 | v | set / reset | `v = 1` / `v = 0` |
| 0A | lo hi | print | Print the message at offset `hi:lo` of the message section |
| 0B | o | carry | Set object bit 7 (also runs an anti-tamper check) |
| 0C | o | obj_erase | Erase the object from the screen and clear its stored room (bit 7 kept) |
| 0D | o | uncarry | Clear object bit 7 |
| 0E | a b | copy_position | Give object *b* the position of object *a*, then draw *b* (as `draw`, nudging it to fit) |
| 0F | v | input_number | Read a decimal number from the keyboard into *v* |
| 10 / 11 | v | increment / decrement | Saturates at 250 / stops at 0 |
| 12 | v n | assign | `v = n` |
| 13 | o c | set_cel | Set the cel; cycle mode 1 |
| 14 / 15 | n | add_score / sub_score | `v19` ± n; picking up and dropping items |
| 16 | o | play_loop_once | Play the loop forward once, then set `v8` (cycle mode 4 → 3); for the ego, also sets `$67`. It means to clear `v8` first, but writes through the wrong index register (`STA $00D0,Y` with Y = 0), so it clears `v0` instead. |
| 17 | o | play_loop_reverse | Clear `v8`, then play the loop backwards from the last cel and set `v8` (cycle mode 2) |
| 18 | o x y | move_to | Motion mode `$0E` toward (x, y); for the ego, also takes control from the player |
| 19 | o x y | position | Set x and y (clamped), also into object bytes 5 and 6 |
| 1A / 1B | o | obj_field4_on / off | Actor field 4 = 1 / 0 |
| 1C, 41 | v | set_ego_view | Give the ego loaded view *v* (same handler) |
| 1D | o | approach_ego | Motion mode `$0F`: approach the ego to within 5, then set `v6`; fixes the priority at 15 until it arrives (arrival clears flag byte `$0B`) |
| 1E | o d | follow_ego | Motion mode `$11`, keeping distance *d* (`$6D`); fixes the priority at 15 |
| 1F | o | chase_ego | Motion mode `$0C`: chase the ego until touching, then set `v6`; cycle mode 0 |
| 20 / 21 | a b c | set_x_limits / set_y_limits | Store a clamped x into `$61`, b into `$63`, c into `$64` (y: `$62`, `$65`, `$66`): the barriers ([motion.md](motion.md#barriers-57f8-confirmed)) |
| 22 | o | wander | Motion mode `$10`: random wandering |
| 23 | y | ego_move_to_y | Move the ego vertically to y (motion mode `$0D`) |
| 24 | v | load_view | Load view *v*'s data only (slot table `$6F41`), for later `set_view` / `set_ego_view`. It does not create an actor. |
| 25 | — | redraw_picture | Draw the picture held at aux `$D000` |
| 26 | o l c | place_object | Set loop *l* and cel *c*, draw the object (nudging it to fit), then add it permanently to the picture (`$6476`). The actor stops being a sprite. |
| 27 | o l c | set_loop_cel | Set loop and cel; actor room = `v1` |
| 28 | x | ego_move_to_x | Move the ego horizontally to x (motion mode `$0E`) |
| 29 | x | nop1 | No effect |
| 2A | — | program_control | Ego cycle mode 0 (animate); `$67 = $68 = 1` |
| 2B | — | disable_input | Sets `$6F05`: typed input is ignored, except Esc, Tab, and the restart and restore keys (`$534E`). It stops the action. Used when the game ends. |
| 2C | p | load_picture | Load picture *p* into aux `$9800` and copy it to `$D000` |
| 2D | v n | ego_special_view | Give the ego view *v*, cycle mode 5; `$73 = n` |
| 2E | — | player_control | `$67 = $68 = 0` |
| 2F | o n | set_priority | Fix the actor's priority at *n* (flag bit 0; `[$1D] = n << 4`) |
| 30 | o v | set_view | Give the object loaded view *v*, at loop 0, cel 0 |
| 31 | x x | inventory_screen | The inventory (same routine as Tab, `$45CF`): text screen, the carried objects and the score, wait for a key, back to graphics. Operands ignored. |
| 32 | — | save_game | Save-game dialog |
| 33 | — | restore_game | Sets the restore flag; the main loop runs `$4F56`. Stops the action. |
| 34 | o | release_priority | Clear flag bit 0, so priority follows y again |
| 35 | v | copy_room_to | `v = v1` |
| 36 | a b | set_if_room_eq | `vb = (va == v1)` |
| 37 | v | new_room_var | `new_room` using the room number held in *v*; stops the action |
| 38 | o l | set_loop | Set the loop, at cel 0 |
| 39 | o | store_position | Add the object's current cel permanently to the picture (`$6476`), then copy its x and y into object bytes 5 and 6 |
| 3A / 3B | o | obj_flags6_on / off | Set / clear flag bits 1 and 2 |
| 3C | x x | text_screen | Switch to the text screen and clear it (operands ignored) |
| 3D | row col lo hi | print_at | Print a message at (row, column) on the text screen |
| 3E | lo hi | print_text | Print a message on the text screen |
| 3F | — | graphics_screen | Wait for a key, then return to graphics |
| 40 | o | obj_flag8_on | Set flag bit 3 |
| 42 | — | init_save_disk | Format a save disk (volume 10) |
| 43 | — | restart_prompt | "Do you want to start over?" |

**Message offsets check out.** Every print operand in all 81 scripts, about
1,500 of them, points exactly at the start of a message. `kqscript` prints the
text inline.

## Vocabulary (confirmed)

The fixed resource loaded at `$0800` holds 345 entries. Each entry is a
NUL-terminated word followed by three bytes:

- **position:** 1 = a single word, 2 = the start of a phrase, 3 = a later word
  in a phrase
- **type:** 2 = verb, 3 = noun, 4 = ignored; 0 on the non-final words of a phrase
  (the phrase's type is on its last word)
- **id**

A phrase takes the id on its last word. The list ends with an empty word. Verb
and noun ids are matched against `$9A`/`$9B`. For example, `look` is 11, and
`get` and `take` are both 9.

## Known data defect: script 064

The header gives section lengths of 64, 65, 1 and 102, which add up to 240
bytes, but the resource is 239 bytes. Section 1 actually ends with its `FF` at
offset 136, so it is 64 bytes long and every later section starts one byte too
late:

- the parser-rule pointer lands on `S` (`$53`), the first letter of the first
  message
- the messages pointer skips that `S`

On the original machine, the parser pass for room 64 would treat `$53` as a verb
number. If that doesn't match, it scans forward for an `FE`, past the end of the
script and into whatever follows it in memory. The death message would also lose its
"S" ("o you're walking on clouds, huh?").

**Room 64 is reachable, and the port reproduces the defect.** It is the fall from the
clouds; its only entrance is room 56's west edge while `v105 = 0` (`new_room 64`), that
is, before the beans are planted (global script, `05B6`). The beanstalk isn't the only
way into the clouds: on the gold-key path (the gnome's riddle failed), room 19's door
and the stairs (rooms 66 to 68) lead to room 69 and the clouds with no beans planted.
An earlier version of this section said the room couldn't be reached; it overlooked
the stairs. The gold-key route (routes/gold_key.route) walks off room 56's west edge
into room 64, and in refemu the original prints "o you're walking on clouds, huh?",
without its "S".

The port now uses the script as it is on the disk (`Resources::load(.., false)`;
`true` still gives the corrected boundary), so it prints the same message. Its parser
pass in room 64 starts on the `S` too. If a line is typed there, the original scans on
past the end of the script into whatever memory follows it (the remains of earlier
room scripts); the port's matcher stops at the end of the data, with no rule matched.
Typing "look" while walking on the clouds of room 64 gives the same state on both
(both answer "You can't do that here."), so the two agree in the measured case; the
port does not model the memory past the script.

## Still open

Nothing in the script format. The behaviour behind the opcodes is in
[motion.md](motion.md) (motion and cycle modes, barriers), [view-format.md](view-format.md)
(views and sprite drawing) and [text-and-parser.md](text-and-parser.md).
