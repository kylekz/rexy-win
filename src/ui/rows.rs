//! The rows of sessions and tabs, in the sidebar or along the top: showing,
//! renaming, moving and closing them.
use eframe::egui::{
    self, Align2, DragAndDrop, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    UiBuilder, Vec2,
};

use super::{
    App,
    close::{Ask, CLOSE, CLOSE_TAB, asking, closer},
    heard::dot,
    remembered::flip,
    style::{BODY, DIM, Icon, Item, ROW, SMALL, bold, entry, fold, item, tints},
};
use crate::model::{self, Action, Activity, Edit, Session, Tab};

/// A session or, with `tab`, one of its tabs.
#[derive(Clone, PartialEq)]
pub(super) struct Target {
    pub(super) server: usize,
    pub(super) session: String,
    pub(super) tab: Option<String>,
}

impl Target {
    /// The action that shows it.
    fn select(&self) -> Action {
        let session = self.session.clone();
        match self.tab.clone() {
            Some(tab) => Action::SelectTab { session, tab },
            None => Action::SelectSession(session),
        }
    }

    /// The action that changes it.
    pub(super) fn edit(self, edit: Edit) -> Action {
        Action::Edit { session: self.session, tab: self.tab, edit }
    }
}

/// A pane being dragged by its header, and the tab it is dragged from.
#[derive(Clone)]
pub(super) struct Carried {
    pub(super) from: Target,
    pub(super) pane: String,
}

/// A name being typed over a session's or a tab's.
pub struct Rename {
    target: Target,
    text: String,
    /// The name the field opened with. Left as it was, it is not given anew: that
    /// would name a tab for good by what was running in it just then.
    was: String,
    /// The field has yet to be shown.
    fresh: bool,
    /// The field was shown this frame. A row that has gone takes its renaming with it.
    pub shown: bool,
}

/// What was asked of a session or a tab through its menu.
pub(super) enum Choice {
    Rename,
    /// Or the reverse, of a tab that is muted.
    Mute,
    Edit(Edit),
}

impl App {
    /// The name a session's or a tab's row goes by, which stays its own whatever
    /// rows come and go around it.
    pub(super) fn row(&self, target: &Target) -> egui::Id {
        let host = &self.servers[target.server].host.label;
        egui::Id::unique(("row", host, &target.session, &target.tab))
    }

    /// Whether a session's tabs are folded away under its name. Only several
    /// are: a session's one tab is its own row.
    pub(super) fn folded(&self, session: &Session) -> bool {
        session.tabs.len() > 1 && self.remembered.kept.folded.contains(&session.id)
    }

    /// The pane let go on a session's or a tab's row, if one was that can go
    /// there, and while one is held over the row the row is marked. A pane
    /// goes to another tab of its own session, or dropped on the session to a
    /// tab of its own: a server moves none from one session to another.
    fn landed(&self, ui: &egui::Ui, row: &egui::Response, target: &Target) -> Option<String> {
        let fits = |carried: &Carried| {
            let near = (carried.from.server, &carried.from.session);
            near == (target.server, &target.session) && carried.from.tab != target.tab
        };
        let held = row.dnd_hover_payload::<Carried>().filter(|carried| fits(carried))?;
        outline(ui, row.rect);
        let dropped = row.dnd_release_payload::<Carried>().is_some();
        dropped.then(|| held.pane.clone())
    }

    /// A session's tabs, down the sidebar or along the top. `shown` is the one on show.
    pub(super) fn tabs(
        &mut self,
        ui: &mut egui::Ui,
        server: usize,
        session: &Session,
        shown: Option<&str>,
        size: Vec2,
    ) {
        let rows = session.tabs.iter().enumerate();
        let rows: Vec<_> = rows
            .map(|(index, held)| (&held.id, self.tab(ui, server, session, shown, index, size)))
            .collect();
        if !self.joining(ui, server, session, &rows) {
            self.reorder(ui, server, &rows);
        }
    }

