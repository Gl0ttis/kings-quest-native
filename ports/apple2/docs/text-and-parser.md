# Text, input and the parser

Implemented in [src/engine/text.rs](../src/engine/text.rs) and
[src/engine/parser.rs](../src/engine/parser.rs). All addresses are in the main
engine image unless they are marked as aux.

## Display (confirmed)

**Graphics text.** Text is drawn into the double hi-res screen by aux `$14A4`
(character output) and aux `$15AB` (clear to end of line).

- **Text rows.** Text rows are 8 screen lines tall. Rows 20–23 sit directly
  below the 160-line picture.
- **The font.** It is the fixed resource loaded to aux `$1D00`: 96 glyphs from
  space to `$7F`, 8 bytes each, with bit 0 leftmost.
- **Colours.** A 1 bit is background (white) and a 0 bit is ink (black), so
  text is black on white.
- **Character width.** Each glyph byte becomes 14 dots, 40 characters per line:
  bits 0–3 doubled go to aux memory, bits 4–7 doubled to main memory, with the
  top bit of each half dropped.
- **Glyph `$7F`** is the block cursor.

**Scrolling.** A carriage return clears the rest of the line, then moves down
one row. On row 23, rows 21–23 scroll up and row 23 is cleared (aux `$154C`).

**Room entry.** `show_overlay_picture` (`$5FA4`) clears the whole screen, text rows
included (aux `$10`), sets `$6EF0` so the prompt and the typed line are reprinted, and
moves the text position to row 20, column 0 (`$57EF`). So each room starts with an
empty text window and the prompt on row 20. (Until 2026-11-08 the port kept the old
text, which differed visibly from the original.)

**Disk swaps.** When the next room's data is on the other side of a disk, the original
prints "Please insert side B and press RETURN." and waits. The port reads every disk
image up front, so it has no swap prompts.

**The text screen.** Opcode `$3C` and the inventory switch to the Apple text page
(`$56E6`); `$3D`/`$3E` then print on it through the ROM, and `$3F` returns to
graphics. The game turns
on the IIe's alternate character set (`$C00F`) and sends plain ASCII (high bit
clear), so everything it prints is stored as an inverse code: black text on a white
cell. The output filter (`$5782`) masks upper case (`$41`-`$60`) to `$01`-`$20`,
because `$40`-`$5F` would be MouseText in the alternate set. The page is cleared
(`$571A`) by printing 960 spaces, inverse and so white; the last one wraps past the
bottom line, and the ROM's scroll clears that line with normal (black) spaces.
Checked against the original in refemu (`REFEMU_TEXTDUMP`): after "speak king" in
room 53, the port's text page holds the same screen codes in every printed cell. (The
bottom line's clear is from the ROM listing: refemu's stand-in for COUT doesn't wrap
or scroll, so it can't show it.) The port stores these screen codes and draws them
as the IIe does, with the game's font. The
exact Apple text-mode character set needs the Apple's video ROM, which the port
doesn't ship. Given a dump (`kqplay --charrom FILE`: the IIe video ROM, 342-0133 or
342-0265), the text screen uses it; `CharRom::from_rom` (src/engine/display.rs) finds
the dump's dot order and polarity from the space and the L (the upright stands at
the left end of the foot; glyphs have a blank first column, so the upright is not
the leftmost dot). Checked with the enhanced IIe ROM (342-0265-A, 4 KB: normal glyphs
inverted, bit 0 the left dot): the letters read correctly, and its own inverse glyphs
are the normal ones inverted, so the port draws inverse that way. A ROM named
342-0265 or 342-0133 next to the disk images is used automatically. The II+ ROM
(341-0036) also reads, but has no lower case: the game needs a IIe anyway.

## Sound (confirmed)

The game beeps using the Apple ROM bell (`JSR $FBDD`, wrapped as `$6618`). Error
paths use a double beep with a pause (`$6625`, about 0.04 s): its callers are the disk
errors in save and restore (`$60AE`). The other callers of `$6618` are the "more" pause
(`$6DEF`), a disk read retry (`$4ADF`), and three fatal-error loops that ring forever
(`$47E6`, `$58E5`, `$65EB`). `src/sound.rs` rebuilds the sound from the ROM routine's
timing. That is the only sound found. An
earlier note said the game was silent, because no code touches the speaker
directly; that was wrong.

## Printing messages (`$56CA`, `$6DA5`, `$6DE9`)

`print` (opcode `$0A`) prints a message in three steps:

1. **Start of line.** If something has been typed, it starts a new line first.
   Then it moves to column 0.
