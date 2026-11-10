# King's Quest for the IBM PC: playing

`kq` plays Sierra's 1984 IBM PC *King's Quest* natively on Windows and Linux, from
your own copy of the game's disk: the colour graphics as a composite monitor showed
them, the PC speaker's music, at the original's speed.

## 1. Get kq

Download the archive for your system from the
[releases page](https://github.com/Gl0ttis/kings-quest-native/releases) and unpack
it into a folder of its own (another port's `kq` would clash in the same one):

- Windows: `kq-pc-<version>-windows-x64.zip`, containing `kq.exe`.
- Linux: `kq-pc-<version>-linux-x64.tar.gz`, containing `kq`. It needs X11 (or XWayland)
  and ALSA, which desktop systems already have.

Nothing else needs installing.

## 2. Add your game disk

`kq` doesn't include the game. You need an image of the 1984 IBM PC version's disk,
either edition:

- the first release, for the Color Graphics Adapter on a composite monitor or TV; or
- the second (August 1984, "BOOT v1.2"), which offers composite or RGB colour, with
  three script bugs fixed.

Either as an HxC `.mfm` dump of the original diskette, copy protection and all, or a
360K sector image (`.img`, 368,640 bytes) of it.

Put the disk image in the same folder as `kq`:

```
King's Quest/
  kq.exe
  King's Quest (IBM PC).mfm
```

## 3. Play

Double-click `kq.exe` (or run `./kq` on Linux).

You can also keep the disk somewhere else and point `kq` at it, from a terminal:

```
kq "D:\Games\PC\King's Quest (IBM PC).mfm"
```

## Keys

The game's keys are the PC's:

| Key | Does |
|---|---|
| Arrow keys, Home, PgUp, End, PgDn | Walk (Home, PgUp, End and PgDn diagonally). Press the same key again to stop. |
| Typing, then Enter | Give a command, such as `open door` or `take dagger`. |
| Tab | Show what Graham is carrying, and the score. |
| `0` | Jump. |
| Esc | Pause. |
| `3` | Save the game. |
| `5` | Restore a saved game. |
| `7` | Restart. |
| `9` | Bring back the last command. |
| `1` | Sound on or off (the game's own switch). |
| Ctrl+Left / Ctrl+Right | Move the picture a little left or right (the game's way of centring it on a monitor). |

A joystick or game controller works as the PC's joystick: the stick (or the d-pad)
walks Graham while you hold it, button 2 jumps, and button 1 is the `-` key held down
(on a PlayStation controller, Cross and Square). Leave the stick at rest as the game
starts: the game takes its centre then, as the original did. On Windows it is the
first controller listed in "Set up USB game controllers" (`joy.cpl`), where Advanced
picks which.

These keys control `kq` itself:

| Key | Does |
|---|---|
| Alt+Enter | Full screen on or off. |
| F11 / F12 | Slower / faster. |
| F9 | Fast loads: skip the wait while a new room loads and draws. |
| F8 | Sound on or off. |
| F7 | The colours' fringes on or off (see below). |

## The colours

The game drew its pictures in black and white dots, and a composite monitor turned the
patterns of dots into colours. `kq` shows them two ways:

- **Plain** (the default): each colour solid, as the game's artists meant them.
- **With fringes** (F7, or `--signal`): as a composite monitor shows the signal, with
  the soft coloured fringes where colours meet.

The text is always shown as the monitor showed it, plain light letters: the monitor's
colour was too soft to colour such thin strokes. The inventory screen is text in the
PC's colours.

The second edition asks at its start which colours you want: press Esc to switch
between composite and RGB, and the space bar to play. RGB is the CGA on an RGB monitor:
green, red and brown on blue, the colours mixed by dithering. The game itself plays a
little differently in RGB, as the original did: it looks at the colour under Graham's
feet, and in RGB those colours are different.

## Saved games

The game saves to its "Play diskette", which it makes itself: type `copy disk` before
your first save and answer `y` (when it asks you to put the Play or the Master
diskette in the drive, `kq` does it for you: just press Enter). The Play diskette is
kept as `KQ1 Play.img` next to the game's disk. When you start `kq` again, it is
already in the drive, so `restore game` finds your saves. It's the original's own format: an
emulator running the original can read it, and `kq` reads one made by the original.

## Options

Add these after `kq` when starting it from a terminal:

| Option | Does |
|---|---|
| `--fullscreen` | Start in full screen. |
| `--fast-loads` | Start with fast loads on. |
| `--speed N` | Play at N times the original speed, such as `--speed 2`. |
| `--mute` | Start with the sound off. |
| `--signal` | Start with the colours' fringes on. |
| `--scale N` | Starting window size, N times 320 × 240 (default 3). The window can also be resized. |
| `--play FILE` | Keep the Play diskette in FILE instead. |
| `--rate N` | A fixed N moments of the game a second (Graham moves a step each), instead of the original's speed. |
| `--room N` | Start in room N (1-80), skipping the title, to look round. |
| `--no-joystick` | Ignore any joystick. |
| `--bios FILE` | Take the PC's own font from this BIOS image. |

## The PC's own lettering (optional)

The text uses a built-in font in the same style. For the IBM PC's own, put an image
of the IBM PC's BIOS ROM (8 KB, or 64 KB, as in an emulator's ROM set) next to the
game's disk as `bios.rom`, and `kq` uses it automatically. No ROM is included with
`kq`.

## Things that are as in the original

- Graham takes a step at most every tick of the PC's timer, 18 times a second, as on
  the PC.
- A new room is drawn line by line and fill by fill, on the screen, as the PC drew it.
  F9 skips it.

## Licence

`kq` is under the MIT licence (`LICENSE`). *King's Quest* is Sierra's. `kq` contains
none of the game: it reads your copy.
