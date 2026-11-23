//! The row along the top that holds the tabs when there is no sidebar, and
//! the buttons both share.
use eframe::egui::{self, Align, Color32, FontId, Layout, Rect, Sense, UiBuilder, Vec2};

use super::{
    App,
    close::{Ask, CLOSE_TAB},
    commands::title,
    floats,
    frame::{controls, titlebar},
    heard::{dot, worn},
    rows::Target,
    style::{
        BODY, CHEVRON, DIM, Flat, Icon, Item, SQUARE, TEXT, TOOLBAR, bold, chevron, confirm,
        confirm_width, fitted, flat_button, item,
    },
};
use crate::model::{Action, Session, Tab, View};

/// How the names that say where the window is are written: in the row over
/// the terminals, apart and as large as the sidebar's, or on Windows, smaller,
/// with a chevron between each and the next, and home named too. There, with
/// the tabs beside them, the tab and what is in it are not named again, and
/// while the window is not the one in use they are fainter.
#[derive(Clone, Copy)]
pub(super) struct Names {
    pub size: f32,
    pub chevrons: bool,
    pub home: bool,
    pub tab: bool,
    pub faint: bool,
}

impl Names {
    /// As the row over the terminals has them.
    const HEADER: Self = Self { size: BODY, chevrons: false, home: false, tab: true, faint: false };
}

impl App {
    /// The buttons that make another terminal: in a tab of its own, beside the
    /// focused pane or below it, or floating over the tab, where the button
    /// also goes to those that do and puts them away. They stand together,
    /// apart from the window's own.
    pub(super) fn makers(&mut self, ui: &mut egui::Ui, view: &View, flat: Flat) {
        self.button(ui, view, flat, Icon::Plus, "New tab", "client.tab.new");
        let splits = [(Icon::SplitRight, "pane.split.right"), (Icon::SplitDown, "pane.split.down")];
        for (icon, name) in splits {
            self.button(ui, view, flat, icon, title(name), name);
        }
        self.button(ui, view, flat, Icon::Float, "Floating panes", floats::TOGGLE);
    }

