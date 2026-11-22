//! The window's own frame, in place of the desktop's: its top row moves it, its
//! edges size it, and three buttons in that row put it away, spread it over the
//! screen and close it. On Windows the desktop's own frame is kept, and of this
//! one only the top row is: it is drawn square, with neither buttons nor edges.
use eframe::egui::{
    self, Align, CursorIcon, Layout, PointerButton, Rect, ResizeDirection, Sense, Stroke,
    StrokeKind, UiBuilder, Vec2, ViewportCommand,
};

use super::style::{CARD_RADIUS, EDGE_LINE, GUTTER, Icon, round_button};

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
fn corners(ctx: &egui::Context) -> f32 {
    if cfg!(windows) {
        return 0.0;
    }
    match spread(ctx) {
        true => 0.0,
        false => f32::from(CARD_RADIUS) + GUTTER,
    }
}

/// The window's ground, which everything in it lies on.
pub fn ground(ui: &egui::Ui, fill: egui::Color32) {
    let ctx = ui.ctx();
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

/// On Windows the desktop's title bar is dark, and of the window's ground, to be
/// one with the top row below it.
#[cfg(windows)]
pub fn dress(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    };
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
    let window = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let [red, green, blue, _] = super::style::VIEW.to_array();
    // Red is the lowest byte.
    let color = u32::from_le_bytes([red, green, blue, 0]);
    let dark: i32 = 1;
    // SAFETY: the window is this one, and each value is as large as it is said to
    // be. Windows before 11 know no colour for the bar, and leave it as it was.
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            4,
        );
        DwmSetWindowAttribute(window, DWMWA_CAPTION_COLOR as u32, (&raw const color).cast(), 4);
    }
}

/// The window's buttons, for a row laid out from the right.
pub fn controls(ui: &mut egui::Ui) {
    if cfg!(windows) {
        return;
    }
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
pub fn edges(ui: &egui::Ui) {
    let ctx = ui.ctx();
    if cfg!(windows) || spread(ctx) {
        return;
    }
    let window = ctx.content_rect();
    let line = Stroke::new(1.0, EDGE_LINE);
    ui.painter().rect_stroke(window, corners(ctx), line, StrokeKind::Inside);
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
