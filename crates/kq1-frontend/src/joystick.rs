//! The first joystick or game controller, as a raw reading each player turns into its
//! machine's: on Windows through winmm (any game controller), on Linux through the
//! joystick device (`/dev/input/js0`, opened at the first read and again if it goes
//! away). None without one.

/// A reading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stick {
    /// The two axes, 0-65535 across, about 32768 at rest. On Linux a hat (d-pad, axes 6
    /// and 7) that is pushed stands in for them.
    pub axes: (u32, u32),
    /// On Windows, the hat (d-pad) when it is pushed: its direction in degrees,
    /// clockwise from up.
    pub hat: Option<f64>,
    /// Buttons 1 and 2.
    pub buttons: [bool; 2],
}

#[cfg(windows)]
pub fn read() -> Option<Stick> {
    #[repr(C)]
    #[derive(Default)]
    struct JoyInfoEx {
        size: u32,
        flags: u32,
        x: u32,
        y: u32,
        z: u32,
        r: u32,
        u: u32,
        v: u32,
        buttons: u32,
        button_number: u32,
        pov: u32,
        reserved1: u32,
        reserved2: u32,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn joyGetPosEx(id: u32, info: *mut JoyInfoEx) -> u32;
    }
    let mut j = JoyInfoEx { size: std::mem::size_of::<JoyInfoEx>() as u32, flags: 0xFF, ..Default::default() };
    // SAFETY: `j` is a JOYINFOEX of the size it says, alive for the call.
    if unsafe { joyGetPosEx(0, &mut j) } != 0 {
        return None;
    }
    Some(Stick {
        axes: (j.x.min(65535), j.y.min(65535)),
        hat: (j.pov != 0xFFFF).then(|| j.pov as f64 / 100.0),
        buttons: [j.buttons & 1 != 0, j.buttons & 2 != 0],
    })
}

#[cfg(target_os = "linux")]
pub fn read() -> Option<Stick> {
    use std::{fs::File, io::Read, os::unix::fs::OpenOptionsExt, sync::Mutex};
    struct State {
        dev: Option<File>,
        axes: [i16; 8],
        buttons: [bool; 2],
    }
    static STATE: Mutex<State> = Mutex::new(State { dev: None, axes: [0; 8], buttons: [false; 2] });
    /// O_NONBLOCK.
    const NONBLOCK: i32 = 0o4000;
    let mut st = STATE.lock().unwrap();
    if st.dev.is_none() {
        st.dev = std::fs::OpenOptions::new().read(true).custom_flags(NONBLOCK).open("/dev/input/js0").ok();
    }
    let dev = st.dev.as_mut()?;
    // struct js_event { u32 time; i16 value; u8 type; u8 number }, 8 bytes each.
    let mut buf = [0u8; 8 * 64];
    let mut events = Vec::new();
    loop {
        match dev.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                events.extend(buf[..n - n % 8].chunks(8).map(|e| (i16::from_le_bytes([e[4], e[5]]), e[6] & 0x7F, e[7])))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                st.dev = None;
                return None;
            }
        }
    }
    for (value, kind, number) in events {
        match kind {
            1 if (number as usize) < 2 => st.buttons[number as usize] = value != 0,
            2 if (number as usize) < 8 => st.axes[number as usize] = value,
            _ => {}
        }
    }
    let across = |v: i16| (v as i32 + 32768) as u32;
    let (hx, hy) = (st.axes[6], st.axes[7]);
    let (x, y) = if hx != 0 || hy != 0 { (hx, hy) } else { (st.axes[0], st.axes[1]) };
    Some(Stick { axes: (across(x), across(y)), hat: None, buttons: st.buttons })
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn read() -> Option<Stick> {
    None
}
