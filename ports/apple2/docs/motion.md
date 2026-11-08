# Movement and animation

This covers how actors move and animate on each game cycle. All addresses are in
the main engine image unless they are marked as aux. Actor fields are described
in [script-format.md](script-format.md#objects-and-actors-confirmed-layout-partial).

## The game cycle (`$4026`, confirmed)

Each pass of the main loop does the following:

1. Polls the keyboard and the parser (`$52A8`, `$408E`).
2. Updates the actors (`$4BF0`).
3. Increments the cycle counter `$4D` and decrements the timers (`$40D0`).
4. Runs a restore, if one was requested (`$4F56`).
5. Runs the cycle rules, global first and then the room (`$40BA`).

## Keyboard (`$52A8`, confirmed)

Keys set the ego's direction in `$75`. Pressing the key for the direction the
ego is already moving in stops it (direction 0).

| Key | Direction | Key | Direction |
|---|---|---|---|
| ↑ (`$0B`) | 1, up | ↓ (`$0A`) | 5, down |
| `'` | 2, up-right | `.` | 6, down-left |
| → (`$15`) | 3, right | ← (`$08`) | 7, left |
| `/` | 4, down-right | `;` | 8, up-left |

If no key has set a direction, the ego uses `$74` instead: the joystick. While `v30` is set,
the ego's direction is forced to 0.

## Actor update (`$4BF0`)

1. **Player control.** While the player has control (`$67 = 0`), the keyboard
   direction is copied into the ego's field `$0F`, if the ego's direction `$10` is
   0 or already equals it (`$4C10`). Then, whatever `$67` is, if the ego's cycle
   mode is below 2 it becomes 1 (hold the cel) when the ego is stopped and 0
   (animate) when it is moving; `$68 != 0` forces 0.
   - A view-0 ego under player control in cycle mode 0, whose `$0F` (below 9)
     differs from `$10`, has its direction cleared (`$4C5E`–`$4C85`).
2. **Updates.** Every actor that is in room `v1` and whose delay counter `[$0A]`
   has reached 0 is updated by `$60DD`, from the highest slot down (`DEX/BPL`):
   - **Direction.** If `[$0F]` is between 1 and 8, it becomes the direction
     `[$10]`. If it is `$0C` or above, the motion-mode handler picks the
     direction (`$5531`).
   - **Loop.** The animation loop is chosen from the direction (`$6239`).
   - **Barriers.** The `set_x_limits`/`set_y_limits` barriers are checked
     (`$57F8`).
   - **Animation.** The cycle-mode handler advances the cel.
3. **Delay counters.** Each actor's `[$0A]` counts down, and is reloaded from
   `[$09]`, the loop's delay (`$5D05`).
4. **Redraw.** If any actor reached 0 this cycle, everything is erased and
   redrawn (`$4DD7`–`$4E69`). The keyboard is polled between steps.

### Choosing a loop (`$6239`)

A view with one loop never changes loop. Otherwise the loop comes from the
direction. In this table, "keep" means the current loop stays.

| Direction | 0 | 1 | 2–4 | 5 | 6–8 |
|---|---|---|---|---|---|
| Views with 2 loops (`$6D69`) | keep | keep | 1 | keep | 0 |
| Views with 3 or more loops (`$6D72`) | keep | 3 | 1 | 2 | 0 |

The ego's view 0 has a loop for walking right, walking left, walking toward the
viewer and walking away.

### Moving

The actor's next position is its current position plus the step sizes `[$0C]`
(x) and `[$0D]` (y). The direction table at aux `$16F0` gives the sign of each
(aux `$122B`). The new position is then clamped to the edges of the screen and
the horizon (aux `$1280`), and checked against the picture's priority buffer
(aux `$12C7`; see [view-format.md](view-format.md)).

The check is done along the actor's bottom row (aux `$12C7`, now implemented in `engine::World::collide`):
- **Other actors:** priority 1 is free. Priority 0 blocks unless flag bit 2 is set. Priority 15 blocks unless flag bit 1 is set. A pixel equal to the actor's own priority blocks.
- **The ego:** priority 0 never blocks. Pixels with priority 2 or more follow the same 15 and own-priority rules. `v4` records whether the ego's whole bottom row is on priority 0 (or, once `v4` is set, on non-1 pixels). This is probably the water test.

**Blocked by the picture.** The actor goes back to its previous position
(`$12FC`).

**Clamped at a screen edge or the horizon.** The edge is reported: 1 = left,
2 = right, 3 = horizon, 4 = bottom (aux `$11F3`). For the ego it goes in `v3`.
For other actors it goes in `v22`, with the actor's view number in `v21`.
Scripts use this to move the player to the next room.

### Barriers (`$57F8`, confirmed)

`set_x_limits a b c` defines a vertical barrier at x = *a*, covering the band
from `a − b` to `a + c`. An actor in that band moving toward the barrier gets
field 4 set to 2, which blocks it for that cycle. `set_y_limits` does the same
for a horizontal barrier, using `$62`/`$65`/`$66`. Actors whose field 4 is 1
ignore barriers. That flag is set by `obj_field4_on` (`$1A`).

## Cycle modes (`[$0E]`, table `$6117`, confirmed)

| Mode | Behaviour |
|---|---|
| 0 | next cel, wrapping round |
| 1 | hold the current cel |
| 2 | previous cel; at cel 0, set `v8 = 1` and switch to mode 1 |
| 3 | next cel; at the last cel, set `v8 = 1` and switch to mode 1. For the ego, also give control back to the player. |
| 4 | switch to mode 3 (a start delayed by one cycle) |
| 5 | advance to the last cel of the ego's special view; then restore view 0, set `v8 = 1`, step right by `$73` and switch to mode 7 |
| 6 | step left by `$73`, then switch to mode 5 |
| 7 | give control back to the player, switch to mode 1, clear the direction and flags, reload loop 0's header |

`$73` (from `ego_special_view`, `$2D`) is the size of the one step after the special
view.

`play_loop_once` (`$16`) uses modes 4 then 3. `play_loop_reverse` (`$17`) uses
mode 2 and clears `v8` first, so scripts can wait for `v8` to be set.
`play_loop_once` means to clear `v8` too, but clears `v0` instead (see
[script-format.md](script-format.md)), so a script must clear `v8` itself.

## Motion modes (`[$0F]` ≥ `$0C`, table `$6AE4`, confirmed)

| Mode | Opcode | Handler | Behaviour |
|---|---|---|---|
| `$0C` | `chase_ego` (`$1F`) | `$43C6` | Head for the ego with no slack. On arrival, set `v6` to the actor's view number. |
| `$0D` | `ego_move_to_y` (`$23`) | `$55D6` | Move down (direction 5) until y reaches `$6A`, then arrive |
| `$0E` | `move_to` (`$18`), `ego_move_to_x` (`$28`) | `$5571` | Head for (`$69`, `$6A`) using the direction table at `$6EF1` (`$566F`), then arrive |
| `$0F` | `approach_ego` (`$1D`) | `$55A4` | As `$0C`, but stop within 5. On arrival, set `v6`. |
| `$10` | `wander` (`$22`) | `$5F2B` | Pick a random direction and duration. Pick again when the time runs out or the actor is blocked. |
| `$11` | `follow_ego` (`$1E`) | `$55AC` | Keep within `$6D` of the ego |

**Getting stuck.** The chasing modes (`$0C`, `$0F`, `$11`) remember the actor's
position from the previous cycle (`$7020`/`$7021`). If the actor hasn't moved,
it walks in a random direction for a few cycles to get round the obstacle.

**Arrival (`$55F8`).** The actor's step sizes and delay are reloaded from loop 0's
header, keeping its loop and cel (aux routine `$0C`; this is the port's reading of
it, which undoes the step of 4 set by `ego_move_to_x`/`_y`), and the ego gives
control back to the player. For modes `$0C` and `$0F`, `v6` is set to the view number;
otherwise `v8 = 1`. The motion mode and direction are then cleared.

**Scripts.** Scripts wait on these flags. For example, room 10's cycle rules
test `v6 == 14` after `chase_ego 14`.

**Registers.** The dispatcher (`$5531`) loads the actor's x, y, step x and step y into
`$57`, `$58`, `$55` and `$56`. The handlers work from these copies. Arrival uses `$55`
and `$57` as the loop and cel to select (both 0), so in mode `$11`, where arrival
switches between diving and patrolling and goes straight on, the rest of that call
sees x = 0 and step x = 0.

**Follow (`$11`).** `$6E` alternates between diving at the ego (chase with y slack
`$6D`, plus the strike check `$5D8E`) and patrolling the top of the screen. Each
patrol targets y = `$28` and alternately x = `$0A` or the right edge, `$5CD7($96)`.
That clamps to `$96 − $F2`, where `$F2` is the last width the engine computed. At the
start of a pass, that is the screen-update width of the last on-screen actor from the
previous pass (aux `$13C4`, run in slot order by `$4E69`): max(old cel width, new cel
width) + |dx|, clipped at x = `$96`. The condor in room 22 uses this mode.

**Self-modified operands.** The handler address is stored into the `JSR` at `$5560`
(`$5557`/`$555D`), and the action interpreter does the same at `$58D6`. The parser's
word compare (`$5232`) stores its table address into its own `CMP $6A70,X` at `$523E`:
`$70` when it looks up a word (`$5228`), `$84` when it tries the second word of a
phrase (`$522E`). So `$523F` is `$84` only when a phrase compare was the last compare
of the line, as in "carrot patch". The random number generator below reads all of
these bytes like any other engine code. The port tracks all three (`motion_jsr`,
`action_jsr`, `word_cmp`), and the `$523F` values match the original after each line
(watched with `REFEMU_PEEK`).

## Random numbers (`$65F1`, confirmed)

The engine has no real random-number generator. `$65F1` steps a pointer through
the engine's own code (`$4011`–`$64FF`) and XORs each byte with the cycle
counter `$4D`.

The pointer's low byte is incremented with `INC`, which doesn't touch the carry
flag, and the following `BCC` tests the carry left over from the caller. So the
high byte advances only when the carry happened to be set on entry; otherwise the
low byte wraps within its page. The pointer starts at `$4011` and goes back there
when the game is initialised (`$5D4C`).

There are two call sites: `$4BBD` (room change) and `$5F4A` (a random direction),
which is reached three ways. The carry on entry, traced through each path, is:

| Call | Carry on entry |
|---|---|
| Room change, `v14 = random & 1` (`$4BBD`) | 0 from the scripts: the edge path ends in `CLC; ADC #5`, and the other path inherits the action interpreter's `ASL` of an opcode below `$80`. 1 from restore when the ego left by no edge (`v3 = 0`): restore reaches `$4B19` straight from the loop at `$48DF`, which ends with carry set. Found by restoring a save in refemu; the port had restored with carry clear, so `v14` and the RNG pointer differed after the next room change. |
| Wander, picking a direction (`$5F2B`) | 0, left by the motion dispatch's `SBC`/`ASL` |
| Re-pick after a blocked trial move (`$5F6E`) | 0 if the picture blocked the move (the XFER return clears it); 1 if the second-edge-contact rule blocked it (an equal compare); otherwise the carry it entered with |
| Chaser's stuck detour (`$4413`) | 1 on the first try, after two equal compares. A retry happens only when `$6F >> 2` is 11 or more, so its carry is also 1 (`$6F` is always a multiple of 4, at least 4). |

**Confirmed against the original** with [refemu](../refemu/README.md). Over a
900-pass walk through rooms 1, 2, 15 and 16, every call came from `$4BBD` with
carry 0, and the pointer advanced one byte per call. `$4D` was exactly the pass
number (mod 256). The port reproduces this (`World::random_c`).

**Power-on.** On a real machine `$4D` holds whatever was in memory at power-on, so
the original's random sequence varies from one power-on to the next. refemu
starts with zeroed memory, and so does the port.

## Changing the ego's view on screen (`$4165`)

When the ego gets a new view while it is drawn and its priority isn't fixed
(`set_ego_view`, and cycle mode 5), the new cel has to fit where the ego stands.
Two things must hold:

- the move check (aux `$11E2`) passes
- the water state agrees with the swimming flag: `v4` (the bottom row is on water)
  equals `v24` (swimming)

If either fails, the ego is taken off the screen (`$5479`) and nudged with the
same spiral search as `draw` (`$655A`, limit `$94 − width`) until both hold. It is
then drawn again (`$5425`), and `v3` is cleared.

For example, in room 1 walking into the moat switches to the drowning view 93, and
the re-fit moves Graham fully onto the water. This matches the original pass for
pass (`tests/reference_trace.rs`, `fall_into_the_moat`).

## Collision with the picture (aux `$12C7`, read in full)

Before an actor moves, each pixel under its new bottom row (`$F2` wide) is checked
against the priority buffer, with the actor's priority for its new y (`$1374`) in `$14`.
`$135E` decides a pixel of priority 2 or more: 15 blocks unless flag bit 1 is set;
otherwise a pixel equal to the actor's own priority blocks (unless both are 15).

- **Other actors** (`[$13] != 0`): priority 1 is free; priority 0 blocks unless flag
  bit 2 is set; the rest go through `$135E`.
- **The ego** (`[$13] = 0`): priorities 0 and 1 never block; the rest go through
  `$135E`. Then `v4` (water) is set when, with `v4` clear, no pixel of the row is
  non-zero, or, with `v4` set, no pixel is priority 1 (so he stays in the water until
  a priority-1 shore pixel is under him).
- **Blocked:** the position goes back to `[$18]`/`[$19]` (`$12FC`). **Not blocked:**
  the priority is stored in `[$1D]`.

These are the only conditions; `World::collide` implements exactly this.

## Erasing and redrawing (`$4DD7`–`$4E69`)

1. Restore every actor's saved background, in slot order (`$4DD7`).
2. Clear `v3`, `v21`, `v22`; sort the actors into draw order (`$4E98`); move each in
   that order, a blocked one keeping its old position (`$4DED`).
3. Save every actor's background, in slot order (`$4E2C`).
4. Draw every actor in draw order, then `v7` = the ego's "nothing drawn" field (`$4E42`).
5. Update every actor's screen rectangle, in slot order (`$4E69`).

All backgrounds are restored before any is saved again, and all are saved before any
sprite is drawn, so each saved background is the bare picture and the restore order
can't matter, however the actors overlap. The port does the same (`restore_all`,
`move_all`, `save_all`, the draw pass, `commit_all`).

