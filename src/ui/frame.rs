//! The window's own frame, in place of the desktop's: its top row moves it, its
//! edges size it, and three buttons in that row put it away, spread it over the
//! screen and close it. On Windows the desktop still draws its shadow, its line
//! and its round corners, and the buttons are drawn as Windows draws its own.
use eframe::egui::{
    self, Align, CursorIcon, Layout, PointerButton, Rect, ResizeDirection, Sense, Stroke,
    StrokeKind, UiBuilder, Vec2, ViewportCommand,
};

use super::style::{CARD_RADIUS, EDGE_LINE, GUTTER, Icon, round_button};
#[cfg(windows)]
use super::{
    snap,
    style::{Caption, Held, caption_button},
};

/// How near an edge of the window the pointer sizes it, and how near a corner.
const EDGE: f32 = 5.0;
const CORNER: f32 = 10.0;

/// Whether the window fills its screen, with or without the desktop's own bars.
fn spread(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let window = i.viewport();
        window.maximized == Some(true) || window.fullscreen == Some(true)
    })
}

/// How round the window's corners are: as round as lets a card's corner lie
/// evenly within, a gutter in. A window that fills its screen has square ones.
/// On Windows the desktop rounds them, and the window is drawn square.
fn corners(ctx: &egui::Context) -> f32 {
    if cfg!(windows) {
        return 0.0;
    }
    match spread(ctx) {
        true => 0.0,
        false => f32::from(CARD_RADIUS) + GUTTER,
    }
}

/// How much of the ground's colour lies over Mica, where it shows through: as
/// much as keeps the ground the terminals' hue, and what is written on it plain.
pub const TINT: f32 = 0.8;

/// The window's ground, which everything in it lies on. Where Mica shows
/// through, it is a tint over it.
pub fn ground(ui: &egui::Ui, fill: egui::Color32, mica: bool) {
    let ctx = ui.ctx();
    let fill = if mica { fill.gamma_multiply(TINT) } else { fill };
    ui.painter().rect_filled(ctx.content_rect(), corners(ctx), fill);
}

/// A row of controls along the top of the window. Where none of them is under
/// the pointer it is the window's handle: a drag moves the window, and two
/// clicks spread it over the screen or bring it back. The controls are laid
/// out after it, and so have the pointer first.
pub fn titlebar(ui: &mut egui::Ui, height: f32, inset: f32) -> egui::Ui {
    let size = Vec2::new(ui.available_width(), height);
    let (rect, handle) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let ctx = ui.ctx();
    if handle.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!spread(ctx)));
    } else if handle.drag_started_by(PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let rect = rect.shrink2(egui::vec2(inset, 0.0));
    let layout = Layout::left_to_right(Align::Center);
    let mut bar = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    bar.spacing_mut().item_spacing.x = 2.0;
    bar
}

/// On Windows the desktop draws the window's line and shadow in its dark
/// colours, and rounds its corners as it does those of its own windows.
#[cfg(windows)]
pub fn dress(window: &impl raw_window_handle::HasWindowHandle) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    let Some(window) = hwnd(window) else { return };
    let (dark, round): (i32, i32) = (1, DWMWCP_ROUND);
    // SAFETY: the window is this one, and each value is as large as it is said to
    // be. Windows before 11 rounds no corners, and leaves them as they were.
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            4,
        );
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const round).cast(),
            4,
        );
    }
}

/// The window as Windows knows it.
#[cfg(windows)]
fn hwnd(
    window: &impl raw_window_handle::HasWindowHandle,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::RawWindowHandle;
    let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as windows_sys::Win32::Foundation::HWND)
}

/// Whether Windows can draw Mica behind a window: from Windows 11's 2022
/// update, the first to have it be asked for. Windows is asked once.
#[cfg(windows)]
pub fn backdrops() -> bool {
    static BACKDROPS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *BACKDROPS.get_or_init(build)
}

