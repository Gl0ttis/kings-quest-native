# Porting guide: adding a King's Quest port

This guide is for whoever starts the next port. The next one is the IBM PCjr release
(1984). It describes how the Apple IIe port was built and checked, what to set up in
the repository, the order of work that held up, and the mistakes worth avoiding. The
Apple port, in [ports/apple2/](../ports/apple2/README.md), is the worked example
throughout. When this guide says "the Apple port did X", the details are in its docs.

## What "done" means

A port is done when the original and the port, given the same key presses, do the same
thing on every main-loop pass, and this is checked by machine:

- **Same state.** The rooms, positions, score and a hash of all variables are compared
  on every pass, over several complete playthroughs.
- **Same screen.** The screen is compared on every pass, dot for dot.
- **Same speed.** Each pass lasts as long as it did on the original hardware.

The Apple port reached all three. Its seven recorded routes, about 68,000 passes in
all, match on every pass in state and on screen.

## Ground rules

These apply to every port. Don't relax them without the repository owner agreeing.

1. **No emulation in the port.** Don't emulate the CPU and don't translate the code
   instruction by instruction. Rewrite each engine subsystem from its documented
   behaviour. An emulator may exist only as a separate measuring tool (the Apple port's
   is `refemu`), never as part of what players run.
2. **Bring your own game.** The port reads the player's own disk images at run time.
   Never commit game content of any kind:
   - disk images;
   - decoded resources;
   - script listings or disassemblies;
   - screenshots of the game;
   - save files.

   `.gitignore` covers `ports/*/original/` and `ports/*/extracted/`. Check `git status`
   before every commit anyway.
3. **No ROM images.** This covers BIOS and character ROMs. Wherever the original calls
   firmware, write a substitute for that routine (refemu does this for the Apple's
   firmware). If a ROM improves the display, the player supplies it, and the port finds
   it next to the disks.
4. **The game logic runs unchanged.** The original's scripts run on a native
   interpreter. Nothing in the game is rewritten by hand.
5. **Faithful by default.** Reproduce the original's quirks, including its bugs, when
   they show on screen or affect play. When the original's behaviour is undefined or
   hardware-dependent, make the choice deliberately and record it in the port's plan
   under "Decisions". Fixes the original never had can be options, off by default (for
   example `--fill-specks`).
6. **What may be committed.** Numbers measured from the original (trace CSVs in
   `ports/<port>/reference/`) and key logs (`routes/*.keys`) contain no game content,
   so they are committed and used as test fixtures.

## Setting up the port in the repository

Use the Apple port as the template. For the PCjr port, `<port>` = `pcjr` and the
crate is `kq1-pcjr`.

1. **Crate.** Create `ports/<port>/` with its crate and add it to `members` in the root
   `Cargo.toml`. Keep the window and sound code behind an optional `player` feature, as
   the Apple port does, so the engine library has no dependencies and builds headless.
   - The player is `kq` to players: its release archive holds it as `kq` (`kq.exe`), so
     every port runs the same way. In the workspace it is `kq-<port>` (the Cargo
     `[[bin]]` name), since two packages' binaries can't share a name there; the release
     workflow renames it. Its archives are named `kq-<port>-<version>-<platform>`, with
     the crate's version.
   - Name the measuring emulator after the port's own crate, e.g. `kq1-pcjr-refemu`, in
     `ports/<port>/refemu/`.
2. **Releases.** Add the crates to `release-plz.toml`, as the Apple port's are:
   - the port with `changelog_include` naming the shared crates it uses, so that its
     changelog lists their changes;
   - refemu with `release = false`. It's a tool that is never published, so it needs no
     versions or releases.

   release-plz finds a package's last release by its tag (`<crate>-v<version>`). A new
   port has none, so its first release is the version in its `Cargo.toml`: start it at
   0.1.0, and it is released by the first push to `main` that has it.
3. **Commit scopes.** Add the port's scope (`pcjr`) to `.commitlintrc.json` and
   `.github/workflows/pr-title.yml`.
4. **CI.** `.github/workflows/ci.yml` builds and tests the whole workspace, so a new
   crate is covered automatically. Its last step tests the Apple engine library without
   default features; add the same step for the new crate.
5. **Release workflow.** `.github/workflows/release.yml` has a build job for each port:
   - the `Released ports` step gives each port's tag when it was released (add the
     port to its loop);
   - the job builds `-p kq1-<port>` and packages `ports/<port>/PLAYING.md`.

   Copy the PCjr's job, or generalise them with a port dimension in the matrix.
   The Release run on `main` must stay green either way. A mistake there blocks
   releases for every port.
