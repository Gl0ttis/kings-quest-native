//! Borderless full screen on the window's monitor, and back: on Windows by restyling the
//! window over the monitor's whole area, on Linux by asking the X11 window manager.

use minifb::Window;

/// Borderless full screen on the window's monitor (Alt+Enter), and back.
#[derive(Default)]
pub struct Fullscreen {
    #[cfg(windows)]
    saved: Option<(isize, win32::Rect)>,
    #[cfg(target_os = "linux")]
    on: bool,
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    #[repr(C)]
    pub struct MonitorInfo {
        pub size: u32,
        pub monitor: Rect,
        pub work: Rect,
        pub flags: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        pub fn GetWindowLongPtrW(h: *mut c_void, index: i32) -> isize;
        pub fn SetWindowLongPtrW(h: *mut c_void, index: i32, value: isize) -> isize;
        pub fn GetWindowRect(h: *mut c_void, r: *mut Rect) -> i32;
        pub fn MonitorFromWindow(h: *mut c_void, flags: u32) -> *mut c_void;
        pub fn GetMonitorInfoW(m: *mut c_void, info: *mut MonitorInfo) -> i32;
        pub fn SetWindowPos(h: *mut c_void, after: *mut c_void, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    pub const GWL_STYLE: i32 = -16;
    pub const WS_OVERLAPPEDWINDOW: isize = 0x00CF_0000;
    pub const WS_POPUP: isize = 0x8000_0000;
    pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_FRAMECHANGED: u32 = 0x0020;
    pub const SWP_NOOWNERZORDER: u32 = 0x0200;
}

impl Fullscreen {
    #[cfg(windows)]
    pub fn toggle(&mut self, win: &mut Window) {
        use win32::*;
        let h = win.get_window_handle();
        unsafe {
            match self.saved.take() {
                None => {
                    let style = GetWindowLongPtrW(h, GWL_STYLE);
                    let mut r = Rect::default();
                    GetWindowRect(h, &mut r);
                    let mut mi = MonitorInfo {
                        size: std::mem::size_of::<MonitorInfo>() as u32,
                        monitor: Rect::default(),
                        work: Rect::default(),
                        flags: 0,
                    };
                    if GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
                        return;
                    }
                    SetWindowLongPtrW(h, GWL_STYLE, (style & !WS_OVERLAPPEDWINDOW) | WS_POPUP);
                    let m = mi.monitor;
                    SetWindowPos(
                        h,
                        std::ptr::null_mut(),
                        m.left,
                        m.top,
                        m.right - m.left,
                        m.bottom - m.top,
                        SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                    self.saved = Some((style, r));
                    win.set_cursor_visibility(false);
                }
                Some((style, r)) => {
                    SetWindowLongPtrW(h, GWL_STYLE, style);
                    SetWindowPos(
                        h,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                    );
                    win.set_cursor_visibility(true);
                }
            }
        }
    }
    /// X11: ask the window manager to toggle `_NET_WM_STATE_FULLSCREEN`; minifb then
    /// stretches the picture to the new size, keeping its shape.
    #[cfg(target_os = "linux")]
    pub fn toggle(&mut self, win: &mut Window) {
        use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
        use x11_dl::xlib;
        let (Ok(dh), Ok(wh)) = (win.display_handle(), win.window_handle()) else { return };
        let (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(w)) = (dh.as_raw(), wh.as_raw()) else {
            println!("full screen needs X11");
            return;
        };
        let Some(display) = d.display else { return };
        let Ok(x) = xlib::Xlib::open() else { return };
        let display = display.as_ptr() as *mut xlib::Display;
        // SAFETY: the display and window are minifb's own, alive while `win` is.
        unsafe {
            let state = (x.XInternAtom)(display, c"_NET_WM_STATE".as_ptr(), xlib::False);
            let full = (x.XInternAtom)(display, c"_NET_WM_STATE_FULLSCREEN".as_ptr(), xlib::False);
            let mut ev: xlib::XEvent = std::mem::zeroed();
            ev.client_message.type_ = xlib::ClientMessage;
            ev.client_message.window = w.window;
            ev.client_message.message_type = state;
            ev.client_message.format = 32;
            ev.client_message.data.set_long(0, 2); // _NET_WM_STATE_TOGGLE
            ev.client_message.data.set_long(1, full as i64);
            ev.client_message.data.set_long(3, 1);
            let root = (x.XDefaultRootWindow)(display);
            (x.XSendEvent)(
                display,
                root,
                xlib::False,
                xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
                &mut ev,
            );
            (x.XFlush)(display);
        }
        self.on = !self.on;
        win.set_cursor_visibility(!self.on);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    pub fn toggle(&mut self, _win: &mut Window) {
        println!("full screen is only implemented on Windows and Linux (X11)");
    }
}
