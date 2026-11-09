//! A hook in front of minifb's window procedure (Windows): Alt+Enter, and all painting,
//! including while the window is dragged or resized, when Windows runs its own loop and
//! the player's loop (and minifb) wait. Elsewhere the functions do nothing.

/// The picture the hook paints: its pixels' size, and the shape it is shown at (its
/// aspect ratio, `aspect.0` : `aspect.1`), centred with black bars.
#[derive(Clone, Copy, Debug)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub aspect: (u32, u32),
}

impl Picture {
    /// Where the picture goes in a `cw` × `ch` client area: the largest rectangle of its
    /// shape, centred (x, y, width, height).
    pub fn rect(&self, cw: i32, ch: i32) -> (i32, i32, i32, i32) {
        let (aw, ah) = (self.aspect.0 as i32, self.aspect.1 as i32);
        let (w, h) = if cw * ah > ch * aw { (ch * aw / ah, ch) } else { (cw, cw * ah / aw) };
        ((cw - w) / 2, (ch - h) / 2, w, h)
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use super::Picture;
    use std::{cell::RefCell, ffi::c_void};
    type Hwnd = *mut c_void;
    type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowLongPtrW(h: Hwnd, index: i32, value: isize) -> isize;
        fn CallWindowProcW(prev: isize, h: Hwnd, msg: u32, w: usize, l: isize) -> isize;
        fn SetTimer(h: Hwnd, id: usize, ms: u32, f: *const c_void) -> usize;
        fn KillTimer(h: Hwnd, id: usize) -> i32;
        fn GetClientRect(h: Hwnd, r: *mut Rect) -> i32;
        fn GetDC(h: Hwnd) -> *mut c_void;
        fn ReleaseDC(h: Hwnd, dc: *mut c_void) -> i32;
        fn FillRect(dc: *mut c_void, r: *const Rect, brush: *mut c_void) -> i32;
        fn ValidateRect(h: Hwnd, r: *const Rect) -> i32;
        fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
        fn GetDpiForWindow(h: Hwnd) -> u32;
        fn GetWindowRect(h: Hwnd, r: *mut Rect) -> i32;
        fn SetWindowPos(h: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn StretchDIBits(
            dc: *mut c_void,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            sx: i32,
            sy: i32,
            sw: i32,
            sh: i32,
            bits: *const c_void,
            info: *const BitmapInfoHeader,
            usage: u32,
            rop: u32,
        ) -> i32;
        fn GetStockObject(i: i32) -> *mut c_void;
        fn SetStretchBltMode(dc: *mut c_void, mode: i32) -> i32;
    }
    const GWLP_WNDPROC: i32 = -4;
    const WM_PAINT: u32 = 0x000F;
    const WM_ERASEBKGND: u32 = 0x0014;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSCHAR: u32 = 0x0106;
    const WM_TIMER: u32 = 0x0113;
    const WM_ENTERSIZEMOVE: u32 = 0x0231;
    const WM_EXITSIZEMOVE: u32 = 0x0232;
    const VK_RETURN: usize = 0x0D;
    const TIMER: usize = 0x4B51;

    struct Hook {
        prev: isize,
        picture: Picture,
        fill: Box<dyn FnMut(&mut [u32])>,
        alt_enter: bool,
        frame: Vec<u32>,
    }
    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Per-monitor DPI awareness (before any window exists).
    pub fn dpi_aware() {
        unsafe { SetProcessDpiAwarenessContext(-4) };
    }

    /// Make the client area `width` × `height` at 96 dpi, at the window's display scale.
    pub fn size_window(win: &minifb::Window, width: u32, height: u32) {
        let h = win.get_window_handle();
        unsafe {
            let dpi = GetDpiForWindow(h).max(96) as i32;
            let (mut wr, mut cr) = (Rect::default(), Rect::default());
            GetWindowRect(h, &mut wr);
            GetClientRect(h, &mut cr);
            let cw = width as i32 * dpi / 96;
            let ch = height as i32 * dpi / 96;
            let bw = (wr.right - wr.left) - cr.right;
            let bh = (wr.bottom - wr.top) - cr.bottom;
            SetWindowPos(h, std::ptr::null_mut(), 0, 0, cw + bw, ch + bh, 0x0002 | 0x0004);
            // NOMOVE | NOZORDER
        }
    }

    /// Put the hook in front of the window's procedure: from then on it paints `picture`,
    /// asking `fill` for its latest pixels (0RGB, `picture.width` × `picture.height`).
    pub fn install(win: &minifb::Window, picture: Picture, fill: Box<dyn FnMut(&mut [u32])>) {
        let h = win.get_window_handle();
        let prev = unsafe { SetWindowLongPtrW(h, GWLP_WNDPROC, proc as WndProc as usize as isize) };
        let frame = vec![0; picture.width * picture.height];
        HOOK.with(|k| *k.borrow_mut() = Some(Hook { prev, picture, fill, alt_enter: false, frame }));
    }

    /// Alt+Enter was pressed since the last call.
    pub fn take_alt_enter() -> bool {
        HOOK.with(|k| k.borrow_mut().as_mut().is_some_and(|h| std::mem::take(&mut h.alt_enter)))
    }

    unsafe extern "system" fn proc(h: Hwnd, msg: u32, wp: usize, lp: isize) -> isize {
        // SAFETY: Win32 calls on this thread's own window handle.
        unsafe {
            // The borrow ends before minifb's procedure runs: it can re-enter this one.
            let prev = HOOK.with(|k| k.borrow().as_ref().map_or(0, |k| k.prev));
            match msg {
                WM_SYSKEYDOWN if wp == VK_RETURN => {
                    if lp & (1 << 30) == 0 {
                        HOOK.with(|k| k.borrow_mut().as_mut().map(|k| k.alt_enter = true));
                    }
                    return 0;
                }
                // The paint covers the whole window, so no erase (it would flicker).
                WM_ERASEBKGND => return 1,
                // Alt+key characters: none go to the game (Alt+Enter is the player's).
                WM_SYSCHAR => return 0,
                // Windows runs its own loop while the window is dragged or resized; a timer
                // keeps the picture coming meanwhile.
                WM_ENTERSIZEMOVE => {
                    SetTimer(h, TIMER, 15, std::ptr::null());
                }
                WM_EXITSIZEMOVE => {
                    KillTimer(h, TIMER);
                }
                WM_TIMER if wp == TIMER => {
                    paint(h);
                    return 0;
                }
                // All painting is done here (minifb's own stretch misplaces the picture
                // in a full-screen window).
                WM_PAINT => {
                    paint(h);
                    ValidateRect(h, std::ptr::null());
                    return 0;
                }
                _ => {}
            }
            CallWindowProcW(prev, h, msg, wp, lp)
        }
    }

    /// Draw the latest frame at the picture's shape, with black bars, stretched in one step.
    fn paint(h: Hwnd) {
        // SAFETY: GDI calls on this thread's own window and device context; the frame
        // buffer outlives the call.
        HOOK.with(|k| unsafe {
            let mut k = k.borrow_mut();
            let Some(k) = k.as_mut() else { return };
            (k.fill)(&mut k.frame);
            let mut r = Rect::default();
            GetClientRect(h, &mut r);
            let (cw, ch) = (r.right, r.bottom);
            if cw <= 0 || ch <= 0 {
                return;
            }
            let (bw, bh) = (k.picture.width as i32, k.picture.height as i32);
            let (x, y, w, hh) = k.picture.rect(cw, ch);
            let dc = GetDC(h);
            SetStretchBltMode(dc, 3); // COLORONCOLOR: whole pixels, never blended
            let black = GetStockObject(4);
            for b in [
                Rect { left: 0, top: 0, right: cw, bottom: y },
                Rect { left: 0, top: y + hh, right: cw, bottom: ch },
                Rect { left: 0, top: 0, right: x, bottom: ch },
                Rect { left: x + w, top: 0, right: cw, bottom: ch },
            ] {
                if b.right > b.left && b.bottom > b.top {
                    FillRect(dc, &b, black);
                }
            }
            let info = BitmapInfoHeader {
                size: 40,
                width: bw,
                height: -bh,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                clr_used: 0,
                clr_important: 0,
            };
            StretchDIBits(dc, x, y, w, hh, 0, 0, bw, bh, k.frame.as_ptr() as *const c_void, &info, 0, 0x00CC_0020);
            ReleaseDC(h, dc);
        });
    }
}

#[cfg(not(windows))]
mod other {
    use super::Picture;
    pub fn dpi_aware() {}
    pub fn size_window(_win: &minifb::Window, _width: u32, _height: u32) {}
    pub fn install(_win: &minifb::Window, _picture: Picture, _fill: Box<dyn FnMut(&mut [u32])>) {}
    pub fn take_alt_enter() -> bool {
        false
    }
}
#[cfg(not(windows))]
pub use other::*;
