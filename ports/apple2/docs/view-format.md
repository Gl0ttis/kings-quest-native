# View (sprite) format

Views are the resources in the `$1900` directory. Scripts load them with
`load_view` (`$24`). [src/view.rs](../src/view.rs) decodes them, and `kqview`
renders every view to `extracted/views/`: one sheet per view with one row per
loop, plus `overview.png`.

All 88 views (674 cels) decode cleanly. The directory has 120 entries, from `$1900` to `$1A67`. Entry 101 duplicates entry 36's disk address while the size table disagrees, and no script loads it, so it is treated as unused. Every row ends exactly on its
end-of-row code, no row is wider than its cel, and every render is a
recognisable sprite. View 0 is the ego, with four walk loops: right, left,
toward the viewer and away.

Addresses below are aux-memory addresses in the `aux_09C0_code` resource unless
marked as engine addresses.

## Loading (confirmed)

- **Where views go.** Views load into aux memory using a bump allocator (engine
  `$47BB`), through the pointers at engine `$7022` (starting at aux `$9800`) or
  engine `$6F06` (starting at aux `$D000`).
- **Sizes come from a table.** The amount to reserve is the view's size from
  the **`$1500` table**: a fixed resource of 120 little-endian `u16` sizes, one
  per view number. 88 of the 89 present directory entries match their table entry exactly. The odd one is view 101, a dead alias of view 36. If
  there is no room, the engine prints the message at engine `$67DC`.
- **The slot table.** The view's address is stored at engine `$6F41 + 2n`. An actor
  points at it through its fields 1 and 2.

## Layout (confirmed)

| Part | Layout |
|---|---|
| View | `[0]` loop count (copied to actor field 6), then a `u16` offset per loop, counted from the start of the view |
| Loop | `[0..4]` header. Bytes 0–2 are copied to actor `$0C`, `$0D` and `$09`; byte 3 is the cel count. Then a `u16` offset per cel, counted from the start of the loop. |
| Cel | `[0]` height, `[1]` width, then a pixel stream |

**The pixel stream** (`$1124`, `$1194`) is a series of nibbles, high nibble
first, running continuously across rows:

| Nibble | Meaning |
|---|---|
| 1–7 | a run of *n* pixels in the colour given by the next nibble |
| 8 | end of row |
| 9–15 | skip `n & 7` transparent pixels |

Colour nibbles are stored directly as the buffer's visual value. This is the
same Apple colour space as a picture *after* its colour table, so cels use no
table. A view has no mirrored loops; each direction is drawn separately.

## Drawing (confirmed, `$10EF`)

- **Position.** The cel's bottom row is drawn at the actor's y (`[$1B]`) and its
  left column at x (`[$1A]`). The top row is `y − height + 1` (`$162B`).
- **Priority check.** For each pixel, the engine compares the priority already
  in the buffer with the actor's priority:
  - **Not higher than the actor's:** the pixel is drawn.
  - **15:** the engine looks down the same column until it finds a priority
    that isn't 15, and compares with that instead.
  - **Otherwise:** the pixel is hidden behind the scene.
- **Two entry points.**
  - `$10AD` draws a sprite and keeps the buffer's priority.
  - `$10B5` writes the actor's priority as well, which adds the sprite to the
    picture permanently. It also marks with priority 15 the cel's bottom
    row, the row at the 8-row boundary above it (`y & $F8`), and both side columns
    between them (`add_to_picture`, `$11BB`/`$11C8`). The look-down rule for 15
    applies to `$10AD` only.
- **Actor field `$1C`** is set to 1 if no pixel of the cel was drawn.

## Priority (confirmed, `$1374`)

- **Fixed priority.** When actor flag bit 0 is set and `[$1D]` is non-zero, the
  actor's priority is `[$1D]` (set by `set_priority`, `$2F`).
- **Priority from position.** Otherwise:
  - y < 48: priority 1
  - otherwise: `((y − 40) × 2) & $F0`, which is one priority band per 8 rows

## Placement limits (confirmed, `$1280`)

When an actor moves, its x is clamped to `11 ≤ x ≤ 150 − width`.

y is clamped to:
- at least 41, unless actor flag bit 3 is set (`obj_flag8_on`, `$40`)
- at most 162

## Collision with the picture (`$12C7`)

Before an actor moves, the engine checks its new bottom row against the priority
buffer. The rules (implemented in `World::collide`) are in
[motion.md](motion.md): for other actors priority 1 is free, 0 blocks unless flag
bit 2 is set, 15 blocks unless flag bit 1 is set, and a pixel of the actor's own
priority blocks; for the ego, priority 0 never blocks, and `v4` records whether it
stands on water. Still open: which flag combinations enable water and the
conditional barriers, beyond those the scripts use.