    /// While a tab of one pane is dragged over the middle of another tab of its
    /// session, mark that tab, and have the pane join it when it is let go
    /// there. True while it is over one: let go between two rows, a tab moves
    /// to there instead, as `reorder` has it. A tab of several panes only
    /// moves, for joining another would lose how they are laid out.
    fn joining(
        &mut self,
        ui: &egui::Ui,
        server: usize,
        session: &Session,
        rows: &[(&String, Rect)],
    ) -> bool {
        let Some(drag) = DragAndDrop::payload::<Target>(ui.ctx()) else {
            return false;
        };
        let here = drag.server == server && drag.session == session.id;
        let from = drag.tab.as_ref().filter(|_| here);
        let from = from.and_then(|id| session.tabs.iter().find(|tab| &tab.id == id));
        let alone = from.and_then(|tab| {
            let mut panes = tab.every();
            panes.next().filter(|_| panes.next().is_none())
        });
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let (Some(from), Some(pane), Some(pointer)) = (from, alone, pointer) else {
            return false;
        };
        // The middle of a row, along the way the rows run: its ends are where
        // a tab is let go to be moved.
        let ends = if ui.layout().is_horizontal() { [0.3, 0.0] } else { [0.0, 0.3] };
        let middle = |space: &Rect| space.shrink2(space.size() * Vec2::from(ends));
        let over =
            |(tab, space): &&(&String, Rect)| **tab != from.id && middle(space).contains(pointer);
        let Some((to, space)) = rows.iter().find(over) else {
            return false;
        };
        outline(ui, *space);
        if ui.input(|i| i.pointer.any_released()) {
            let (session, tab) = (session.id.clone(), (*to).clone());
            self.tell(server, Action::MovePane { pane: pane.id.clone(), tab: Some(tab.clone()) });
            self.open(server, Action::SelectTab { session, tab });
        }
        true
    }

    /// While one of these rows is dragged, mark where among them it would land, and
    /// move it there when it is let go. The rows are a server's sessions, or the tabs
    /// of one of them, each with the space it takes.
    pub(super) fn reorder(&mut self, ui: &egui::Ui, server: usize, rows: &[(&String, Rect)]) {
        let Some(drag) = DragAndDrop::payload::<Target>(ui.ctx()) else {
            return;
        };
        let id = drag.tab.as_ref().unwrap_or(&drag.session);
        let from = rows.iter().position(|(row, _)| *row == id);
        let from = from.filter(|_| drag.server == server);
        let (Some(from), Some(pointer)) = (from, ui.input(|i| i.pointer.latest_pos())) else {
            return;
        };
        let across = ui.layout().is_horizontal();
        let along = |pos: Pos2| if across { pos.x } else { pos.y };
        // It lands ahead of the first row whose middle the pointer has yet to pass.
        let passed = |(_, space): &&(&String, Rect)| along(pointer) >= along(space.center());
        let to = rows.iter().take_while(passed).count();
        // Where it is already, there is nowhere to go.
        if to == from || to == from + 1 {
            return;
        }
        let (edge, row) = match rows.get(to) {
            Some((_, space)) => (along(space.min) - 1.0, space),
            None => (along(rows[to - 1].1.max) + 1.0, &rows[to - 1].1),
        };
        let line = Stroke::new(2.0, tints(ui.ctx()).accent);
        if across {
            ui.painter().vline(edge, row.y_range(), line);
        } else {
            ui.painter().hline(row.x_range(), edge, line);
        }
        if ui.input(|i| i.pointer.any_released()) {
            let before = rows.get(to).map(|(row, _)| (*row).clone());
            self.edit(Target::clone(&drag), Edit::MoveBefore(before));
        }
    }