6. **Docs.** Add the port to the "Ports" table in the root README. Write
   `ports/<port>/README.md` for developers, `ports/<port>/PLAYING.md` for players
   (it ships in the archive), and `ports/<port>/docs/` with a `port-plan.md` modelled on
   the Apple one.

Follow [CONTRIBUTING.md](../CONTRIBUTING.md) for commits, pull requests and branch
protection. Changes go through pull requests with conventional titles. `main` requires
five checks. Release pull requests run them too, and merge themselves once they pass.

## The order of work

The Apple port went through these phases. Each phase starts with a short
reverse-engineering pass on its unknowns and ends with a doc in `ports/<port>/docs/`.
Write things down as you go. The docs are what made the later phases fast, and they
are what the tests cite.

### 1. Disk images

Decode the player's disk images, check every sector, and refuse damaged ones with a
clear message.

- **Apple:** `.nib` images (raw GCR nibbles), checked sector by sector by `nibdecode`
  and `nib.rs`. Each game disk records its number as its DOS volume (1–3), and the save
  disk is volume 10. That is what lets `kq` find disks by content, not by name
  (`Disks::find`).
- **Before writing any code:** find out what image formats people actually have for the
  new platform, and whether the original was copy-protected. If it was, sector images
  may lose information, and you may need a flux-level format.

### 2. The disk layout

Find the boot chain, the engine code, the resource directories and the addressing
scheme. Write [disk-layout.md](../ports/apple2/docs/disk-layout.md)'s equivalent.

Check two things:
- that every directory entry decodes and no resources overlap;
- that the counts add up (the Apple disks hold 259 resources).

### 3. The static formats

Write a reader for each resource type, with a tool that renders or lists all of them:
- pictures (`kqpic`);
- sprites/views (`kqview`);
- scripts and vocabulary (`kqscript`);
- each room entered through the engine (`kqroom`).

Every resource must decode without warnings before you move on. Tools write to
`extracted/` (git-ignored). Keep the readers as library code: the engine uses them.

### 4. The measuring tool (refemu), early

Build the reference emulator before the engine, not after. The Apple port built it in
milestone 6, and several "done" subsystems had to be revisited once it could measure
them.

What it must do:
- **Boot the original from the player's images.** Substitute the firmware calls the
  game makes, so no ROM is needed. Anything unsubstituted should stop the run with a
  note saying what was called.
- **Find the main loop's entry address.** A "pass" is one run of the main loop, and the
  tool counts passes there. This is the unit every comparison uses.
- **Take input from a key log**: lines of `PASS KEY-HEX`, each key pressed before that
  pass. The same file drives the port.
- **Write a trace CSV**, one row per pass: room, ego x/y, direction, score, a hash of
  all variables, and cycles for the pass. Keep the column order of
  `ports/apple2/reference/*.csv` so the tools and tests carry over.
- **Record the screen** (`--frames`) once the comparison of phase 8 exists.
- **Debug switches**, each added when a bug needed it. In refemu they are environment
  variables:
  - dump the variables and actor table at given passes;
  - log every random-number call;
  - watch memory bytes and report which instruction changes them;
  - log the cycle count at given code addresses;
  - log each picture plot.

It must be cycle-exact enough to measure speed. The Apple port's speed model was fitted
to refemu's cycle counts.

### 5. The engine, headless

Build the engine as a library that runs without a window:
- **Plain-data state.** The world state is plain data: variables, objects, actors,
  picture buffer. A save is then a copy of it, and tests can inspect it.
- **Raw-byte VM.** The script VM runs on the raw script bytes, not a parsed tree. The
  original's rule matcher skips by scanning bytes, and the scripts depend on that.
- **Exhaustive opcodes.** One dispatcher with an arm per opcode. An unknown opcode is
  logged, never silently ignored.
- **Deterministic.** The same images and the same key log give the same run, every
  time.
- **I/O behind a trait.** `IoImpl` handles keys, waits, sound, saves and pauses. A
  scripted implementation (`Scripted`) answers waits automatically for headless runs and
  tests.
- **Original addresses in comments.** Comment each function with the address it
  reimplements (`/// $4DD7: erase every on-screen actor.`). It makes every later
  comparison with the original's code quick.

### 6. Pass-by-pass comparison

This is the core loop of the whole project:
1. Replay a key log in refemu and in the port.
2. Diff the traces.
3. Find the first pass that differs.
4. Find out why, in the original's code.
5. Fix the port.
6. Repeat.

