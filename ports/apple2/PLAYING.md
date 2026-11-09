# King's Quest for the Apple IIe: playing

`kq` plays Sierra's 1984 Apple IIe *King's Quest* natively on Windows and Linux, from
your own copy of the game's disks. It plays and sounds like the original, at the
original's speed.

## 1. Get kq

Download the archive for your system from the
[releases page](https://github.com/Gl0ttis/kings-quest-native/releases) and unpack
it into a folder of its own (another port's `kq` would clash in the same one):

- Windows: `kq-apple2-<version>-windows-x64.zip`, containing `kq.exe`.
- Linux: `kq-apple2-<version>-linux-x64.tar.gz`, containing `kq`. It needs X11 (or XWayland)
  and ALSA, which desktop systems already have.

Nothing else needs installing.

## 2. Add your game disks

`kq` doesn't include the game. You need images of the three game disks of the Apple
II version:

- `.nib` files (232,960 bytes each). Any names work, because each image records which
  disk it is.
- or `.do`/`.dsk` files (143,360 bytes each). Their names must say which disk they
  are, such as `Disk 1`, `disk2` or `Side C`.

Put them in the same folder as `kq`:

```
King's Quest/
  kq.exe
  King's Quest (Disk 1 of 3).nib
  King's Quest (Disk 2 of 3).nib
  King's Quest (Disk 3 of 3).nib
```

## 3. Play

Double-click `kq.exe` (or run `./kq` on Linux).

You can also keep the disks somewhere else and point `kq` at them, from a terminal:

```
kq "D:\Games\Apple II\King's Quest"
kq disk1.nib disk2.nib disk3.nib
```

If `kq` can't find the disks, it says which ones are missing and where it looked.

## Keys

The game's keys are the Apple's:

| Key | Does |
|---|---|
| Arrow keys | Walk. Press the same arrow again to stop. |
| `'` `/` `.` `;` | Walk diagonally. |
| Typing, then Enter | Give a command, such as `open door` or `take dagger`. |
| Tab | Show what Graham is carrying. |
| Esc | Pause. |
| Ctrl+E | Bring back the last command. |
| `1` | Save the game. |
| `3` | Restore a saved game. |
| `5` | Restart (also after Graham dies). |

A joystick or game controller that is plugged in before `kq` starts works as the
Apple joystick.

These keys control `kq` itself:

| Key | Does |
|---|---|
| Alt+Enter | Full screen on or off. |
| F11 / F12 | Slower / faster. |
| F9 | Fast loads: skip the wait while a new room loads and draws. |
| F8 | Sound on or off. |
| F7 | NTSC colour fringing, like a composite monitor. |
| F6 | Fill the stray white specks the original leaves in some pictures. |

## Saved games

Saved games go in a `saves` folder next to the disks, one file per letter (`kq-a.sav`
to `kq-z.sav`).

## Options

Add these after `kq` when starting it from a terminal:

| Option | Does |
|---|---|
| `--fullscreen` | Start in full screen. |
| `--fast-loads` | Start with fast loads on. |
| `--speed N` | Play at N times the original speed, such as `--speed 2`. |
| `--mute` | Start with the sound off. |
| `--ntsc` | Start with colour fringing on. |
| `--fill-specks` | Start with the speck filling on. |
| `--scale N` | Starting window size, from 1 up (default 2). The window can also be resized. |
| `--saves DIR` | Keep saved games in DIR. |
| `--savedisk FILE` | Save to an Apple II save disk image instead (`.nib` or `.do`). Emulators can read the same image. |
| `--swap-prompts` | Show the original's "Please insert side B" prompts. |
| `--no-joystick` | Ignore any joystick. |
| `--charrom FILE` / `--no-charrom` | Use, or don't use, an Apple IIe character ROM for the text screens. |

## The Apple's own lettering (optional)

The inventory and other text screens use a built-in font. For the Apple's own, put a
dump of the Apple IIe video ROM next to the disks. Its name needs to contain
`342-0265` (the enhanced IIe) or `342-0133` (the original IIe), and `kq` uses it
automatically. No ROM is included with `kq`.

## Things that are as in the original

- When the troll blocks the bridge, Graham keeps walking on the spot until you stop
  him or turn. The original does the same.
- Some pictures have single white pixels where a fill missed a spot. F6 fills them.

## Licence

`kq` is under the MIT licence (`LICENSE`). *King's Quest* is Sierra's. `kq` contains
none of the game: it reads your copy.
