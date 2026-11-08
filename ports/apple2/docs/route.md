# Maximum-score route and the autopilot

The route lives in [routes/max_score.route](../routes/max_score.route). It is derived
from the room scripts: their scoring rules, the conditions on them, the room exits, and
the special entrances. [src/bin/kqroute.rs](../src/bin/kqroute.rs) plays it with the
native engine and writes a key log. The key log is then replayed into the original game
in [refemu](../refemu/README.md), and the two traces are compared pass by pass.

The route scores **158 of 158** and plays from power-on to the coronation in 17,332
passes (the test compares all 17,339 passes recorded from the original). The original, given the same keys, does the same thing on every pass. The key
log is kept in [routes/max_score.keys](../routes/max_score.keys), and the test
`max_score_playthrough` (tests/reference_trace.rs) replays it against the original's
trace, `extracted/reference/max_score.csv`.

Run these from `ports/apple2` (kqroute reads `routes/room_exits.json`); the built
binaries are in the workspace's `target/release`.

```
kqroute extracted/disk1.do extracted/disk2.do extracted/disk3.do routes/max_score.route \
    --keys route.keys --trace route_trace.csv
refemu original/disk1.nib original/disk2.nib original/disk3.nib --keys route.keys \
    --passes N --trace emu_trace.csv
python tools/cmp_trace.py route_trace.csv emu_trace.csv 1
```

To watch it in the window, at the original's speed (about an hour; F12/F11 change the
speed, or start faster with `--speed N`; `--fast-loads` or F9 skips the 10 s or so of each
room change, 26 of the run's 60 minutes):

```
kqplay extracted/disk1.do extracted/disk2.do extracted/disk3.do --keys routes/max_score.keys
```

kqplay also takes the `.nib` images directly, in any order (each records its disk
number), so nothing needs extracting first:

```
kqplay "original/King Quest (1983)(Sierra)(Disk 1 of 3)[nib].nib" "original/...(Disk 2 of 3)[nib].nib" "original/...(Disk 3 of 3)[nib].nib"
```

While the log plays, typed keys are ignored; the front-end keys (F6 to F9, F11, F12, Alt+Enter)
still work. "More" pauses are
answered with RETURN after about 3 seconds, as refemu does with the original. When the
log ends, the game is yours. `--headless "" --keys … --trace …` replays it without a
window. That trace matches the original on every pass.

## The autopilot

- **Walking.** A breadth-first search over ego positions, using the engine's rules:
  - the screen edges and the horizon
  - priority 15, and pixels in Graham's own priority band
  - water: priority 0 under his whole bottom row, in rooms whose scripts react to `v4`
  - another actor's bottom row where their x ranges touch (`$0AC5`)

  Each step is checked with the width of the cel the engine will use for that
  direction (Graham is 6 wide walking sideways, 5 walking up or down). Water is judged
  with the narrowest cel. It replans every pass.
- **Edges.** The engine fires the right edge at x + width ≥ 150, so the last standing
  column is x + width = 149. In a room with no right-hand exit, the clamped
  position x + width = 150 is allowed too.
- **Changing rooms.** The exits come from each room's `v3` rules
  (`tools/room_map.py`, with a few added by hand in `routes/room_exits.json`
  where the exit sits behind an `if v3==N` block: rooms 55 and 63, and room 56 once
  the beanstalk is planted). Before leaving, the autopilot draws the destination room's
  picture and checks that Graham will land on open ground that can reach the next exit
  or the goal. That check applies even when he is already standing on the edge.
- **Deadly rectangles.** Cycle rules of the form "ego in [rect] → action" count as
  deadly when the action leads to the death flag `v32` or to water (`v4`). Flags are
  followed by value. For example, room 25's stream goes v112 = 1 → v110 = 1 →
  v110 = 2 with timer v12 → v32. Each rectangle keeps its rule's other conditions and
  only counts while they hold.
