//! One list to find things in by name: every session, the tabs of the one on
//! show or of them all, and what can be done just now.
use std::sync::Arc;

use eframe::egui::{self, Color32, Key, Modifiers, Popup, Rect, RectAlign, Stroke, Vec2};
use serde_json::Value;

use super::{
    App,
    commands::{COMMANDS, Deed, label},
    heard::dot,
    hosts,
    style::{BODY, BORDER, Icon, Item, ROW, SMALL, TEXT, bold, caption, item, tick},
};
use crate::{
    model::{Action, Answer, Host, Question, Session, Tab, View},
    program::Program,
};

/// The name the palette goes by, to be opened and put away.
pub fn id() -> egui::Id {
    egui::Id::unique("palette")
}

/// Whether what is typed finds something, by any of the words for it.
fn found(typed: &str, words: &[&str]) -> bool {
    let typed = typed.trim().to_lowercase();
    words.iter().any(|word| word.to_lowercase().contains(&typed))
}

/// The words a tab is found by: its name, what its panes are titled and the
/// programs in them, and its session's name where that has other tabs too. A
/// session's only tab is found with it, by its row.
fn words<'a>(session: &'a Session, tab: &'a Tab) -> Vec<&'a str> {
    let panes = tab.every().flat_map(|pane| [pane.title.as_str(), pane.program.name()]);
    let session = (session.tabs.len() > 1).then_some(session.name.as_str());
    std::iter::once(tab.name()).chain(panes).chain(session).collect()
}

/// The servers a session may be made on from the palette: the one on show,
/// then every other that is connected, in the order the sidebar lists them.
fn makers(current: usize, connected: &[bool]) -> Vec<usize> {
    let others = (0..connected.len()).filter(|&server| server != current && connected[server]);
    std::iter::once(current).chain(others).collect()
}

/// What the palette lists until something is typed, and what it is typed for.
#[derive(Clone, PartialEq)]
pub enum Listing {
    /// The sessions and the tabs. What is typed is looked for among those and
    /// among what can be done.
    Places,
    /// What can be done, likewise.
    Commands,
    /// The host that what is typed names: one to add, or what the host given
    /// is to be from now on.
    Host(Option<Host>),
    /// The hosts that can be removed.
    Hosts,
    /// What an action of a server's asks the user: the first of its questions
    /// yet to be answered. What is typed is the answer, or finds one.
    Asked,
}

/// A row of the list, and what taking it does.
struct Row {
    label: String,
    /// A second line, where it has more to say.
    under: String,
    /// A few words at its far end, in place of a mark.
    detail: String,
    icon: Option<Program>,
    /// A picture in that place, where an action gives it one.
    sign: Option<Icon>,
    mark: Option<(Icon, Color32)>,
    /// Whether it is among those chosen, where several may be.
    chosen: Option<bool>,
    /// The keys that do the same.
    keys: String,
    pick: Pick,
}

enum Pick {
    /// Show a session or a tab, or make a session, on some server.
    Show(usize, Action),
    /// Do something by its name: an action of this client's, or of the server's.
    Do(String),
    /// Have home do something to its list of hosts.
    Home(Action),
    /// Answer the question on show.
    Answer(Answer),
    /// Choose one of the things it offers, or leave it out again.
    Choose(String),
}

impl App {
    /// The session on show, as a button that opens the palette.
    pub(super) fn switcher(&mut self, ui: &mut egui::Ui, views: &[View]) {
        let current = self.current;
        let name = views[current].session().map_or("Sessions", |s| &s.name);
        // The tabs of the session on show speak for themselves, right beside this.
        let elsewhere = views.iter().enumerate().flat_map(|(server, view)| {
            let shown = view.selected_session.as_ref().filter(|_| server == current);
            let sessions = view.sessions.iter();
            sessions.filter(move |session| Some(&session.id) != shown)
        });
        let elsewhere = elsewhere.map(|session| session.activity()).max();
        let text = ui.painter().layout_no_wrap(name.to_owned(), bold(BODY), TEXT);
        let size = Vec2::new((text.size().x + 54.0).min(240.0), 28.0);
        let button = Item {
            menu: true,
            strong: true,
            mark: dot(ui.ctx(), elsewhere.unwrap_or_default()),
            label: name,
            ..Default::default()
        };
        if item(ui, size, button).0.clicked() {
            self.todo.push(("session.switch", None));
        }
    }

