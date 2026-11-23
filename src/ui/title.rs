//! On Windows, one row along the top of the window in place of Windows' own
//! title bar and of the rows over the sidebar and the terminals: the window's
//! icon, the button for the sidebar, where the window is, a button drawn as a
//! field that searches, the buttons that make terminals, list what is at work
//! and every command, and the window's own three. Where nothing of it is under
//! the pointer, it moves the window.
use eframe::egui::{self, Align, FontId, Layout, Rangef, Rect, Stroke, UiBuilder, Vec2};

use super::{
    App,
    frame::{controls, titlebar},
    style::{
        CHEVRON, DIM, EDGE_LINE, Icon, TEXT, TITLEBAR, TITLED, bold, chevron, emblem, search_field,
    },
    toolbar::Names,
};
use crate::model::View;

/// The least and the most the search box is wide.
const SEARCH: [f32; 2] = [160.0, 380.0];
/// How much room the names keep before the search box gives up any of its
/// width for them.
const NAMES: f32 = 120.0;
/// The space between the parts of the row.
const GAP: f32 = 8.0;
/// How wide each of the window's own buttons is.
const CAPTION: f32 = 46.0;
/// How wide the line is that parts the buttons that make terminals from the
/// others, with the room to each side of it.
const DIVIDER: f32 = 13.0;
/// How large the names are.
const SIZE: f32 = 13.0;
/// A tab in the row, with the sidebar hidden, and the least of one there must
/// be room for beside the names.
const TAB: Vec2 = Vec2::new(220.0, 32.0);
const LEAST_TAB: f32 = 120.0;
/// The least room for a strip of tabs: egui's scrolling areas are no narrower,
/// and one given less would lie over the search box.
const LEAST_STRIP: f32 = 64.0;

impl App {
    /// The row, across the top of the window.
    pub(super) fn title(&mut self, root_ui: &mut egui::Ui, views: &[View]) {
        egui::Panel::top("title bar")
            .exact_size(TITLEBAR)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(root_ui, |ui| self.title_row(ui, views));
    }

    fn title_row(&mut self, ui: &mut egui::Ui, views: &[View]) {
        let view = &views[self.current];
        let mut bar = titlebar(ui, TITLEBAR, 0.0);
        let row = bar.max_rect();
        let away = ui.input(|i| i.viewport().focused == Some(false));
        let within = |left: f32, right: f32| Rect::from_x_y_ranges(left..=right, row.y_range());
        let ahead = Layout::left_to_right(Align::Center);
        // The window's three, flush with its right edge.
        let captions = within(row.right() - 3.0 * CAPTION, row.right());
        controls(&mut part(&mut bar, captions, Layout::right_to_left(Align::Center)));
        // Before them, the buttons that make terminals, and the one that brings
        // out those hidden while there are any; then those for what is at work
        // and for every command.
        let hidden = view.current_tab().is_some_and(|tab| !tab.hidden.is_empty());
        let wide = buttons(hidden);
        let right = captions.left() - GAP;
        let group = within(right - wide, right);
        let mut ui = part(&mut bar, group, ahead);
        self.makers(&mut ui, view, TITLED);
        self.hidden(&mut ui, view, TITLED);
        let (line, _) = ui.allocate_exact_size(Vec2::new(DIVIDER, 20.0), egui::Sense::hover());
        ui.painter().vline(line.center().x, line.y_range(), Stroke::new(1.0, EDGE_LINE));
        self.watch(&mut ui, views, TITLED);
        let palette = "client.palette.toggle";
        self.button(&mut ui, view, TITLED, Icon::More, "Command palette", palette);
        // The window's icon, and the button for the sidebar.
        let icon = row.left_center() + egui::vec2(20.0, 0.0);
        let icon = Rect::from_center_size(icon, Vec2::splat(16.0));
        emblem(bar.painter(), icon);
        let toggle = within(icon.right() + 6.0, icon.right() + 6.0 + TITLED.size.x);
        let tip = match self.remembered.kept.sidebar {
            true => "Tabs along the top",
            false => "Tabs in a sidebar",
        };
        let mut ui = part(&mut bar, toggle, ahead);
        self.button(&mut ui, view, TITLED, Icon::Sidebar, tip, "client.sidebar.toggle");
        // Between them, where the window is, and the search box. With the
        // sidebar hidden, the tabs of the session on show follow its name.
        let tabbed = view.session().filter(|_| !self.remembered.kept.sidebar);
        let named = |tab| Names { size: SIZE, chevrons: true, home: true, tab, faint: away };
        let crumbs = self.crumbs(&bar, view);
        // The tabs, each as wide as it is drawn, with the room between them.
        let tabs = tabbed.map_or(0.0, |session| {
            session.tabs.iter().map(|tab| self.across(&bar, tab, TAB.x) + 2.0).sum::<f32>()
        });
        let keep = if tabbed.is_some() { crumbs + tabs } else { NAMES };
        let (lead, trail) = (toggle.right() + GAP - row.left(), row.right() - group.left() + GAP);
        let [names, search] = places(row.width(), lead, trail, keep);
        let at = |range: Rangef| within(row.left() + range.min, row.left() + range.max);
        let (names, search) = (at(names), at(search));
        let search = Rect::from_center_size(search.center(), Vec2::new(search.width(), 32.0));
        let mut ui = part(&mut bar, search, ahead);
        let switch = "session.switch";
        let field = search_field(&mut ui, search, &self.keys.map.shortcut(switch));
        if field.on_hover_text("Search sessions and tabs").clicked() {
            self.todo.push((switch, None));
        }
        let mut ui = part(&mut bar, names, ahead);
        let Some(session) = tabbed else {
            self.names(&mut ui, view, names, named(true));
            return;
        };
        // The server and the session, if there is room for them and a tab, or
        // for all the tabs where they take less: with none, for them alone.
        let mut start = names.left();
        if names.width() >= crumbs + LEAST_TAB.min(tabs) {
            let end = self.names(&mut ui, view, names, named(false));
            let ink = if away { DIM.gamma_multiply(0.6) } else { DIM };
            chevron(ui.painter(), egui::pos2(end, names.center().y), ink);
            start = end + CHEVRON;
        }
        let room = names.right() - start;
        if room >= LEAST_STRIP {
            let mut ui = part(&mut bar, within(start, names.right()), ahead);
            self.along(&mut ui, view, session, room, TAB);
        }
    }