    /// A session in the sidebar: its name, heading its tabs. One with a single
    /// tab is a row like a tab's, which goes by the session's name and says the
    /// tab's after it.
    pub(super) fn session(
        &mut self,
        ui: &mut egui::Ui,
        server: usize,
        sessions: &[Session],
        index: usize,
        shown: bool,
    ) {
        let session = &sessions[index];
        let target = Target { server, session: session.id.clone(), tab: None };
        let lone = match session.tabs.as_slice() {
            [tab] => Some(tab),
            _ => None,
        };
        // Several tabs fold away under its name, which then stands for them:
        // it is lit while one of them is on show, and says the most pressing
        // thing any of them does.
        let (folds, folded) = (session.tabs.len() > 1, self.folded(session));
        let height = if lone.is_some() { ROW } else { 28.0 };
        let size = Vec2::new(ui.available_width(), height);
        let id = self.row(&target);
        if self.renaming(ui, &target, id, size).is_some() {
            return;
        }
        // Its one tab has no row of its own for the field that renames it.
        let tab = lone.map(|tab| Target { tab: Some(tab.id.clone()), ..target.clone() });
        if let Some(tab) = &tab
            && self.renaming(ui, tab, self.row(tab), size).is_some()
        {
            return;
        }
        // Closing it loses nothing if it is one tab at most, with nothing in
        // it but shells at their prompts. Several tabs are not closed by one
        // press of a button that stands beside the one for a new tab.
        let spared = session.tabs.len() <= 1 && session.tabs.iter().all(Tab::idle);
        let ask = self.closing.ask(id, spared);
        // Its one tab, asked to close by a command, asks again here: by its own
        // name, and of itself, for the session outlives it.
        let asked = tab.as_ref();
        let asked = asked.filter(|tab| self.closing.waits(self.row(tab)));
        // What the tab's own row would say of it.
        let muted = lone.map(|tab| self.remembered.kept.muted.contains(&tab.id));
        // Under the session's name, what the tab says or the tab's own.
        let told = lone.map(|tab| self.lines(ui.ctx(), tab, &session.name, tab.name()));
        let (said, under, word, tip) = told.unwrap_or_default();
        let behind = lone.map(Tab::behind).unwrap_or_default();
        let row = Item {
            id: Some(id),
            caption: lone.is_none(),
            icon: lone.map(Tab::program),
            behind: &behind,
            under: &under,
            selected: (lone.is_some() || folded) && shown,
            mark: match muted {
                Some(true) => Some((Icon::Muted, DIM)),
                _ if folded => dot(ui.ctx(), session.activity()),
                _ => dot(ui.ctx(), said),
            },
            // A muted tab's mark has the edge to itself.
            word: word.as_deref().filter(|_| muted != Some(true)),
            tip: &tip,
            buttons: &[(Icon::Plus, "New tab"), (Icon::Close, "Close session")],
            confirm: match asked {
                Some(_) => Some(CLOSE_TAB),
                None => (ask == Ask::Again).then_some(CLOSE),
            },
            movable: true,
            label: &session.name,
            ..Default::default()
        };
        let (response, button) = item(ui, size, row);
        if lone.is_some() || folded {
            self.follow(id, shown, &response);
        }
        // A press on its name shows the session, as two there ask for a new
        // name. One anywhere else on the row folds its tabs away, or brings
        // them back.
        let aside = folds && {
            let wide = ui.painter().layout_no_wrap(session.name.clone(), bold(SMALL), DIM);
            let left = response.rect.left() + 12.0;
            let name = left - 4.0..=(left + wide.size().x + 4.0).min(response.rect.right() - 12.0);
            response.interact_pointer_pos().is_some_and(|at| !name.contains(&at.x))
        };
        if folds {
            fold(ui, response.rect, folded);
        }
        if let Some(pane) = self.landed(ui, &response, &target) {
            self.open(server, Action::MovePane { pane, tab: None });
        }
        if aside && response.clicked() {
            flip(&mut self.remembered.kept.folded, &session.id);
        } else if !aside {
            self.picked(&response, &target, shown);
        }
        let either = if asked.is_some() { Ask::Again } else { ask };
        self.wait(ui, response.rect, &response, either);
        if response.drag_started_by(PointerButton::Primary) {
            DragAndDrop::set_payload(ui.ctx(), target.clone());
        }
        let labels = ["Move Up", "Move Down", "Close Session"];
        let at = |index: usize| sessions.get(index).map(|s| s.id.clone());
        let asked_for = menu(&response, labels, muted, index, at, ask);
        let asked_for = asked_for.filter(|_| !(aside && response.double_clicked()));
        match (button, asked_for) {
            (Some(0), _) => {
                self.remembered.kept.folded.remove(&session.id);
                self.open(server, Action::CreateTab(session.id.clone()));
            }
            // The tab that asked, and not the session with it. Otherwise this
            // ends every program in the session.
            (Some(_), _) => self.shut(asked.cloned().unwrap_or(target), spared, true),
            (None, Some(Choice::Edit(Edit::Close))) => self.shut(target, spared, true),
            // Only a tab is muted.
            (None, Some(Choice::Mute)) => {
                if let Some((target, lone)) = tab.zip(lone) {
                    self.choose(target, lone.name(), Choice::Mute);
                }
            }
            (None, Some(choice)) => self.choose(target, &session.name, choice),
            (None, None) => {}
        }
    }