2. **Line by line.** Messages contain explicit carriage returns, so they arrive
   already wrapped. Each line is printed and followed by a new line.
3. **Pausing.** `$1D` counts the lines printed since the last input. After a
   fourth line is printed, the engine beeps and waits for RETURN (or Esc) before
   the line break, and the count starts again.

After the message, `$1C` is set to 10 cycles. When it runs out, `$1D` is reset
(`$40E5`).

`print_text` (`$3E`) and `print_at` (`$3D`, row and column) print a message raw,
followed by a carriage return, with no pausing.

## The input line (`$52A8`, `$5312`, `$53C3`)

**The prompt.** It is `> ` followed by the block cursor (`$57B1`). It is redrawn
at the start of the next pass of the main loop: after a command (`$702C`), or
after a message (`$6EF0`).

**Keys.** Direction keys are checked first (see [motion.md](motion.md)), then
these:

| Key | Effect |
|---|---|
| Esc | pause until a key is pressed |
| `5` | types "restart game" and RETURN |
| `3` | types "restore game" and RETURN |
| Tab | inventory screen (`$45CF`) |
| *(input disabled)* | if `$6F05` is set (opcode `$2B`), every other key is ignored |
| `1` | types "save game" and RETURN |
| `=` | toggles `v31` |
| Ctrl-E | recall the last line |
| Ctrl-C | clear the line |
| Ctrl-N | recall the next character of the last line |
| Delete (`$7F`) | erase a character |
| Return | end the line: copy it to `$6F08` and flag it for the parser (`$6EFC`) |
| space, `A`–`$7E` | add to the line, up to 37 characters |

Digits and punctuation can't be typed, so `1`, `3` and `5` act as function keys.

## Parser (`$514B`)

1. **Reading words.** Words are taken from the saved line. Leading spaces are
   skipped, each character is ORed with `$20` (which lowercases letters), and a
   word is cut at 19 characters; the rest is read as the next word.
2. **Searching.** Each word is looked up in the vocabulary (fixed resource
   `$0800`). The list is sorted, and the search stops as soon as an entry sorts
   after the word. Skipping a phrase start also skips its next entry.
3. **Storing.** Type 3 sets the noun (`$9B`), type 4 is ignored, and anything
   else sets the verb (`$9A`). A later word overwrites an earlier one.
4. **Phrases.** After a single-word match, the search continues for phrase
   entries starting with the same word. If the next word completes a phrase,
   that phrase's id is stored and both words are used up. Otherwise the next
   word is read again on its own.
5. **Stopping.** Parsing stops when both a verb and a noun are set, or at the
   end of the line.
6. **Unknown words.** The verb becomes `$FF`. Unless `v5` is set, the engine
   prints `I don't understand "word".`

**One difference.** At the end of the vocabulary list the engine would keep
searching through the memory that follows it. The port treats the end of the
list as "not found".

**After parsing (`$4044`):**

- **Nothing usable** (no verb or noun, or an unknown word): if `v5` is set,
  `v28 = 1`. The parser rules are not run.
- **Otherwise:** the parser rules run, global first and then the room (`$408E`).
  Then `v28` is set and `v13 = (v13 + 1) mod 3`. The global script uses `v28`
  for its fallback replies:

| `v28` | Meaning | Reply (when `v5 = 0`) |
|---|---|---|
| 1 | no verb found | "I do not understand that." |
| 2 | a verb, but no rule matched | "You can't do that here." |
| 3 | a rule matched | none |

`$408E` also gives 3 when the verb is `$FF`, but an unknown word never gets there:
`$514B` then returns "nothing usable" (`$51DC`), so the rules don't run.

## Inventory (`$45CF`)

On the text screen it prints "You are now carrying:" on row 4. Below that it
lists each carried object (object byte 2, bit 7) among objects 0–110, in two
columns at 3 and 21, or "nothing" if there are none. Three rows further down it
prints "Your score is: N", then waits for a key.

**Object names.** The object table at `$1A80` is 111 eight-byte records followed
by the name strings. A name is at the address in bytes 0–1, minus 2. Examples
are "dagger", "pouch", "pouch of diamonds" and "carrot".

## Other prompts

- **Restart (`$43`).** It prints "Do you want to start over? (Y or N)". `Y`
  restarts: the engine goes back to `$4018`, which reloads the object table and
  clears all state. `N` continues.
- **`input_number` (`$0F`).** It reads digits, echoing them, until RETURN.

## Still open

- **The exact Apple text-mode characters**: only with a user-supplied ROM dump
  (above).
- **What `v31` controls** (toggled with `=`).
