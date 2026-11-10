//! The platform side the native players share, so each port's player keeps only what is
//! its own (the engine thread, its screen, its keys, its sound):
//! - [`hook`]: on Windows, a hook in front of minifb's window procedure that paints the
//!   picture at its shape, also while the window is dragged, and catches Alt+Enter;
//! - [`fullscreen`]: borderless full screen and back (Windows, X11);
//! - [`joystick`]: the first joystick or game controller, as a raw reading;
//! - [`fatal`]: an error before the window opens, in a message box on Windows.

pub mod fullscreen;
pub mod hook;
pub mod joystick;

/// Report an error that stops a player before its window opens, and exit. On Windows it is
/// shown in a message box too, since a player is often started from Explorer, whose console
/// window closes at once.
pub fn fatal(caption: &str, msg: &str) -> ! {
    eprintln!("{msg}");
    // The caption is the message box's, so only Windows uses it.
    #[cfg(not(windows))]
    let _ = caption;
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, caption) = (wide(msg), wide(caption));
        // SAFETY: both strings are NUL-terminated and outlive the call. 0x10: MB_ICONERROR.
        unsafe { MessageBoxW(0, text.as_ptr(), caption.as_ptr(), 0x10) };
    }
    std::process::exit(1)
}
