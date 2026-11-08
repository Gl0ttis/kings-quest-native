# refemu: reference Apple IIe (measuring tool only)

This crate is a minimal Apple IIe used to **measure the original game**: how long each
main-loop pass takes, and what state it produces. The port in the parent crate does not
use it. The port reimplements the engine natively, and this tool only provides evidence
for checking it.

## What it emulates

- **CPU:** an NMOS 6502 with the documented opcodes and exact cycle counts, including the
  page-crossing and branch penalties. Undocumented opcodes stop the run with a report.
- **Memory:** 128K, with auxiliary memory (RAMRD/RAMWRT), the alternate zero page,
  80STORE/PAGE2/HIRES banking and the language card.
- **Disk II in slot 6:** it reads the raw `.nib` tracks at one nibble per 32 cycles, so
  the game's own RWTS and loader run unmodified. The disk the game expects (the volume
  number it puts at `$6F33`) is inserted automatically. The game still prints its swap
  prompt ("Please insert side B and press RETURN."), which the automatic RETURN answers.
- **Keyboard:** injected from a plan, or a key log (`--keys`). Paddles read as centred.
- **No Apple ROM.** ROM images are copyrighted, so the handful of firmware routines the
  game calls are substituted at their entry points:
  - XFER `$C314` and the Disk II boot read `$C65C`
  - INIT, HOME, SETNORM, SETKBD and SETVID
  - BASCALC, COUT and COUT1
  - PRBYTE
  - BELL, which is charged at about 116,000 cycles, roughly the cost of the real routine

  Any other firmware call stops the run with a note.

## Usage

```
cargo run --release -- ../original/disk1.nib ../original/disk2.nib ../original/disk3.nib \
    --plan "w5,U,w40,S,w5,R,w25,S,w5,>look castle,w15" --passes 130 \
    --trace trace.csv --shot screen.png
```

**The plan** runs in main-loop passes:

| Item | Meaning |
|---|---|
| `wN` | wait N passes |
| `U` `UR` `R` `DR` `D` `DL` `L` `UL` | press that direction key |
| `S` | press the last direction again, which stops |
| `>text` | type a command and RETURN |
| `kHH` | press a raw key code |
| `pHH` | press a raw key code once the game waits at a prompt (a letter for save or restore) |

**Other options:** `--keys FILE` replays a kqroute key log (lines "PASS KEY-HEX", the
key pressed before that pass); `--passes N` stops after N passes (default 400);
`--max-cycles N` stops after N cycles; `--picbuf FILE` writes the picture buffer
(aux `$4000`) at the end.

A fourth image is the save disk (volume 10): the game's writes go to it (the Disk
II write latch is emulated), and `--save-out FILE` writes it out afterwards.

**Output:**

- **Cycles per pass:** overall and per room, as a distribution, and converted to passes
  per second at 1.0205 MHz.
- **`--trace`:** a CSV with the room, ego position, direction, score and a hash of all
  variables at every pass.
- **`--shot`:** a screenshot of double hi-res page 1.
- **Profile:** with `REFEMU_PROFILE=1`, a profile of instructions by 256-byte page.
- **Debug variables:**
  - `REFEMU_DUMP=p,…` prints the variables, the actor table and the follow-motion zero
    page at the start of those passes.
  - `REFEMU_RNG=1` logs each random-number call with the byte it reads.
  - `REFEMU_F2=a,b` logs every change to `$F2` between those passes.
  - `REFEMU_PEEK=523F,…` logs every change to those main-memory bytes, with the pass and
    the instruction that made it.
  - `REFEMU_TEXTDUMP=FILE` writes the text page's screen codes (24 lines of 40) the
    first time the game waits for a key in text mode. The COUT stand-in neither wraps
    nor scrolls, so cells the game cleared that way may read `00`.
  - `REFEMU_PCLOG=hex,…` logs the cycle count when main-memory code reaches those
    addresses (the first 20 times each).
  - `REFEMU_CHARS=n` logs every character drawn (aux `$14A4`) during pass `n`, with the
    text row and column.

**Automatic RETURN.** When the game waits for a key that the plan doesn't supply (a
"more" pause, a disk-swap prompt, a Y/N question), refemu presses RETURN after 3 million
cycles. It only does this while the game is polling the keyboard, and the 3 million
cycles count from the last automatic RETURN as well as from the start of the pass.
Before 2026-11-08 neither condition held, so long room loads and a pause right after one
got extra RETURNs. The game read those as empty typed lines. That never changed the game
state, but it did change the text window and the timings.

## Findings so far (2026-11-08)

- **Random numbers.** Every call to `$65F1` during a 900-pass walk came from `$4BBD`
  (room changes) with carry 0, and the pointer advanced one byte per call.
  `$4D` = pass number. The port models this (`docs/motion.md`).
- **Pass-by-pass agreement.** From power-on, the port reproduces room, ego position,
  direction and all variables `v0`–`v222` on every pass of the 158-point max-score
  playthrough (17,332 passes), a death and restart, and the walks in
  `tests/reference_trace.rs`.
- **The speed model** in `src/engine/cost.rs` was fitted from these traces.
- **Boot.** The game boots without any ROM and reaches its main loop (`$4026`) after
  about 34.1 million cycles, 33.4 s at 1.02 MHz. Room 1 then looks the same as in the
  port.
- **Speed depends on the room:**

  | Situation | Cycles per pass | Passes per second |
  |---|---|---|
  | Room 1, standing or walking | about 140,000 | **about 7.2** |
  | Room 8, standing | about 88,800 | **about 11.5** |

  The first pass, which includes loading, takes 13.3 million cycles.
- **Movement agrees with the port.** Walking moves Graham one pixel per pass, and the
  second key press stops him. Leaving room 1 by the right edge at x = 144 enters room 8
  at x = 11.
- **Where the time goes**, idle in room 1:

  | Share | Work |
  |---|---|
  | 36% | aux `$14xx`: copying the changed screen area, and character output |
  | 21% | aux `$11xx`: cel drawing |
  | 16% | `$63xx`: the rule matcher |
  | 9% | aux `$16xx`: saving and restoring the background |

  Pass time therefore follows sprite sizes, the area of screen redrawn, and the length
  of the room's scripts.