**Alignment.** Refemu's row for pass k+1 is the port's state after pass k (refemu
records at the start of a pass). Get this right once, put it in a helper, and reuse it.

**Routes.** Long comparisons need complete playthroughs. The Apple port built them this
way:
- a route file describes the plan;
- an autopilot (`kqroute`) plays it in the port and writes the key log;
- the key log is replayed in refemu.

Record more than one route, and compare saving and restoring the same way. The
gold-key route reached a room believed unreachable (room 64, with a data defect the
original shows). Comparing a restore in refemu found a carry difference in the port's
restore. The death routes cover dying and restarting.

Turn every matched route into a test that replays the key log against the reference
CSV. The tests skip themselves when the disks aren't present, so CI stays green without
game data.

### 7. Text, parser, save, restore, restart

Text output, the input line, the parser against the original vocabulary, the save
format (the port's own files first; the original's save disks later, as an option),
restart and death. Compare the text screens character by character with a dump from
refemu.

### 8. The screen, dot by dot

State traces don't see everything. On the Apple port they missed five screen
differences, found only by comparing frames:
- a background restored at the wrong place;
- a cel past the end of its loop;
- an animation frame the original resets;
- leftover title text;
- picture overlays shown a pass late.

Build the frame comparison (refemu `--frames` and the `framediff` example) as soon as
the engine draws. Don't leave it to the end as the Apple port did.

Compare in the hardware's own terms, not in RGB: double hi-res dots for the Apple, the
equivalent memory format for the PCjr. That way the comparison has no palette or
scaling noise.

Capture at the right instant. The port draws the prompt for the next pass at the end of
each pass. Refemu first captured before that redraw, which made the prompt look a pass
late everywhere. It was a measuring artefact, not a game difference. Know exactly what
"the screen after pass k" means on both sides.

### 9. Timing

The original has no speed control: a pass lasts as long as the CPU takes. The Apple
port recreates this with a cost model (`src/engine/cost.rs`):
- **Counted work:** the engine counts the work each pass does (rule bytes scanned,
  sprite nibbles decoded, pixels plotted, characters drawn, sectors read, room changes).
- **Fitted constants:** the constants come from fits against refemu's cycles per pass
  (`tools/portfit.py`, `tools/runfit.py`).
- **Start-up:** the start-up sequence uses measured phase times.
- **Drawing:** picture drawing is replayed progressively, as the original draws.
- **Options:** fast loads (F9) skips the disk and drawing time; `--speed` and `--rate`
  override the pace.

Measure first, then model. A fixed rate is a fine first step; the model came later.

### 10. The player

The player is a window with a picture of the right shape, plus:
- keyboard (with the original's keys);
- joystick;
- sound;
- full screen;
- resizing that keeps the picture's shape;
- disk discovery next to the executable;
- save files next to the disks.

Things that cost time on Windows:
- **Drag and resize.** Painting from a window-procedure hook, plus a timer, keeps the
  game drawing during drag and resize.
- **Alt+Enter.** It must be caught before it types a character.
- **DPI.** The process must be per-monitor DPI aware, or full screen lands in a corner.
- **Locks.** Never call into the engine while holding the shared lock. The Apple player
  deadlocked on Tab this way.

## Quirks to expect

Each of these was in the Apple original, and each one showed. Expect the same kinds in
the PCjr version. They come from the same design, and the PCjr version was the first.

- **Bytes read as data that aren't.** The random-number generator reads the engine's
  own code bytes, some of which are patched at run time (self-modifying code). Model
  the patches.
- **Off-by-one saves.** Leaving view 0, `set_ego_view` saved the byte after the loop
  instead of the cel. Graham's walk depends on it.
- **Wrong index registers.** `play_loop_once` means to clear `v8` but clears `v0`.
- **Flag and carry leaks.** A CPU flag left over from the caller reaches the random call
  in `new_room`, and differs between script calls and restore.
- **Positions read at use, not at save.** Erasing an actor restores its background at
  the actor's current position, not where it was saved.
- **Reading past a table.** A cel number one past the loop's end keeps the last cel on
  screen.
- **Data defects on the disk.** Script 064's header is off by one. The original shows
  the result ("o you're walking on clouds"), so the port does too.
- **Memory beyond a resource.** The parser can scan past a script into whatever was
  loaded next. The Apple port doesn't model this; it's recorded as a known
  simplification.

The working rule: when the port and the original disagree, the original is right, even
when it's wrong. Find the instruction responsible and cite its address in the code and
the docs.

## What carries over from the Apple port

**Reusable as is or nearly:**
- the workspace, CI, release and repository conventions;
- the key-log format and the routes;
- the trace columns;
- the comparison method and the tests' structure;
- the autopilot's approach;
- the cost-model approach;
- the player's platform side: `crates/kq1-frontend` (the Windows paint hook, full
  screen, the joystick as a raw reading, the `fatal()` message box), which the Apple
  and PCjr players both use; a player gives the hook its picture's size and shape and
  turns the joystick reading into its machine's. Its `player` module is the half the
  IBM PC and PCjr players share: the clocks for play, routes and --headless, the window
  and its keys, the sound output (the port gives its samples), finding the disk, and the
  Play diskette's handling. A player of that family keeps only its screen, sound, font
  and engine glue;