    /// Open the palette on something to list, or put it away. It opens with
    /// nothing typed, but on a host to change, which is there as it would be
    /// typed. Asked for a list it is not showing, it turns to that one, and is
    /// put away only by the one it shows.
    pub(super) fn list(&mut self, ctx: &egui::Context, listing: Listing) {
        let other = Popup::is_id_open(ctx, id()) && self.listing != listing;
        self.filter = match &listing {
            Listing::Host(Some(host)) => hosts::line(host),
            _ => String::new(),
        };
        (self.listing, self.cursor) = (listing, None);
        if !other {
            Popup::toggle_id(ctx, id());
        }
    }

    /// Take in what the servers' actions ask of the user, and put the first of
    /// their questions on show. One is over when it is answered here or from
    /// elsewhere, when its server is lost, and when the palette is put away or
    /// turned to something else while it shows the question: that says the
    /// user will not answer.
    pub(super) fn questions(&mut self, ctx: &egui::Context, views: &[View]) {
        let shows = Popup::is_id_open(ctx, id()) && self.listing == Listing::Asked;
        for (server, view) in views.iter().enumerate() {
            let asked = view.questions.iter().cloned();
            self.questions.extend(asked.map(|question| (server, question)));
            let over = |(of, question): &(usize, Arc<Question>)| {
                *of == server && (!view.connected || view.settled.contains(&question.call))
            };
            self.questions.retain(|question| !over(question));
            if self.asking.take_if(|question| over(question)).is_some() && shows {
                Popup::close_id(ctx, id());
            }
        }
        if self.asking.is_some() && !shows {
            self.settle(Answer::Refused);
        }
        // The next waits its turn, and for the palette to be free: it does not
        // take over what the user is typing there for something else.
        if self.asking.is_some() || Popup::is_id_open(ctx, id()) {
            return;
        }
        self.asking = self.questions.pop_front();
        if let Some((_, question)) = &self.asking {
            let offered = question.choices.iter().flatten();
            self.chosen = offered.filter(|choice| choice.chosen).map(|c| c.id.clone()).collect();
            self.filter = question.value.clone();
            (self.listing, self.cursor) = (Listing::Asked, None);
            Popup::open_id(ctx, id());
        }
    }

    /// Answer the question on show, which is then over.
    fn settle(&mut self, answer: Answer) {
        if let Some((server, question)) = self.asking.take() {
            self.tell(server, Action::Answer { call: question.call.clone(), answer });
        }
    }