    /// How wide a tab is along the top, up to `most`, as it is drawn there.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn across(&self, ui: &egui::Ui, tab: &Tab, most: f32) -> f32 {
        let muted = self.remembered.kept.muted.contains(&tab.id);
        let word = speaks(tab).1.filter(|_| !muted);
        breadth(ui, tab.name(), word.as_deref(), most).0
    }

    /// The list turns to the row on show when another comes to be it, by
    /// whatever means, and stays where the user turns it otherwise.
    fn follow(&mut self, row: egui::Id, shown: bool, response: &egui::Response) {
        let list = row.with(self.remembered.kept.sidebar);
        if shown && self.followed != Some(list) {
            self.followed = Some(list);
            response.scroll_to_me(None);
        }
    }

    /// Take a click on a session's or a tab's row, if there was one, and show the
    /// row unless it is on show. The second click of two asks for a new name, and
    /// nothing tells a first click from a lone one, so the first has shown the
    /// row by then.
    fn picked(&mut self, response: &egui::Response, target: &Target, shown: bool) {
        if response.clicked() && !shown {
            self.open(target.server, target.select());
        }
    }

    /// A tab, in the sidebar or along the top. Returns the space it took.
    fn tab(
        &mut self,
        ui: &mut egui::Ui,
        server: usize,
        session: &Session,
        shown: Option<&str>,
        index: usize,
        size: Vec2,
    ) -> Rect {
        let across = ui.layout().is_horizontal();
        let tab = &session.tabs[index];
        let target = Target { server, session: session.id.clone(), tab: Some(tab.id.clone()) };
        let id = self.row(&target);
        let muted = self.remembered.kept.muted.contains(&tab.id);
        // Along the top a tab has one line. Down the sidebar it says under its
        // name what it does, or its terminal's title where it has another name.
        let (said, under, word, tip) = match across {
            true => {
                let (said, word, tip) = speaks(tab);
                (said, String::new(), word, tip)
            }
            false => {
                let title = tab.top().map_or("", |pane| &pane.title);
                self.lines(ui.ctx(), tab, tab.name(), title)
            }
        };
        // One that is muted says nothing in words.
        let mut word = word.filter(|_| !muted);
        // Along the top a tab is as wide as its name, up to the size given: its
        // icon and the room for its mark or its button come to as much again as
        // the row's own edges. What it says in words stands after the name
        // where both fit, and where they do not the name stays: its dot says
        // as much in colour, and the pointer is told the rest.
        let size = match across {
            true => {
                let (wide, both) = breadth(ui, tab.name(), word.as_deref(), size.x);
                word = word.filter(|_| both);
                Vec2::new(wide, size.y)
            }
            false => size,
        };
        if let Some(field) = self.renaming(ui, &target, id, size) {
            return field;
        }
        let selected = shown == Some(&tab.id);
        let spared = tab.idle();
        let ask = self.closing.ask(id, spared);
        let behind = tab.behind();
        let row = Item {
            id: Some(id),
            icon: Some(tab.program()),
            behind: &behind,
            selected,
            mark: if muted { Some((Icon::Muted, DIM)) } else { dot(ui.ctx(), said) },
            under: &under,
            word: word.as_deref(),
            buttons: &[(Icon::Close, "Close tab")],
            confirm: (ask == Ask::Again).then_some(CLOSE),
            tip: &tip,
            movable: true,
            label: tab.name(),
            // Down the sidebar it stands in from the session's name over it.
            inset: if across { 0.0 } else { 10.0 },
            ..Default::default()
        };
        let (response, button) = item(ui, size, row);
        self.follow(id, selected, &response);
        self.picked(&response, &target, selected);
        if let Some(pane) = self.landed(ui, &response, &target) {
            self.tell(server, Action::MovePane { pane, tab: Some(tab.id.clone()) });
            self.open(server, target.select());
        }
        self.wait(ui, response.rect, &response, ask);
        if response.drag_started_by(PointerButton::Primary) {
            DragAndDrop::set_payload(ui.ctx(), target.clone());
        }
        let labels = if across {
            ["Move Left", "Move Right", "Close Tab"]
        } else {
            ["Move Up", "Move Down", "Close Tab"]
        };
        let at = |index: usize| session.tabs.get(index).map(|tab| tab.id.clone());
        match (button, menu(&response, labels, Some(muted), index, at, ask)) {
            (Some(_), _) | (None, Some(Choice::Edit(Edit::Close))) => {
                self.shut(target, spared, true);
            }
            // The field opens on the name it was given, if it was given one.
            (None, Some(choice)) => {
                self.choose(target, tab.label.as_deref().unwrap_or_default(), choice);
            }
            (None, None) => {}
        }
        response.rect
    }

    /// What a tab says on a row of the sidebar that goes by `name`: the most
    /// pressing thing any of its panes does, the line under the name, the word
    /// at the row's edge, and what the pointer is told besides. The line is
    /// what its program says. Failing that it is `title`, which is under the
    /// pointer otherwise, and is not said at all where it is the name.
    fn lines(
        &self,
        ctx: &egui::Context,
        tab: &Tab,
        name: &str,
        title: &str,
    ) -> (Activity, String, Option<String>, String) {
        let Some(top) = tab.top() else {
            return Default::default();
        };
        let (said, edge) = self.told(ctx, top);
        let title = if title == name { "" } else { title }.to_owned();
        match said.is_empty() {
            true => (top.activity, title, edge, String::new()),
            false => (top.activity, said, edge, title),
        }
    }

    /// Do what a session's or a tab's menu was asked. `name` is what the field
    /// that renames it opens on.
    pub(super) fn choose(&mut self, target: Target, name: &str, choice: Choice) {
        match choice {
            Choice::Rename => {
                self.rename = Some(Rename {
                    target,
                    text: name.to_owned(),
                    was: name.to_owned(),
                    fresh: true,
                    shown: true,
                });
            }
            Choice::Mute => {
                let tab = target.tab.as_deref().unwrap_or_default();
                let muted = &mut self.remembered.kept.muted;
                flip(muted, tab);
                self.desktop.mute(muted.clone());
            }
            Choice::Edit(edit) => self.edit(target, edit),
        }
    }

    fn edit(&mut self, target: Target, edit: Edit) {
        self.tell(target.server, target.edit(edit));
    }

    /// Close a session or a tab as `close` does, by the name of its row: it asks
    /// again wherever that name is asked after.
    pub(super) fn shut(&mut self, target: Target, spared: bool, near: bool) {
        let (what, server) = (self.row(&target), target.server);
        self.close(what, spared, near, server, target.edit(Edit::Close));
    }

    /// Whether this is the session or the tab being renamed.
    pub(super) fn renames(&self, target: &Target) -> bool {
        let renamed = self.rename.as_ref();
        renamed.is_some_and(|rename| &rename.target == target)
    }

    /// Show the field that stands in for a row while it is renamed, and return the
    /// space it took. None if this row is not the one.
    pub(super) fn renaming(
        &mut self,
        ui: &mut egui::Ui,
        target: &Target,
        row: egui::Id,
        size: Vec2,
    ) -> Option<Rect> {
        let rename = self.rename.as_mut().filter(|r| &r.target == target)?;
        rename.shown = true;
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        // By the row's own name, so that the field keeps the keyboard when rows
        // before it come and go.
        let field = egui::TextEdit::singleline(&mut rename.text)
            .id(row.with("rename"))
            .hint_text("Name")
            .margin(egui::vec2(11.0, 0.0))
            .align(Align2::LEFT_CENTER)
            .min_size(rect.size());
        let mut output = field.show(&mut ui.new_child(UiBuilder::new().max_rect(rect)));
        if std::mem::take(&mut rename.fresh) {
            // Typing replaces the old name.
            let all = egui::text::CCursorRange::select_all_str(&rename.text);
            output.state.cursor.set_char_range(Some(all));
            output.state.store(ui.ctx(), output.response.id);
            output.response.request_focus();
        } else if output.response.lost_focus() {
            // Escape leaves the name as it was. Anything else, such as Enter or a
            // click elsewhere, takes the new one, if it is new.
            let name = self.rename.take().filter(|rename| rename.text != rename.was);
            let name = name.map(|rename| rename.text);
            if let Some(name) = name.filter(|_| !ui.input(|i| i.key_pressed(Key::Escape))) {
                self.edit(target.clone(), Edit::Rename(name));
            }
        }
        Some(rect)
    }
}