- the IBM PC and PCjr engines' shared parts: `crates/kq1-ibm` (the clock's types and
  the picture painter);
- the documentation layout.

**Probably reusable in idea, not code:**
- the script VM and the picture reader. The view, script and vocabulary readers
  are already shared: `crates/kq1-formats`, which the Apple and PCjr ports both use
  (the PCjr passes in its own script operand table), and so is the HxC MFM image
  decoder, which the PCjr and PC ports use. Move more into `crates/` only
  once a second port actually needs it, and keep every port's tests green while
  moving.

**Not reusable:**
- anything about the Apple's disk encoding, double hi-res, the 6502 or refemu's machine
  model.

## The IBM PCjr port: what to find out first

Nothing below has been checked against the PCjr release yet. Treat each item as a
question for the first reverse-engineering pass, and record the answers in
`ports/pcjr/docs/`.

- **The release and its disks.** It was Sierra's 1984 release for IBM, which the Apple
  version followed. Confirm:
  - which disks and versions exist;
  - their image formats;
  - whether the disks are self-booting;
  - whether they are copy-protected (decisive for phase 1).
- **The engine's relationship to the Apple one.** The Apple engine has 68 script
  opcodes, picture and view formats with priority, and a parser with a vocabulary. Check
  whether the PCjr engine is the same design in 8088 code: same opcodes, same resource
  formats, same scripts? If the scripts and resources match byte for byte, much of the
  Apple port's library carries over, and the Apple port's docs become a map. If not,
  they are still a guide to what to look for.
- **The hardware to model in the port:**
  - the PCjr's 16-colour graphics mode the game uses, and its pixel shape;
  - the sound chip (the PCjr has a three-voice tone generator plus noise, where the
    Apple only had a bell);
  - the keyboard and joystick;
  - the timer the game uses for pacing, if any.

  Find where the engine touches each of these, and record the addresses.
- **The measuring tool.** An 8088 PCjr emulator with the BIOS calls the game makes
  substituted, no IBM ROM. Cycle-exact 8088 timing is harder than the 6502: the prefetch
  queue, memory refresh, and the PCjr's video sharing system memory all cost cycles.
  Decide early how exact the speed model needs to be. If possible, check it against
  timings from real hardware or a cycle-accurate emulator.
- **Pacing.** The Apple engine has no timer: speed is how long a pass takes. The PCjr
  engine may use the system timer. If it does, the cost model may matter less, or
  differently. Measure before modelling.

## Reference: the Apple port's docs

| Doc | What it covers |
|---|---|
| [port-plan.md](../ports/apple2/docs/port-plan.md) | Goals, milestones, decisions, risks, what was checked and how |
| [disk-layout.md](../ports/apple2/docs/disk-layout.md) | Boot chain, addressing, directories |
| [picture-format.md](../ports/apple2/docs/picture-format.md) | Picture commands, fills, priority |
| [view-format.md](../ports/apple2/docs/view-format.md) | Sprites: loops, cels, encoding |
| [script-format.md](../ports/apple2/docs/script-format.md) | The script VM and every opcode, with handler addresses |
| [motion.md](../ports/apple2/docs/motion.md) | The game cycle, movement, collision, the RNG, drawing order, the joystick |
| [text-and-parser.md](../ports/apple2/docs/text-and-parser.md) | Text output, the input line, the parser, sound |
| [save-format.md](../ports/apple2/docs/save-format.md) | Saves, restore, the original save disk |
| [route.md](../ports/apple2/docs/route.md) | The routes, the autopilot and the comparison workflow |
| [refemu/README.md](../ports/apple2/refemu/README.md) | The measuring emulator and its switches |
