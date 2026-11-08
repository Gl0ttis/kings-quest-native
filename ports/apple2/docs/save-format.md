# Save, restore and restart

Implemented in [src/engine/save.rs](../src/engine/save.rs). All addresses are in
the main engine image.

## The original (confirmed)

**Where saves go.** Each saved game takes one track on the save disk, which is a
disk formatted by the game's "init disk" command with volume 10. Letter A is
track 1, through Z on track 26. The records at `$7024`, `$7028`, `$6EED`, `$703F`
and `$6F02` hold disk 10, sectors 0, 1, 6, 7 and 8; the letter (`$59E3`) sets the
track.

**What is saved (`$5905` → `$5941`).**

| Sector | Block | Address | Length | Contents |
|---|---|---|---|---|
| 0 | header | `$1600` | 1 sector | bytes 1–5 are a signature copied from `$7039` ("ame  ", from "game  ") |
| 1 | object table | `$1A80` | `$500` | 111 object records, the object names, and 80 further bytes |
| 6 | actor table | `$0100` | `$7C` | the four 31-byte actor records |
| 7 | zero page | `$0060` | `$90` | engine globals (actor count, barriers, motion targets, parsed words, script pointers) and `v0`–`v31` at `$D0` |
| 8 | variables | `$6E06` | `$DF` | `v32`–`v222`. Variables 223–255 are not saved. |

Before writing, the game copies each actor back into its object record (`$491F`).

**Restoring (`$4F56`).**

1. Ask for the letter. Read the header; if the signature doesn't match, print
   "That game was not saved on this disk." and offer RETURN to try another
   letter, or Esc to give up.
2. Read the four blocks back.
3. Copy the actors into their objects and free the actor slots
   (`clear_actors`).
4. Re-enter room `v1` (`$4B19`), keeping `v18` and `v14`.
5. Set `v25 = 0`, so the room scripts set the room up again. Clear `$6F05`, which
   re-enables input after a game over. Set `v29 = 1`.

The picture, the loaded views and the room script aren't saved; they are
rebuilt by the room scripts.

**Restarting (opcode `$43`).** It asks "Do you want to start over? (Y or N)".
`Y` goes back to `$4018`: all state is cleared, the object table is reloaded,
and the global script starts room 1 again.

## The port

**Save files.** A save is a file, `saves/kq-a.sav` to `kq-z.sav` (`kqplay
--saves DIR` changes the folder). It holds the same four blocks in the same
layout, after an 8-byte tag `KQSAVE01`, 1,815 bytes in all. Keeping the
original layout made reading and writing real disk-4 save images (below) a matter of
added later without changing the saved data.

**Gaps.**

- **The 80 bytes after the object table** (`$1F30`–`$1F7F`) aren't modelled, so
  the port saves them as zero.
- **Zero page.** Only the addresses the port models are filled in. Script
  pointers are rebuilt on restore.

**Prompts.** The prompts are the original strings, read from the engine image on
the user's disks.

- The disk-swap prompts ("Insert a disk initialized with…", "Re-insert your game
  disk…") are left out, because there is no disk to swap.
- The save confirmation keeps only "The game has been saved."
- `init_save_disk` (`$42`) does nothing, because save files need no
  formatting.

**Tests.** [tests/save_restore.rs](../tests/save_restore.rs) runs the real
scripts, and skips if the decoded disk images are missing. It checks:

- **A round trip.** Take the pouch in room 6 (score 3), save to slot A, then
  walk away and lose the score and the pouch. Restoring slot A brings back the
  score, the pouch, the room and Graham's exact position, and the room scripts
  run their set-up again.
- **An empty slot** gives the original "not saved on this disk" message.
- **Restart** through the `5` key and `Y` brings back a fresh room 1 with a score
  of 0.

## Save disks (confirmed with refemu)

`src/savedisk.rs` reads and writes saves on an original save-disk image (`.nib`, which
must be volume 10, or a DOS-order `.do`); `kqplay --savedisk FILE` uses one in place
of the save files.

**The format on disk.** The game writes through a 256-byte buffer at `$1600` ($5941,
$5E63). Sector 0 is the header: the signature in bytes 1-5. Then each block is a
stream that starts at offset 0 of its first sector: a 2-byte length, then the data,
carried on into the next sectors ($605E). Objects start in sector 1, actors in sector
6, zero page in sector 7 and variables in sector 8. Restoring reads the stored length
($5EC7).

- **The object stream overlaps.** 2 + `$500` bytes run 2 bytes into sector 6, which
  the actor block then overwrites. So the original restores `$1F7E-$1F7F` as `$7C $00`,
  the actor block's length. The port does the same.
- **The buffer is never cleared**, so bytes after a block hold leftovers. Nothing reads
  them; the port writes zeros there.
- **The variables block** starts at `$6E06`, the address of `v0`, so its first 32
  bytes are not variables. The original's own save has zeros there, as the port writes.

**Checks (refemu, which emulates the Disk II write latch for this).**

1. The original replayed the max-score route to pass 2000 and saved to letter A on a
   blank disk 4. The port reads it back (extracted/reference/save_disk_pass2001.nib).
2. Play went on to pass 2300, then restored A, then ran 100 more passes
   (extracted/reference/restore_pass2301.csv). That is the reference.
3. The port restoring the original's save, the port restoring its own save, and the
   original restoring the port's save all match the reference on all 100 passes
   (tests/save_disk.rs for the first two; the third was run once in refemu).

This found one port bug: restore re-enters the room with the 6502 carry set, which
changes the room-entry random number (see [motion.md](motion.md#random-numbers-65f1-confirmed)).