/// Whether Windows is of a build that draws Mica.
#[cfg(windows)]
fn build() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    const FIRST: u32 = 22621;
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let mut build = [0u16; 16];
    let mut size = size_of_val(&build) as u32;
    // SAFETY: the key and the name are strings that end in a nul, and what is
    // read is no larger than it is said to be, which it is.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            wide("CurrentBuildNumber").as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            build.as_mut_ptr().cast(),
            &mut size,
        )
    };
    let build = String::from_utf16_lossy(&build);
    let build = build.trim_end_matches('\0').parse::<u32>();
    read == 0 && build.is_ok_and(|build| build >= FIRST)
}

/// Has Windows draw Mica behind the window, and says whether it will. A
/// Windows that has no Mica draws nothing, and the ground is then drawn whole.
/// Mica fills the window without its frame being stretched over it: what is
/// drawn reaches Windows' compositor with what is see-through in it. Stretched,
/// the frame would show Windows' own buttons through the ground.
#[cfg(windows)]
pub fn mica(window: &impl raw_window_handle::HasWindowHandle) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute,
    };
    let Some(window) = hwnd(window) else { return false };
    let backdrop = DWMSBT_MAINWINDOW;
    // SAFETY: the window is this one, and the value is as large as it is said to be.
    let drawn = unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const backdrop).cast(),
            4,
        )
    };
    drawn >= 0
}

/// Keeps a window made see-through from showing white under what is drawn,
/// whether or not Mica is then drawn behind it.
#[cfg(windows)]
pub fn clear(window: &impl raw_window_handle::HasWindowHandle) {
    use windows_sys::Win32::{
        Graphics::Gdi::{RDW_ERASE, RDW_INVALIDATE, RedrawWindow},
        UI::Shell::SetWindowSubclass,
    };
    let Some(window) = hwnd(window) else { return };
    // SAFETY: the window is this one, made on this thread, and what hooks into it
    // unhooks itself as it goes.
    unsafe {
        SetWindowSubclass(window, Some(cleared), CLEARED, 0);
        RedrawWindow(window, std::ptr::null(), std::ptr::null_mut(), RDW_ERASE | RDW_INVALIDATE);
    }
}

/// The window's own name for what clears it.
#[cfg(windows)]
const CLEARED: usize = 2;

/// Under what is drawn, Windows keeps a surface of the window's own, which
/// nothing paints: left as it comes it is white, and lies over Mica. Black is
/// nothing there.
#[cfg(windows)]
unsafe extern "system" fn cleared(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _hook: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{BLACK_BRUSH, FillRect, GetStockObject, HDC},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass},
            WindowsAndMessaging::{GetClientRect, WM_ERASEBKGND, WM_NCDESTROY},
        },
    };
    match message {
        WM_ERASEBKGND => {
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: the window is this one, and Windows has sent what to paint it with.
            unsafe {
                GetClientRect(window, &mut rect);
                FillRect(wparam as HDC, &rect, GetStockObject(BLACK_BRUSH));
            }
            return 1;
        }
        WM_NCDESTROY => {
            // SAFETY: it is this window that this was hooked into, by this name.
            unsafe { RemoveWindowSubclass(window, Some(cleared), CLEARED) };
        }
        _ => {}
    }
    // SAFETY: what Windows sent, passed on as it was sent.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

/// The window's buttons, for a row laid out from the right. On Windows they are
/// as tall as the row, and square, as Windows draws its own.
pub fn controls(ui: &mut egui::Ui) {
    #[cfg(windows)]
    captions(ui);
    #[cfg(not(windows))]
    rounds(ui);
}

