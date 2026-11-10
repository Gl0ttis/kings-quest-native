# Measuring the original in 86Box

86Box emulates the IBM PC 5150 with the IBM CGA, and its CGA has a composite monitor's
output. It is used here only to measure the original. Nothing from it is part of the
repository, the BIOS least of all.

## Set-up (local, not in git)

- **86Box.** The 86Box release for your system, unpacked into a folder `app\`.
- **ROMs**, from 86Box's ROM set, in `app\roms\`:
  - the PC's BIOS: `machines/ibmpc82/BIOS_5150_27OCT82_1501476_U33.BIN` (IBM's BIOS of
    10/27/82);
  - the CGA's characters: `video/mda/mda.rom`.
- **A VM folder** whose `86box.cfg` has:
  - `[Machine]`: `machine = ibmpc82`, `cpu_family = 8088`, `cpu_speed = 4772728`,
    `mem_size = 256`;
  - `[Video]`: `gfxcard = cga`;
  - `[IBM CGA]`: `display_type = 1` (composite), `composite_type = 0` (the old CGA),
    `snow_enabled = 0`;
  - `[Floppy and CD-ROM drives]`: `fdd_01_type = 525_2dd`, `fdd_01_fn = wp://kq.86f`
    (the original's `.86f`, copied into the VM folder), `fdd_01_writeprot = 1`.

The original boots with its protection, as on a PC: the Sierra logo after about 35 s
(IBM's BIOS tests memory first), the title, then room 1.

## Composite colour

- **`shots.ps1 -Dir <folder with app\> -Vm <VM folder>`** runs 86Box, presses Enter
  through the title screens, and takes 86Box's own screenshot every 4 s into the VM's
  `screenshots\` folder: 960 × 600, each of the CGA's 640 dots 1.5 pixels wide, each
  line 3 tall.
- **`../compositefit.py <room 1's screenshot> extracted/res/room/001.visual.bin`**
  finds the picture in the shot, takes 86Box's colour of each picture colour from the
  middle of its areas, and fits the composite model's three numbers to them.

## Timing

- **`measure.ps1 -Dir <folder with app\> -Vm <VM folder> -Out <frames>`** runs 86Box,
  presses Enter through the title screens, Up once at 75 s, and captures its window
  about 10 times a second from 70 s, each frame named by its time in milliseconds.
- **`walkspeed.py <frames> 75400 79300`** fits Graham's walking speed from the frames.

The results, against refemu-pc's, are in [../../refemu/README.md](../../refemu/README.md)
("Timing"): with the original, walking up in room 1 at 18.1 picture rows a second (one
pass a timer tick), room 16 cleared 6.51 s after Up and its picture drawn in 2.52 s.

## Results (86Box 6.0, the "[cp] [!]" dump, the old CGA)

- **Room 1:** eight of the 16 patterns appear in areas big enough to measure. The model
  ([src/composite.rs](../../src/composite.rs)) fits them with hue 322.5°, saturation
  0.58 and the foreground's level 0.62, within 5.7/255 RMS.
- **Over the whole picture**, edges included, both of the port's modes are within
  about 9/255 of 86Box on average (flat areas 6/255). The signal mode's fringes are
  where 86Box shows them, a little stronger.