- **Chance.** Many encounters depend on the random bit `v14`, set at each room change.
  If a line fails because Graham dies, is robbed, is frozen by the sorcerer, or a
  `require` isn't met, the line is retried from a snapshot with 1–30 extra passes of
  waiting. If that fails, it backs up to one, two or three earlier lines. The result is
  still one ordinary playthrough.
- **Typing.** At most one key per pass, so the original reads them on the same passes.

Route steps: `room N [goal rect]`, `goto`, `area`, `edge`, `enter ROOM rect [water]`,
`hold U|D|L|R`, `say`, `key`, `wait`, `waitvar`, `require V VALUE`, `jumpuntil V VALUE MAX`,
`keepin rect…` (confine the planner in this room; no arguments lifts it), `final rect`
(walk into the game-ending rectangle), `expect score` and `expect room`.

**Debugging aids** (environment variables):

| Variable | Effect |
|---|---|
| `KQ_VARS=a,b` | print these variables at the end |
| `KQ_BASE_OUT=file` | write the picture buffer at the end |
| `KQ_VARLOG=file` | write every pass's variables, actors and follow state for the final run |
| `KQ_WATCH=pass` | print the ego and the log from that pass on |
| `KQ_HOPS=1` | print the room path and the chosen edge crossings |
| `KQ_MAP=1` | print the reachable map when a walk fails |
| `KQ_RETRY=1` | print each retry and why it failed |
| `KQ_DEADLY=1` | print the deadly rectangles |
| `KQ_TRACE=1` | trace collisions |
| `KQ_RNG=1` | trace room-change random draws |
| `KQ_RNG_ALL=1` | trace every random number (pointer, counter, byte) |
| `KQ_DUMPVARS=pass` | print all variables after that pass |

In refemu, `REFEMU_DUMP=p,…` prints the variables, the actor table and the follow-motion
zero page at those passes, and `REFEMU_F2=a,b` logs every change to `$F2` between those
passes. `REFEMU_RNG=1` logs each RNG call with the byte it reads, and
`REFEMU_PEEK=addr,…` logs every change to those bytes.

## Engine fixes found by following the route

- **Background save and restore.** The original saves the buffer under each actor
  (aux `$15BF`) and restores it to erase the actor (`$15F5`). That includes the
  background saved just before an actor is drawn permanently into the picture, so when
  such an actor becomes active again, its permanent image disappears. Room 11's corral
  gate depends on this.
- **Self-modifying code and the RNG.** The RNG reads engine bytes, and some of those
  bytes are patched at run time. One is the operand of the `JSR` at `$5560`, the last
  motion handler (from table `$6AE4`). Another is the operand of the `JSR` at `$58D6`,
  the last action handler (from `$6AF3`). The port tracks both. A third patched
  operand, `$523F` (the parser's word compare), is tracked too (see
  [motion.md](motion.md)). The RNG pointer didn't reach it in this playthrough.
- **`$F2`, the last width.** Several routines leave a width in `$F2`:
  - aux `$0AB0`: selecting a cel
  - aux `$13C4`: each actor's screen-update rectangle, max(old width, new width) + |dx|,
    clipped at x = `$96`
  - the background routines

  The follow motion reads it when it chooses its patrol target: `$5CD7` clamps x = `$96`
  to `$96 − $F2`. That is how far the condor (room 22) flies before turning. The port
  now leaves the update-rectangle width there after each pass (`$4E69`, slot order, so
  the last on-screen actor wins).
- **Arrival clobbers the motion registers.** The motion dispatcher (`$5531`) loads the
  actor's x, y and steps into `$57/$58/$55/$56`. The arrival routine (`$55F8`) reuses
  `$55/$57` as the loop and cel to select, and zeroes them. So a follow motion that
  carries on in the same call works from x = 0 and x-step 0. When the condor finishes
  a dive, it makes one step right before patrolling left.

## The route