/// Windows' three, side by side with no room between them: the one that
/// spreads the window, or brings it back, goes by how it is now. While the
/// window is not the one in use they are fainter.
#[cfg(windows)]
fn captions(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let away = ctx.input(|i| i.viewport().focused == Some(false));
    let free = Held::default();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if caption_button(ui, Caption::Close, "Close", away, free).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let (spread, caption, tip) = match spread(&ctx) {
            true => (true, Caption::Restore, "Restore"),
            false => (false, Caption::Maximize, "Maximize"),
        };
        // Where this one is, Windows offers its layouts, and it is Windows that
        // follows the pointer over it. Its top edge still sizes the window.
        let button = caption_button(ui, caption, tip, away, snap::held());
        let mut place = button.rect;
        if !spread {
            place.min.y += EDGE;
        }
        // Not while something of egui's that takes the pointer lies over any of
        // it, a menu or the palette or a note: what is pressed there is then
        // egui's, and Windows is told of no button.
        let row = ui.layer_id();
        let within = place.shrink(1.0);
        let spots = [
            within.center(),
            within.left_top(),
            within.right_top(),
            within.left_bottom(),
            within.right_bottom(),
        ];
        let covered = spots.iter().any(|&at| ctx.layer_id_at(at).is_some_and(|on| on != row));
        snap::place(&ctx, (!covered).then_some(place));
        if button.clicked() || snap::clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!spread));
        }
        if caption_button(ui, Caption::Minimize, "Minimize", away, free).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    });
}

/// Round ones, apart, as the desktops of Linux draw theirs.
#[cfg_attr(windows, allow(dead_code))]
fn rounds(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if round_button(ui, Icon::Close, "Close").clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let (spread, tip) = match spread(&ctx) {
            true => (true, "Restore"),
            false => (false, "Maximize"),
        };
        if round_button(ui, Icon::Maximize, tip).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!spread));
        }
        if round_button(ui, Icon::Minimize, "Minimize").clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    });
}

/// The window's edges and corners, which drag to size it, and a line around it
/// to part it from what lies behind. A window that fills its screen has neither.
/// They are asked for last of all, to have the pointer before anything they lie over.
/// On Windows the desktop draws the line.
pub fn edges(ui: &egui::Ui) {
    let ctx = ui.ctx();
    if spread(ctx) {
        return;
    }
    let window = ctx.content_rect();
    if !cfg!(windows) {
        let line = Stroke::new(1.0, EDGE_LINE);
        ui.painter().rect_stroke(window, corners(ctx), line, StrokeKind::Inside);
    }
    for (direction, rect) in handles(window, EDGE, CORNER) {
        let id = egui::Id::unique(("window edge", direction as u8));
        let handle = ui.interact(rect, id, Sense::click_and_drag());
        if handle.hovered() || handle.dragged() {
            ctx.set_cursor_icon(cursor(direction));
        }
        // The desktop sizes the window from the press on: it is not for waiting
        // to see whether the pointer moves.
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if pressed && handle.is_pointer_button_down_on() {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}

/// What takes the pointer to give a rect another size, each with the way it
/// goes: a strip `edge` deep inside each side, and a square of `corner`
/// inward from each corner. The corners come after the sides they lie over.
pub fn handles(rect: Rect, edge: f32, corner: f32) -> [(ResizeDirection, Rect); 8] {
    use ResizeDirection::*;
    let (x, y) = (rect.x_range(), rect.y_range());
    let (left, right) = (rect.left(), rect.right());
    let (top, bottom) = (rect.top(), rect.bottom());
    let square =
        |at: egui::Pos2, inward: [f32; 2]| Rect::from_two_pos(at, at + corner * Vec2::from(inward));
    [
        (North, Rect::from_x_y_ranges(x, top..=top + edge)),
        (South, Rect::from_x_y_ranges(x, bottom - edge..=bottom)),
        (West, Rect::from_x_y_ranges(left..=left + edge, y)),
        (East, Rect::from_x_y_ranges(right - edge..=right, y)),
        (NorthWest, square(rect.left_top(), [1.0, 1.0])),
        (NorthEast, square(rect.right_top(), [-1.0, 1.0])),
        (SouthWest, square(rect.left_bottom(), [1.0, -1.0])),
        (SouthEast, square(rect.right_bottom(), [-1.0, -1.0])),
    ]
}

/// The pointer's shape over one of them.
pub fn cursor(direction: ResizeDirection) -> CursorIcon {
    use ResizeDirection::*;
    match direction {
        North | South => CursorIcon::ResizeVertical,
        East | West => CursorIcon::ResizeHorizontal,
        NorthWest | SouthEast => CursorIcon::ResizeNwSe,
        NorthEast | SouthWest => CursorIcon::ResizeNeSw,
    }
}