## The joystick (`$4817`, `$4846`)

**Calibration (`$4817`, once at power-on).** Both paddles are read (`$48BC`: count
while `$C064`/`$C065` stay high after `$C070`). A count of 0 for x means no joystick,
and `$F5 = 0` turns it off for good. Otherwise the resting position sets the
thresholds, half of it either side: `$F4 = x - x/2`, `$F5 = x + x/2`, `$F6 = y - y/2`,
`$F7 = y + y/2` (8-bit, no overflow check). Initialisation (`$5D4C`) clears `$60-$EF`
only, so the thresholds survive a restart.

**Each pass (`$4846`, from the keyboard step `$52A8` via `$4F0E`).**

1. **Buttons (`$489D`).** Button 0 (`$C061`, the Open-Apple key on a IIe) sets `v16`.
   Button 1 (`$C062`, Solid-Apple) sets `v17`. Either clears `$1D`, the "more" line
   count. The global script turns `v16` into `v27` (swim) and `v17` into `v20`
   (jump or duck).
2. **Direction.** `$74 = 0`. If the joystick is on (`$F5 != 0`) and this is the main
   loop's poll (`$F8 = 1`, set only around the `$52A8` call at `$4039`), each axis
   falls in a zone: 0 below the low threshold, 2 at or above the high one, 1 between. `$74` is
   the table at `$67F4`, indexed by `3 * x zone + y zone`: 8, 7, 6, 1, 0, 5, 2, 3, 4
   (up-left, left, down-left; up, none, down; up-right, right, down-right). A non-zero
   direction clears `v31`.

The redraw polls the keyboard four more times (`$5D33`–`$5D45`, `$F8 = 0`): those
calls read the buttons and set `$74 = 0`, and the main poll sets `$74` again before
it is next used. The port reads the joystick once a pass, at the main poll, so a
button press shorter than a pass that falls only within the redraw is seen by the
original and not by the port.

The port: `World::calibrate_joystick` and `poll_joystick` (src/engine/cycle.rs), with
the front end supplying paddle counts through `IoImpl::joystick`. `kqplay` reads any
Windows game controller (winmm) as counts 1-255, about 128 at rest; the hat pushes
to the edges. tests/joystick.rs walks Graham with the stick and swims with button 0.
