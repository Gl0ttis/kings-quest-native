# Measuring the original in 86Box

86Box emulates the IBM PCjr with IBM's own BIOS. It is used here, like refemu-pcjr, only
to measure the original. Nothing from it is part of the repository, the BIOS least of
all.

## Set-up (local, not in git)

- **86Box.** The 86Box release for your system, unpacked anywhere.
- **The PCjr BIOS.** `roms/machines/ibmpcjr/bios.rom` from 86Box's ROM set, in the
  `roms` folder next to `86Box.exe`.
- **A VM folder** whose `86box.cfg` has:
  - `[Machine]`: `machine = ibmpcjr`, `cpu_family = 8088`, `cpu_speed = 4772728`,
    `mem_size = 128`;
  - `[Floppy and CD-ROM drives]`: `fdd_01_type = 525_2dd`, `fdd_01_fn = <the original's
    .86f>`, `fdd_01_writeprot = 1`.

## Measuring

- **`measure.ps1 -Dir <folder with app\ and vm\>`** runs 86Box with that VM. It:
  - presses Enter every 3 s from 8 s to 38 s (through the title screens; harmless in
    the game);
  - presses Up once at 45 s;
  - captures the window about 10 times a second to `frames\<ms>.png`.
- **`walkspeed.py <frames> 45300 49000`** fits Graham's walking speed from the frames.

## Results (86Box 6.0, the "[cp] [!]" dump)

- **Walking up in room 1:** 12.52 picture rows a second, one row a pass, so 79.9 ms a
  pass.
- **Room 1 to room 16:** Up pressed at 45.13 s. The screen freezes at about 51.75 s,
  when Graham leaves, and room 16 appears at 54.19 s: 9.1 s from the key, about 2.4 s
  of it loading and drawing.

**86Box adds no PCjr wait states for this game.** It applies the Video Gate Array's
wait states only to accesses through the B800 window, and the game draws into its pages
at 1000:0 and 1800:0 directly. So 86Box measures the 8088 alone. A real PCjr also slows
every access to its shared RAM (IBM: two wait states on average), so it would be
slower. No working PCjr is available, so **86Box is the timing reference** for the
port.
