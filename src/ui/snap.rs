//! Windows 11 offers layouts to snap a window into while the pointer rests on
//! the button that spreads it over the screen. It knows where that button is
//! only as the window tells it, point by point, and over the button it is
//! Windows that follows the pointer, not egui: whether the pointer is over it,
//! and pressed on it, is heard here, for the row to draw and act on.
//!
//! Windows offers them only to a window whose frame is as deep at its sides as
//! at its top. winit leaves a pixel of frame at the top alone, for the desktop
//! to draw its line and shadow around: here there is one at every edge.
use std::sync::{Mutex, MutexGuard, PoisonError};

use eframe::egui;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{ClientToScreen, ScreenToClient},
    UI::{
        Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongW, GetWindowRect, HTMAXBUTTON, IsIconic, IsZoomed,
            NCCALCSIZE_PARAMS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
            SetWindowPos, TITLEBARINFOEX, WM_DPICHANGED, WM_GETTITLEBARINFOEX, WM_NCCALCSIZE,
            WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
            WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WS_CAPTION, WS_THICKFRAME,
        },
    },
};

use super::style::Held;

/// A stretch of the window in its own pixels, from the top left corner of what
/// it draws: from `left` and `top`, up to but not taking in `right` and `bottom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Pixels {
    /// None at all.
    pub const NONE: Self = Self { left: 0, top: 0, right: 0, bottom: 0 };

    /// Those a rect of points is drawn over, `scale` pixels to a point.
    pub fn of(rect: egui::Rect, scale: f32) -> Self {
        let at = |points: f32| (points * scale).round() as i32;
        Self {
            left: at(rect.left()),
            top: at(rect.top()),
            right: at(rect.right()),
            bottom: at(rect.bottom()),
        }
    }

    /// Whether the pixel at `x` and `y` is among them.
    pub fn holds(self, [x, y]: [i32; 2]) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
}

/// What the window's messages and the row know of one another.
struct Shared {
    /// Where the button is: nowhere until the row is drawn.
    button: Pixels,
    held: Held,
    /// Whether the button was clicked since the row last asked.
    clicked: bool,
    /// Whether Windows is to say when the pointer leaves the button.
    tracking: bool,
    /// The window's, to draw it again when what Windows says changes how the
    /// button looks.
    ctx: Option<egui::Context>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    button: Pixels::NONE,
    held: Held { over: false, down: false },
    clicked: false,
    tracking: false,
    ctx: None,
});

fn shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The window's own name for what is hooked into its messages.
const HOOK: usize = 1;

/// Hooks into the window's messages, once, as it opens.
pub fn hook(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
    shared().ctx = Some(cc.egui_ctx.clone());
    let window = handle.hwnd.get() as HWND;
    // SAFETY: the window is this one, made on this thread, and what hooks into it
    // unhooks itself as it goes.
    unsafe { SetWindowSubclass(window, Some(messages), HOOK, 0) };
    grow(window);
}

/// How deep the window's frame is at each edge, in pixels, while it is neither
/// spread over the screen nor filling it.
const FRAME: i32 = 1;

/// Whether the window has a frame of its own: none while it is spread over the
/// screen, where winit fits what it draws to the screen, nor while it fills it,
/// nor while it is put away.
fn framed(window: HWND) -> bool {
    let frame = WS_CAPTION | WS_THICKFRAME;
    // SAFETY: the window is this one.
    unsafe {
        IsZoomed(window) == 0
            && IsIconic(window) == 0
            && GetWindowLongW(window, GWL_STYLE) as u32 & frame == frame
    }
}

/// Where the window is on the screen, frame and all, if Windows says: its
/// left, top, right and bottom.
fn bounds(window: HWND) -> Option<[i32; 4]> {
    let mut rect = RECT::default();
    // SAFETY: the window is this one, and the rect is one to be written over.
    let read = unsafe { GetWindowRect(window, &mut rect) } != 0;
    read.then_some([rect.left, rect.top, rect.right, rect.bottom])
}

/// Makes the window as much larger as its frame takes, for what it draws to
/// be as large as winit made it: as it opens, and as winit sizes it again for
/// another screen.
fn grow(window: HWND) {
    if !framed(window) {
        return;
    }
    let Some([left, top, right, bottom]) = bounds(window) else { return };
    let wide = right - left + 2 * FRAME;
    let tall = bottom - top + 2 * FRAME;
    let keep = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
    // SAFETY: the window is this one.
    unsafe { SetWindowPos(window, std::ptr::null_mut(), 0, 0, wide, tall, keep) };
}

/// Where the button is now, in points, as the row draws it: or that, for now,
/// it is nowhere Windows is to know of.
pub fn place(ctx: &egui::Context, button: Option<egui::Rect>) {
    let scale = ctx.pixels_per_point();
    shared().button = button.map_or(Pixels::NONE, |button| Pixels::of(button, scale));
}