    /// A button that lists the panes of the tab on show that are out of sight:
    /// behind a zoomed one, or in a floating layer that was put away. Picking one
    /// brings it out. With none, there is no button.
    pub(super) fn hidden(&mut self, ui: &mut egui::Ui, view: &View, flat: Flat) {
        let hidden = view.current_tab().map_or(&[][..], |tab| &tab.hidden);
        if hidden.is_empty() {
            return;
        }
        let button = flat_button(ui, flat, Icon::Behind, "Hidden panes", false);
        // It wears the most pressing thing any of them has to say.
        worn(ui, &button, hidden.iter().map(|pane| pane.activity).max());
        let mut picked = None;
        egui::Popup::menu(&button).width(260.0).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for pane in hidden {
                let word = pane.word();
                let row = Item {
                    icon: Some(pane.program),
                    mark: dot(ui.ctx(), pane.activity),
                    word: word.as_deref(),
                    label: &pane.title,
                    ..Default::default()
                };
                if item(ui, Vec2::new(ui.available_width(), 32.0), row).0.clicked() {
                    picked = Some(pane.id.clone());
                }
            }
        });
        if let Some(pane) = picked {
            self.send(Action::Reveal { pane });
        }
    }

    /// With the sidebar hidden, one row holds what it held: the sessions, behind a
    /// button, and the tabs of the one on show.
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, views: &[View]) {
        let view = &views[self.current];
        let mut bar = titlebar(ui, TOOLBAR, 8.0);
        let sidebar = "client.sidebar.toggle";
        self.button(&mut bar, view, SQUARE, Icon::Sidebar, "Tabs in a sidebar", sidebar);
        self.switcher(&mut bar, views);
        bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
            controls(ui);
            ui.add_space(8.0);
            self.hidden(ui, view, SQUARE);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(4.0);
                let Some(session) = view.session() else {
                    return;
                };
                // Room is kept at the end for the buttons that make another.
                let room = (ui.available_width() - 108.0).max(0.0);
                self.along(ui, view, session, room, Vec2::new(220.0, 28.0));
                self.makers(ui, view, SQUARE);
            });
        });
    }

    /// The tabs of the session on show, side by side in a strip as wide as
    /// `room` at most, each `size`.
    pub(super) fn along(
        &mut self,
        ui: &mut egui::Ui,
        view: &View,
        session: &Session,
        room: f32,
        size: Vec2,
    ) {
        // Each is as wide as its name, and the wheel turns them when they are
        // too many for the row, whichever way it is turned. The row is not
        // theirs to drag: where no tab is, it is the window's handle.
        ui.style_mut().always_scroll_the_only_direction = true;
        let wheel = egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..Default::default()
        };
        egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .scroll_source(wheel)
            .max_width(room)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let shown = view.selected_tab.as_deref();
                self.tabs(ui, self.current, session, shown, size);
            });
    }

    /// With the sidebar, the row over the terminals holds the buttons the
    /// sidebar has no room for, and says where the window is: the server, if it
    /// is not home, the session, its tab, and what is at the keyboard in it.
    /// The sidebar's rows say the same in less room, and cut short. While the
    /// sidebar lists what is at work instead, or the session's server is folded
    /// away, the session and its tab have no rows, and nor has a tab folded away
    /// under its session: the field that renames either is then here, in place
    /// of its name, and so is the asking again of a tab asked to close.
    pub(super) fn header(&mut self, ui: &mut egui::Ui, view: &View) {
        let mut bar = titlebar(ui, TOOLBAR, 8.0);
        bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
            controls(ui);
            ui.add_space(8.0);
            self.hidden(ui, view, SQUARE);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                self.makers(ui, view, SQUARE);
                let room = ui.available_rect_before_wrap();
                let room = room.with_min_x(room.left() + 8.0);
                self.names(ui, view, room, Names::HEADER);
            });
        });
    }

    /// Say where the window is, from the left of `room`, as `how` says: the
    /// server, the session, its tab, and what is at the keyboard in it. The
    /// sidebar's rows say the same in less room, and cut short. While the
    /// sidebar lists what is at work instead, or the session's server is
    /// folded away, the session and its tab have no rows, and nor has a tab
    /// folded away under its session: the field that renames either is then
    /// here, in place of its name, and so is the asking again of a tab asked
    /// to close. Gives back where the names end.
    pub(super) fn names(&mut self, ui: &mut egui::Ui, view: &View, room: Rect, how: Names) -> f32 {
        let (session, tab) = (view.session(), view.current_tab());
        let tab = tab.filter(|_| how.tab);
        let focused = self.focused.as_deref().and_then(|id| view.pane(id));
        let title = focused.map(|pane| pane.title.as_str()).filter(|_| how.tab);
        let plain = FontId::proportional(how.size);
        // The session and its tab, as the sidebar's rows go by, while it
        // has none of them.
        let (server, folded) = (self.current, &self.remembered.kept.folded);
        let away = self.activity || folded.contains(self.servers[server].address());
        let row = |tab: Option<&Tab>| {
            let (session, tab) = (session?.id.clone(), tab.map(|tab| tab.id.clone()));
            Some(Target { server, session, tab })
        };
        let named = row(None).filter(|_| away);
        let tucked = away || session.is_some_and(|session| self.folded(session));
        let tabbed = tab.and_then(|tab| row(Some(tab))).filter(|_| tucked);
        // The tab was asked to close, and asks again after the names, on
        // a button so wide.
        let asked = tabbed.clone().filter(|tab| self.closing.waits(self.row(tab)));
        let asked = asked.map(|tab| (tab, confirm_width(ui, CLOSE_TAB)));
        let mut room = room;
        if let Some((_, width)) = asked {
            room.max.x -= width + 10.0;
        }
        let host = (how.home || server > 0).then(|| self.servers[server].label().to_owned());
        let ink = |color: Color32| if how.faint { color.gamma_multiply(0.6) } else { color };
        let names = [
            (host.as_deref(), &plain, ink(DIM), None),
            (session.map(|session| session.name.as_str()), &bold(how.size), ink(TEXT), named),
            (tab.map(Tab::name), &plain, ink(DIM), tabbed),
            // A tab that was given a name has this to add.
            (title.filter(|title| Some(*title) != tab.map(Tab::name)), &plain, ink(DIM), None),
        ];
        // They are drawn, not laid out: nothing of them takes the pointer
        // from the handle they lie on. But one being renamed gives its
        // place to the field for it.
        let mut at = room.left_center();
        let mut first = true;
        for (name, font, color, target) in names {
            let Some(name) = name else {
                continue;
            };
            // On Windows a chevron leads every name but the first.
            let lead = if how.chevrons && !first { CHEVRON } else { 0.0 };
            let left = room.right() - 4.0 - at.x - lead;
            if left <= 24.0 {
                continue;
            }
            if lead > 0.0 {
                chevron(ui.painter(), at, ink(DIM));
                at.x += lead;
            }
            first = false;
            let size = match target.filter(|target| self.renames(target)) {
                Some(target) => {
                    let size = Vec2::new(left.min(260.0), 28.0);
                    let field = Rect::from_min_size(at - egui::vec2(0.0, size.y / 2.0), size);
                    let mut within = ui.new_child(UiBuilder::new().max_rect(field));
                    self.renaming(&mut within, &target, self.row(&target), size);
                    size
                }
                None => {
                    let name = fitted(ui, name, font.clone(), color, left);
                    let size = name.size();
                    ui.painter().galley(at - egui::vec2(0.0, size.y / 2.0), name, color);
                    size
                }
            };
            at.x += size.x + if how.chevrons { 0.0 } else { 10.0 };
        }
        if let Some((target, width)) = asked {
            let (edge, id) = (at + egui::vec2(width, 0.0), self.row(&target));
            let asks = confirm(ui, edge, id.with("over"), CLOSE_TAB);
            self.wait(ui, asks.rect, &asks, Ask::Again);
            if asks.clicked() {
                self.shut(target, tab.is_some_and(Tab::idle), true);
            }
        }
        at.x
    }
}

/// A row of controls, such as the toolbar or a server's heading.
pub fn bar(ui: &mut egui::Ui, height: f32, inset: f32) -> egui::Ui {
    let size = Vec2::new(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let rect = rect.shrink2(egui::vec2(inset, 0.0));
    let layout = Layout::left_to_right(Align::Center);
    let mut bar = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    bar.spacing_mut().item_spacing.x = 2.0;
    bar
}