| Segment | Points | Score |
|---|---|---|
| Move the rock, take the dagger (room 3) | 7 | 7 |
| Look in the stump, take and open the pouch (room 6) | 7 | 14 |
| Cut the well rope for the bucket (room 12) | 2 | 16 |
| Carrot from the garden (room 15) | 2 | 18 |
| The goat follows the carrot out of the corral (rooms 11/10) | 5 | 23 |
| The goat butts the troll off the bridge (room 25) | 4 | 27 |
| Clover (24), gold walnut opened (30), bowl and its word (31), pebbles (34) | 13 | 40 |
| Woodcutter: give the empty bowl, "fill", take the fiddle (44/79) | 8 | 48 |
| The elf's ring (18) | 3 | 51 |
| Castle door, bow to the king (2, 55 → 53) | 4 | 55 |
| Down the well rope, fill the bucket, dive (12, 49, 52) | 5 | 60 |
| Dragon's cave, douse the dragon, take the mirror (51) | 14 | 74 |
| The gnome's name and the beans (40) | 9 | 83 |
| Witch's house: push her into the oven, cabinet, cheese, note; eat the house (28/65) | 15 | 98 |
| The mushroom (47, east bank, from room 48) | 1 | 99 |
| Down the hole in room 48; the rat takes the cheese (73 → 75) | 2 | 101 |
| Leprechauns: the fiddle, the shield, the sceptre (76, 77) | 17 | 118 |
| Eat the mushroom, out through the small hole (78) | 3 | 121 |
| Read the note (any room) | 1 | 122 |
| The condor (22), before the beanstalk exists | 3 | 125 |
| Plant the beans (6), climb to the clouds (70 → 72) | 2 | 127 |
| The giant falls asleep; take the chest (58) | 15 | 142 |
| The sling (62) | 2 | 144 |
| Back down the stalk; the eagle's nest and golden egg (14 → 63) | 8 | 152 |
| Refill the bucket at the moat, castle door again (2) | 3 | 155 |
| Bow to the king with the three treasures (53); the coronation | 3 | 158 |

### Lessons from the scripts

- **Choices that cost points.** Two alternatives are mutually exclusive. If you fail
  the gnome's riddle, you get a gold key instead of the beans; it opens the door in
  room 19 (+2), but that path skips the beans (+4) and planting them (+2). If you kill
  the giant with the sling, you get +3 −1 for the pebble, against +7 for letting him
  fall asleep. Rubbing the ring makes Graham invisible, but when the spell expires the
  ring is lost (−3). The shield protects permanently.
- **Water.** Filling the bucket gives +2. Pouring it out and dousing the dragon each
  take the 2 back, so the bucket should be full again at the end.
- **The witch.** Each entry to room 28 toggles `v154`. On odd entries she is alerted
  and chases Graham. The +7 needs an even entry and the random bit set, and no
  protection active. Then she walks to her oven, and while she heats it (`v101 = 2`)
  Graham steps behind her ([34,110,40,128]).
- **The rat's door** and **room 65's table** are reached through gaps the planner can't
  see by itself: script flags while in a doorway, and a 5-pixel strip by the right edge
  that only the up/down cel fits.
- **The beanstalk.** Off the stalk in rooms 70–72 Graham falls; the safe rectangles
  come from the rules that reset `v101`. The stalk's own pixels block him in the band
  where his priority matches theirs, so the climb side-steps to x 76 there.
- **The condor** only appears before the beans are planted (`v35 = 0`). It grabs Graham
  when it touches him mid-jump (view 75), so the route keeps jumping until `v149 = 1`.
- **Earlier notes.** The corral gate opens from x 1–56, y 132–166. The goat waits in
  room 10 until it reaches Graham (`v40 = 1`). The carrot needs `v142`, which is set by
  entering the garden from below. The troll appears when Graham steps onto room 25's
  bridge band (y 93–94). The goat charges from y 91–92.