/// Whether the pointer is over the button, as Windows says, and pressed on it.
pub fn held() -> Held {
    shared().held
}

/// Whether the button was clicked since this was last asked.
pub fn clicked() -> bool {
    std::mem::take(&mut shared().clicked)
}

/// Says the button is held as it is now, and has the window drawn again if
/// that is not as it was.
fn hold(held: Held) {
    let mut shared = shared();
    let changed = shared.held != held;
    shared.held = held;
    let ctx = shared.ctx.clone().filter(|_| changed);
    drop(shared);
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

/// What the window hears before winit does. Windows asks how deep its frame
/// is, and where every point is: those over the button are told as its, and
/// what the pointer does there is heard here. The rest goes on to winit as ever.
unsafe extern "system" fn messages(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _hook: usize,
    _data: usize,
) -> LRESULT {
    let on_button = wparam == HTMAXBUTTON as WPARAM;
    match message {
        WM_NCCALCSIZE if wparam != 0 && framed(window) => {
            // SAFETY: as it says that it may, Windows has sent where the window is to be.
            let rect = unsafe { &mut (*(lparam as *mut NCCALCSIZE_PARAMS)).rgrc[0] };
            rect.left += FRAME;
            rect.top += FRAME;
            rect.right -= FRAME;
            rect.bottom -= FRAME;
            return 0;
        }
        // Only if winit sized the window again for the other screen: otherwise
        // it is as large as it was, frame and all.
        WM_DPICHANGED => {
            let before = bounds(window);
            // SAFETY: what Windows sent, passed on as it was sent.
            let done = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            if bounds(window) != before {
                grow(window);
            }
            return done;
        }
        WM_NCHITTEST if over(window, lparam) => return HTMAXBUTTON as LRESULT,
        // Windows asks where the button is to put its layouts under it.
        WM_GETTITLEBARINFOEX => {
            // SAFETY: what Windows sent, passed on as it was sent.
            let done = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            let button = shared().button;
            if button != Pixels::NONE {
                let mut corners = [(button.left, button.top), (button.right, button.bottom)]
                    .map(|(x, y)| POINT { x, y });
                // SAFETY: the window is this one, and the points are ones to be
                // written over. What is asked for is as Windows says it is.
                unsafe {
                    for corner in &mut corners {
                        ClientToScreen(window, corner);
                    }
                    let [min, max] = corners;
                    let info = &mut *(lparam as *mut TITLEBARINFOEX);
                    info.rgrect[3] = RECT { left: min.x, top: min.y, right: max.x, bottom: max.y };
                }
            }
            return done;
        }
        WM_NCMOUSEMOVE => {
            let down = on_button && shared().held.down;
            hold(Held { over: on_button, down });
            if on_button && !std::mem::replace(&mut shared().tracking, true) {
                let mut leave = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: window,
                    dwHoverTime: 0,
                };
                // SAFETY: what it is given is as large as it says, and the window is this one.
                unsafe { TrackMouseEvent(&mut leave) };
            }
        }
        WM_NCMOUSELEAVE => {
            shared().tracking = false;
            hold(Held::default());
        }
        // Pressed, it waits to be let go over it, as Windows' own does. Windows
        // is not to follow the pointer itself from there.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if on_button => {
            hold(Held { over: true, down: true });
            return 0;
        }
        WM_NCLBUTTONUP if on_button => {
            let down = shared().held.down;
            hold(Held { over: true, down: false });
            if down {
                let ctx = {
                    let mut shared = shared();
                    shared.clicked = true;
                    shared.ctx.clone()
                };
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            }
            return 0;
        }
        WM_NCDESTROY => {
            // SAFETY: it is this window that this was hooked into, by this name.
            unsafe { RemoveWindowSubclass(window, Some(messages), HOOK) };
        }
        _ => {}
    }
    // SAFETY: what Windows sent, passed on as it was sent.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

/// Whether the point on the screen in `lparam` is over the button. There is
/// none while the window is put away.
fn over(window: HWND, lparam: LPARAM) -> bool {
    // Each half of it is a signed number: a screen to the left of or above the
    // first one has points less than nought.
    let mut point =
        POINT { x: i32::from(lparam as u16 as i16), y: i32::from((lparam >> 16) as u16 as i16) };
    // SAFETY: the window is this one, and the point is one to be written over.
    let within = unsafe { IsIconic(window) == 0 && ScreenToClient(window, &mut point) != 0 };
    within && shared().button.holds([point.x, point.y])
}

#[cfg(test)]
#[path = "../../tests/unit/ui_snap.rs"]
mod tests;