    /// How wide the server's name and the session's are, as they lead the tabs
    /// in the row, with the chevrons after each.
    fn crumbs(&self, ui: &egui::Ui, view: &View) -> f32 {
        let wide = |text: &str, font: FontId| {
            ui.painter().layout_no_wrap(text.to_owned(), font, TEXT).size().x
        };
        let server = wide(self.servers[self.current].label(), FontId::proportional(SIZE));
        let session = view.session().map_or(0.0, |session| wide(&session.name, bold(SIZE)));
        server + session + 2.0 * CHEVRON
    }
}

/// How wide the buttons are that make terminals, with the one that brings out
/// those hidden while there are any, and those for what is at work and for
/// every command, with the line that parts them.
fn buttons(hidden: bool) -> f32 {
    let buttons = 6.0 + f32::from(u8::from(hidden));
    buttons * (TITLED.size.x + 2.0) + DIVIDER
}

/// A part of the row, laid out as `layout` says within `rect`.
fn part(bar: &mut egui::Ui, rect: Rect, layout: Layout) -> egui::Ui {
    let mut part = bar.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    part.spacing_mut().item_spacing.x = 2.0;
    part
}

/// Where the middle of the row goes, across a row `width` wide whose buttons
/// take `lead` at its left and `trail` at its right: the names, and the search
/// box. The box is in the middle of the window where it can be. Where it
/// cannot, the names, which want `keep`, keep that much, and the box narrows
/// to the room past them, and moves no further from the middle than it must.
/// With too little room for both, the box keeps its least width where there
/// is that much between the buttons, and otherwise takes all there is, and
/// the names have what is left, which may be none. Each is a stretch of the
/// row from its left.
pub fn places(width: f32, lead: f32, trail: f32, keep: f32) -> [Rangef; 2] {
    let [least, most] = SEARCH;
    let (start, end) = (lead, width - trail);
    let first = start + keep.max(0.0) + GAP;
    let middle = width / 2.0;
    let centred = middle - most / 2.0;
    let wide = match centred >= first && centred + most <= end {
        true => most,
        false => (end - first).clamp(least, most),
    };
    // Never past the buttons at either end.
    let wide = wide.min((end - start).max(0.0));
    let left = (middle - wide / 2.0).max(first).min(end - wide).max(start);
    let names = Rangef::new(start, (left - GAP).max(start));
    [names, Rangef::new(left, left + wide)]
}

#[cfg(test)]
#[path = "../../tests/unit/ui_title.rs"]
mod tests;