    /// The palette, while it is open: the sessions on every server, the tabs of
    /// the one on show, and what can be done just now, with the keys that do it.
    /// What is typed is looked for among the tabs of every session, too.
    pub(super) fn palette(&mut self, ctx: &egui::Context, views: &[View]) {
        if !Popup::is_id_open(ctx, id()) {
            return;
        }
        let (current, view, map) = (self.current, &views[self.current], self.keys.map.clone());
        let typed = self.filter.trim().to_owned();
        let named = (!typed.is_empty()).then(|| typed.clone());
        let found = |words: &[&str]| found(&typed, words);
        // With nothing typed it lists what it was opened for. What is typed is
        // looked for among the places and the commands alike, but where it was
        // opened for the hosts.
        let (places, deeds) = match self.listing {
            Listing::Places => (true, named.is_some()),
            Listing::Commands => (named.is_some(), true),
            Listing::Host(_) | Listing::Hosts | Listing::Asked => (false, false),
        };
        let row = |label: &str, icon, mark, pick| Row {
            label: label.to_owned(),
            under: String::new(),
            detail: String::new(),
            icon,
            sign: None,
            mark,
            chosen: None,
            keys: String::new(),
            pick,
        };
        let command = |label: &str, name: &str| Row {
            keys: map.shortcut(name),
            ..row(label, None, None, Pick::Do(name.to_owned()))
        };
        // The sessions of each server come under its name, and so first. The
        // one shown before this one is picked out, so that Enter goes back to
        // it, or failing that the one on show.
        let (mut rows, mut under, mut shown, mut back) = (Vec::new(), Vec::new(), 0, None);
        for (server, view) in views.iter().enumerate() {
            for session in view.sessions.iter().filter(|s| places && found(&[&s.name])) {
                if server == current && Some(&session.id) == view.selected_session.as_ref() {
                    shown = rows.len();
                }
                if self.visited[1]
                    .as_ref()
                    .is_some_and(|(at, id)| (*at, id) == (server, &session.id))
                {
                    back = Some(rows.len());
                }
                let show = Pick::Show(server, Action::SelectSession(session.id.clone()));
                rows.push(row(&session.name, None, dot(ctx, session.activity()), show));
            }
            under.push(rows.len());
        }
        // A name that finds no session offers to make one.
        let new = named.clone().filter(|_| places && rows.is_empty());
        let connected: Vec<bool> = views.iter().map(|view| view.connected).collect();
        let makers = makers(current, &connected);
        // What comes after the sessions is headed by what it is.
        let mut heads = vec![(rows.len(), "Tabs")];
        // Those of the session on show come first, by their names alone and
        // with the keys of their places. What is typed is looked for in every
        // other session too, and a tab found there says where it is: in which
        // session, and on which server if that is another.
        let mut others = Vec::new();
        for (server, held) in views.iter().enumerate().filter(|_| places) {
            let shown = held.selected_session.as_ref().filter(|_| server == current);
            let elsewhere = (server != current).then(|| self.servers[server].label());
            let looked = |session: &&Session| named.is_some() || Some(&session.id) == shown;
            for session in held.sessions.iter().filter(looked) {
                let (away, of) = (Some(&session.id) != shown, session.tabs.len());
                let tabs = session.tabs.iter().enumerate();
                for (place, tab) in tabs.filter(|tab| found(&words(session, tab.1))) {
                    let label = [elsewhere, away.then_some(&*session.name), Some(tab.name())];
                    let label = label.into_iter().flatten().collect::<Vec<_>>().join(" › ");
                    let keys = if away { String::new() } else { map.tab(place, of) };
                    let to = Action::SelectTab { session: session.id.clone(), tab: tab.id.clone() };
                    let (icon, mark) = (Some(tab.program()), dot(ctx, tab.says().0));
                    let row = Row { keys, ..row(&label, icon, mark, Pick::Show(server, to)) };
                    if away { others.push(row) } else { rows.push(row) }
                }
            }
        }
        rows.append(&mut others);
        heads.push((rows.len(), "Commands"));
        // All that can be done just now, but to put this list away: found by
        // what it is called here as well as by its title.
        for (name, title, _) in COMMANDS.iter().filter(|_| deeds) {
            let called = label(name, title, &self.remembered.kept);
            if !found(&[title, name, called]) {
                continue;
            }
            let plan = self.standing(view).plan(name, &Value::Null, None);
            let away = |deed| matches!(deed, Deed::Palette(Listing::Places | Listing::Commands));
            if plan.is_ok_and(|deed| !away(deed)) {
                rows.push(command(called, name));
            }
        }
        // A session made on another server than the one on show, as the button
        // beside its name in the sidebar makes one.
        for &server in makers.iter().filter(|&&server| deeds && server != current) {
            let label = format!("New Session on {}", self.servers[server].label());
            if found(&[&label]) {
                let make = Pick::Show(server, Action::CreateSession(None));
                rows.push(row(&label, None, None, make));
            }
        }
        // And those of the server's own actions that are for showing.
        let served = map.served.iter().filter(|served| deeds && !served.1.is_empty());
        for (name, title, _) in served.filter(|served| found(&[&served.1, &served.0])) {
            rows.push(command(title, name));
        }
        // On the server on show first, then on any other, each said by name
        // where there are others.
        for &server in makers.iter().filter(|_| new.is_some()) {
            let name = new.clone().unwrap_or_default();
            let label = match self.servers.len() {
                1 => format!("New Session “{name}”"),
                _ => format!("New Session “{name}” on {}", self.servers[server].label()),
            };
            let make = Pick::Show(server, Action::CreateSession(Some(name)));
            rows.push(row(&label, None, None, make));
        }
        // The host that what is typed comes to, which Enter then adds or
        // changes, or those of home's own list, of which one is taken away.
        match &self.listing {
            Listing::Host(was) => {
                let (doing, head) = match was {
                    Some(_) => ("Change to", "Change Host"),
                    None => ("Add", "Add Remote Host"),
                };
                heads.push((rows.len(), head));
                rows.extend(hosts::typed(&typed, was.as_ref()).map(|host| {
                    let label =
                        format!("{doing} “{}” at {}", host.label, host.endpoints.join(", "));
                    row(&label, None, None, Pick::Home(Action::PutHost(host)))
                }));
            }
            Listing::Hosts => {
                heads.push((rows.len(), "Remove Host"));
                let kept = views[0].hosts.iter().filter(|host| host.source.is_empty());
                for host in kept.filter(|host| found(&[&hosts::line(host)])) {
                    let label = format!("{} at {}", host.label, host.endpoints.join(", "));
                    let remove = Pick::Home(Action::RemoveHost(host.id.clone()));
                    rows.push(row(&label, None, None, remove));
                }
            }
            Listing::Places | Listing::Commands | Listing::Asked => {}
        }
        // What an action asks: what is typed, as it is typed, or one of the
        // things it offers. Where several may be chosen each is ticked, and the
        // row at the foot answers with them all.
        let question = self.asking.clone().filter(|_| self.listing == Listing::Asked);
        let mut foot = None;
        if let Some((_, question)) = &question {
            // It is headed by what it asks, and by nothing else.
            heads.clear();
            let choices = question.choices.iter().flatten();
            for choice in choices.clone().filter(|c| found(&[&c.title, &c.under, &c.id])) {
                let pick = match question.several {
                    true => Pick::Choose(choice.id.clone()),
                    false => Pick::Answer(Answer::Chosen(vec![choice.id.clone()])),
                };
                rows.push(Row {
                    under: choice.under.clone(),
                    detail: choice.detail.clone(),
                    chosen: question.several.then(|| self.chosen.contains(&choice.id)),
                    sign: Icon::named(&choice.icon),
                    ..row(&choice.title, None, None, pick)
                });
            }
            // A line of text goes by what the action calls answering with it.
            if question.choices.is_none() || (question.other && named.is_some()) {
                let label = question.confirm.as_ref().filter(|_| question.choices.is_none());
                let label = label.cloned().unwrap_or_else(|| format!("Use “{}”", self.filter));
                rows.push(row(&label, None, None, Pick::Answer(Answer::Text(self.filter.clone()))));
            }
            if question.several {
                let chosen = choices.filter(|choice| self.chosen.contains(&choice.id));
                let chosen: Vec<_> = chosen.map(|choice| choice.id.clone()).collect();
                let confirm = question.confirm.as_deref().unwrap_or("Done");
                let label = format!("{confirm} · {} chosen", chosen.len());
                let answer = row(&label, None, None, Pick::Answer(Answer::Chosen(chosen)));
                foot = Some(Row { keys: "Ctrl+Enter".into(), ..answer });
            }
        }

        let (mut picked, mut pointed) = (None, None);
        let top = ctx.content_rect().center_top() + egui::vec2(0.0, 56.0);
        let popup = Popup::new(id(), ctx.clone(), top, egui::LayerId::background())
            .open_memory(None)
            .align(RectAlign::BOTTOM)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(520.0);
        popup.show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            // The arrows walk the list while what is typed goes to the field.
            // With Ctrl, Enter answers with all that has been chosen.
            let (step, enter, all) = ui.input_mut(|i| {
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                let all = foot.is_some() && i.consume_key(Modifiers::CTRL, Key::Enter);
                (isize::from(down) - isize::from(up), i.key_pressed(Key::Enter), all)
            });
            // The field is the head of the list, with a line under it and none
            // around it: nothing else here could be taking what is typed.
            let field = egui::TextEdit::singleline(&mut self.filter)
                .hint_text(match self.listing {
                    Listing::Commands => "Search commands…",
                    Listing::Places => "Search sessions and tabs, or name a new session…",
                    Listing::Host(_) => "A name and its address, such as: studio ssh://studio",
                    Listing::Hosts => "Search hosts…",
                    Listing::Asked => question.as_ref().map_or("", |asked| &asked.1.hint),
                })
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)))
                .desired_width(f32::INFINITY);
            let field = ui.add(field);
            let rule = field.rect.bottom() + 3.0;
            let line = Stroke::new(1.0, BORDER);
            ui.painter().hline(ui.max_rect().x_range(), rule, line);
            ui.add_space(6.0);
            // The keyboard is its own while it is open, wherever a click lands.
            if !field.has_focus() {
                field.request_focus();
            }
            // What is typed starts it afresh.
            if field.changed() {
                self.cursor = Some(0);
            }
            // It opens from the top, whatever it was scrolled to when last open.
            let fresh = self.cursor.is_none();
            let moved = ui.input(|i| i.pointer.delta() != Vec2::ZERO);
            let at = self.cursor.unwrap_or(back.unwrap_or(shown)).saturating_add_signed(step);
            let at = at.min(rows.len().saturating_sub(1));
            self.cursor = Some(at);
            let mut row = |ui: &mut egui::Ui, index: usize, row: &Row| {
                // Its keys stand where a mark's word would.
                let keys = Some(row.keys.as_str()).filter(|keys| !keys.is_empty());
                let entry = Item {
                    selected: index == at,
                    icon: row.icon,
                    sign: row.sign,
                    mark: row.mark,
                    word: Some(row.detail.as_str()).filter(|detail| !detail.is_empty()),
                    keys,
                    label: &row.label,
                    under: &row.under,
                    // A tick stands ahead of what may be chosen with others.
                    inset: if row.chosen.is_some() { 24.0 } else { 0.0 },
                    ..Default::default()
                };
                let tall = if row.under.is_empty() { 32.0 } else { ROW };
                let size = Vec2::new(ui.available_width(), tall);
                // A long list draws only what is in sight, and the row that is lit.
                if index != at && !ui.is_rect_visible(Rect::from_min_size(ui.cursor().min, size)) {
                    ui.allocate_space(size);
                    return;
                }
                let (response, _) = item(ui, size, entry);
                if let Some(chosen) = row.chosen {
                    tick(ui, response.rect, chosen);
                }
                if index == at && (step != 0 || fresh) {
                    response.scroll_to_me(None);
                }
                // The pointer picks out the row it moves onto, as the arrows do:
                // one row is lit, and Enter takes that one.
                if moved && response.hovered() {
                    pointed = Some(index);
                }
                if response.clicked() || (index == at && enter) {
                    picked = Some(index);
                }
            };
            // No taller than the window has room for beneath it.
            let room = ui.ctx().content_rect().height() - 140.0;
            // And no shorter than what it lists, where that is less.
            let room = room.clamp(96.0, 420.0);
            let mut list = egui::ScrollArea::vertical().max_height(room).min_scrolled_height(room);
            if fresh {
                list = list.vertical_scroll_offset(0.0);
            }
            list.show(ui, |ui| {
                // A question is headed by what it asks, and by whose it is where
                // that could be another's.
                if let Some((server, question)) = &question {
                    let whose = match self.servers.len() {
                        1 => question.title.clone(),
                        _ => format!("{} · {}", question.title, self.servers[*server].label()),
                    };
                    caption(ui, egui::RichText::new(whose).font(bold(SMALL)));
                }
                let mut listed = rows.iter().enumerate().peekable();
                for (server, view) in views.iter().enumerate() {
                    // A session made on a server takes the name typed, too.
                    if places && self.heading(ui, server, view, false).0 {
                        self.open(server, Action::CreateSession(named.clone()));
                        Popup::close_id(ui.ctx(), id());
                    }
                    while let Some((index, entry)) = listed.next_if(|row| row.0 < under[server]) {
                        row(ui, index, entry);
                    }
                }
                for (index, entry) in listed {
                    // A head with nothing found under it is not shown.
                    let head = heads.iter().rev().find(|head| head.0 <= index);
                    if let Some((_, head)) = head.filter(|head| head.0 == index) {
                        caption(ui, egui::RichText::new(*head).font(bold(SMALL)));
                    }
                    row(ui, index, entry);
                }
            });
            // The row that answers with all that is chosen stays in sight under
            // the list, however long that is.
            if let Some(foot) = &foot {
                ui.add_space(3.0);
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), line);
                ui.add_space(4.0);
                let entry =
                    Item { label: &foot.label, keys: Some(&foot.keys), ..Default::default() };
                let (response, _) = item(ui, Vec2::new(ui.available_width(), 32.0), entry);
                if response.clicked() || all {
                    picked = Some(rows.len());
                }
            }
        });
        self.cursor = pointed.or(self.cursor);
        rows.extend(foot);
        let Some(row) = picked.map(|picked| rows.swap_remove(picked)) else {
            return;
        };
        // Choosing one of several leaves the rest to be chosen from.
        if !matches!(row.pick, Pick::Choose(_)) {
            Popup::close_id(ctx, id());
        }
        match row.pick {
            Pick::Show(server, action) => self.open(server, action),
            Pick::Do(name) => self.run(ctx, view, &name, &Value::Null, "palette"),
            Pick::Home(action) => self.tell(0, action),
            Pick::Answer(answer) => self.settle(answer),
            Pick::Choose(choice) => {
                if !self.chosen.remove(&choice) {
                    self.chosen.insert(choice);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ui_palette.rs"]
mod tests;