/// Mark a row as where what is held over it would go.
fn outline(ui: &egui::Ui, row: Rect) {
    let line = Stroke::new(1.5, tints(ui.ctx()).accent);
    ui.painter().rect_stroke(row, 7, line, StrokeKind::Inside);
}

/// How wide a tab is along the top, up to `most`, by its `name`: its icon and
/// the room for its mark or its button come to as much again as the row's
/// own edges. Also whether its `word` fits after the name.
fn breadth(ui: &egui::Ui, name: &str, word: Option<&str>, most: f32) -> (f32, bool) {
    let wide = |text: &str, size| {
        let font = egui::FontId::proportional(size);
        ui.painter().layout_no_wrap(text.into(), font, DIM).size().x
    };
    let name = wide(name, BODY) + 70.0;
    let both = word.map(|word| name + wide(word, SMALL)).filter(|both| *both <= most);
    (both.unwrap_or(name).clamp(96.0, most), both.is_some())
}

/// What a tab has to say on its row: the most pressing thing any of its panes
/// does, the word for that or how far along it is, and all of it in full, for a
/// row with no room for it, with what its program adds.
fn speaks(tab: &Tab) -> (Activity, Option<String>, String) {
    let Some(top) = tab.top() else {
        return Default::default();
    };
    let word = top.word();
    let tip = model::line(word.as_deref().unwrap_or_default(), &top.saying());
    (top.activity, word, tip)
}

