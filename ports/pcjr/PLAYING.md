# King's Quest for the IBM PCjr: playing

`kq-pcjr` plays Sierra's 1984 IBM PCjr *King's Quest* natively on Windows and Linux,
from your own copy of the game's disk. It plays and sounds like the original, at the
original's speed.

## 1. Get kq-pcjr

Download the archive for your system from the
[releases page](https://github.com/Gl0ttis/kings-quest-native/releases) and unpack
it anywhere:

- Windows: `kq-pcjr-windows-x64.zip`, containing `kq-pcjr.exe`.
- Linux: `kq-pcjr-linux-x64.tar.gz`, containing `kq-pcjr`. It needs X11 (or XWayland)
  and ALSA, which desktop systems already have.

Nothing else needs installing.

## 2. Add your game disk

`kq-pcjr` doesn't include the game. You need an image of the PCjr version's disk (the
one that boots, with "Sierra On-Line Inc 1984 / IBM Corp. 1984" on it):

- an HxC `.mfm` dump of the original diskette, copy protection and all; or
- a 360K sector image (`.img`, 368,640 bytes) of it.

Put it in the same folder as `kq-pcjr`:

```
King's Quest/
  kq-pcjr.exe
  King's Quest (PCjr).mfm
```

## 3. Play

Double-click `kq-pcjr.exe` (or run `./kq-pcjr` on Linux).

You can also keep the disk somewhere else and point `kq-pcjr` at it, from a terminal:

```
kq-pcjr "D:\Games\PCjr\King's Quest (PCjr).mfm"
```

## Keys

The game's keys are the PCjr's:

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

A joystick or game controller works as the PCjr's joystick: the stick (or the d-pad)
walks Graham while you hold it, button 2 jumps, and button 1 is the `-` key held down.

These keys control `kq-pcjr` itself:

| Key | Does |
|---|---|
| Alt+Enter | Full screen on or off. |
| F11 / F12 | Slower / faster. |
| F9 | Fast loads: skip the wait while a new room loads and draws. |
| F8 | Sound on or off. |

## Saved games

The game saves to its "Play diskette", which it makes itself: type `copy disk` before
your first save and answer `y` (when it asks you to put the Play or the Master
diskette in the drive, `kq-pcjr` does it for you: just press Enter). The
Play diskette is kept as `KQ1 Play.img` next to the game's disk. It's the original's
own format: an emulator running the original can read it, and `kq-pcjr` reads one
made by the original.

## Options

Add these after `kq-pcjr` when starting it from a terminal:

| Option | Does |
|---|---|
| `--fullscreen` | Start in full screen. |
| `--fast-loads` | Start with fast loads on. |
| `--speed N` | Play at N times the original speed, such as `--speed 2`. |
| `--mute` | Start with the sound off. |
| `--scale N` | Starting window size, N times 320 × 240 (default 3). The window can also be resized. |
| `--play FILE` | Keep the Play diskette in FILE instead. |
| `--no-joystick` | Ignore any joystick. |
| `--font FILE` / `--no-rom-font` | Use, or don't use, the PCjr's own font from a ROM image. |

## The PCjr's own lettering (optional)

The text uses a built-in font in the PCjr's style. For the PCjr's own, put an image
of the PCjr BIOS (64 KB, as in an emulator's ROM set) next to the game's disk as
`bios.rom`, and `kq-pcjr` uses it automatically. No ROM is included with `kq-pcjr`.

## Things that are as in the original

- Graham walks more slowly when other characters are on the screen: the PCjr took
  longer over each moment of the game with more to move and draw.
- A new room is drawn line by line and fill by fill, as the PCjr drew it. F9 skips it.

## Licence

`kq-pcjr` is under the MIT licence (`LICENSE`). *King's Quest* is Sierra's. `kq-pcjr`
contains none of the game: it reads your copy.