/// What a session's or a tab's menu was asked, if anything. A double click asks for
/// a new name, as the menu's first entry does. A tab says whether it is `muted`.
/// The row is the one at `index` among its like, whose IDs `id` gives; `labels`
/// name moving it back, moving it on, and closing it, and `ask` says what
/// choosing to close it will do.
fn menu(
    response: &egui::Response,
    labels: [&str; 3],
    muted: Option<bool>,
    index: usize,
    id: impl Fn(usize) -> Option<String>,
    ask: Ask,
) -> Option<Choice> {
    let mut picked = response.double_clicked().then_some(Choice::Rename);
    asking(response, 200.0, |ui| {
        let mute = |muted| if muted { "Unmute Tab" } else { "Mute Tab" };
        let entries = [
            Some(("Rename", true, Choice::Rename)),
            muted.map(|muted| (mute(muted), true, Choice::Mute)),
            Some((
                labels[0],
                index > 0,
                Choice::Edit(Edit::MoveBefore(id(index.saturating_sub(1)))),
            )),
            Some((
                labels[1],
                id(index + 1).is_some(),
                // Before the one after the next, or last of all.
                Choice::Edit(Edit::MoveBefore(id(index + 2))),
            )),
        ];
        for (label, enabled, choice) in entries.into_iter().flatten() {
            let row = Item { label, ..Default::default() };
            if entry(ui, enabled, row).clicked() {
                picked = Some(choice);
                ui.close();
            }
        }
        if closer(ui, labels[2], ask) {
            picked = Some(Choice::Edit(Edit::Close));
        }
    });
    picked
}
