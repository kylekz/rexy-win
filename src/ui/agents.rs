//! The sidebar as a list of what is at work: every pane on every server whose
//! program has something to say, or is a coding agent at rest, under what it
//! is doing, with how long it has been so.
use std::time::{Duration, Instant};

use eframe::egui::{self, Vec2};

use super::{
    App,
    heard::{dot, worn},
    peek,
    style::{Flat, Icon, Item, ROW, SMALL, bold, caption, flat_button, item},
};
use crate::model::{self, Activity, Pane, Session, Tab, View};

/// What the panes are sorted under, in the order they are listed.
const KINDS: [&str; 4] = ["Needs you", "Done", "Working", "Idle"];

/// Which of them a pane belongs under, if any: only a coding agent is listed
/// for having nothing to say.
fn kind(pane: &Pane) -> Option<usize> {
    Some(match pane.activity {
        Activity::Blocked(_) | Activity::Error | Activity::Rang => 0,
        Activity::Done => 1,
        Activity::Working => 2,
        Activity::Idle if pane.program.agent() => 3,
        Activity::Idle => return None,
    })
}

/// How long ago something was, in a word: `now`, `4m`, `2h`, `3d`.
pub fn ago(since: Duration) -> String {
    match since.as_secs() {
        0..60 => "now".to_owned(),
        seconds @ 60..3_600 => format!("{}m", seconds / 60),
        seconds @ 3_600..86_400 => format!("{}h", seconds / 3_600),
        seconds => format!("{}d", seconds / 86_400),
    }
}

/// How long the window must have been out of use for what happened meanwhile
/// to be worth saying on the way back.
const AWAY: Duration = Duration::from_secs(30);

/// What came to want the user while they were away, in a few words: so many
/// panes `needing` them, and so many `done`. Nothing, if none did.
fn recap(needing: usize, done: usize) -> Option<String> {
    let needing = match needing {
        0 => None,
        1 => Some("1 pane needs you".to_owned()),
        many => Some(format!("{many} panes need you")),
    };
    let done = (done > 0).then(|| format!("{done} finished"));
    let both: Vec<_> = needing.into_iter().chain(done).collect();
    (!both.is_empty()).then(|| format!("While you were away: {}.", both.join(" and ")))
}

/// What a pane goes by where it is one of all there are: its session, and its
/// tab where the session has others to tell it from.
pub fn place(session: &Session, tab: &Tab) -> String {
    match session.tabs.len() {
        1 => session.name.clone(),
        _ => format!("{} › {}", session.name, tab.name()),
    }
}

/// A pane that is listed: since when it has been as it is, its server, and the
/// session and the tab it is in.
type Listed<'a> = (Option<Instant>, usize, &'a Session, &'a Tab, &'a Pane);

impl App {
    /// Every pane that is listed, on every server, under its kind. Of each
    /// kind, the one that has been so longest comes first.
    fn listed<'a>(&self, views: &'a [View]) -> [Vec<Listed<'a>>; 4] {
        let mut listed: [Vec<Listed>; 4] = Default::default();
        for (server, view) in views.iter().enumerate() {
            for (session, tab) in view.every_tab() {
                for pane in tab.every() {
                    if let Some(kind) = kind(pane) {
                        let since = self.heard.get(&pane.id).and_then(|heard| heard.since);
                        listed[kind].push((since, server, session, tab, pane));
                    }
                }
            }
        }
        for panes in &mut listed {
            panes.sort_by_key(|listed| listed.0);
        }
        listed
    }

    /// The panes that want the user, in the order the list has them: those
    /// that need them, and then those that are done.
    pub(super) fn wanted(&self, views: &[View]) -> Vec<(usize, String)> {
        let [needing, done, ..] = self.listed(views);
        let wanting = needing.into_iter().chain(done);
        wanting.map(|(_, server, _, _, pane)| (server, pane.id.clone())).collect()
    }

    /// As the window comes back into use, say what came to want the user while
    /// it was out of it, and how to look at each. What is done and on show
    /// has been seen by now, and is not said.
    pub(super) fn returned(&mut self, ctx: &egui::Context, views: &[View]) {
        if !ctx.input(|i| i.focused) {
            self.left.get_or_insert_with(Instant::now);
            return;
        }
        let Some(left) = self.left.take().filter(|left| left.elapsed() >= AWAY) else {
            return;
        };
        let [needing, done, ..] = self.listed(views);
        let new = |panes: &[Listed]| panes.iter().filter(|pane| pane.0 > Some(left)).count();
        if let Some(recap) = recap(new(&needing), new(&done)) {
            let how = match self.keys.map.shortcut(peek::NEXT).as_str() {
                "" => String::new(),
                keys => format!(" {keys} looks at each."),
            };
            self.toast.note(recap + &how);
        }
    }

    /// The button that turns the sidebar to that list and back. It wears the
    /// most pressing thing any pane has to say, on any server.
    pub(super) fn watch(&mut self, ui: &mut egui::Ui, views: &[View], flat: Flat) {
        let tip = match self.activity {
            true => "Sessions and tabs",
            false => "Activity on every server",
        };
        let name = "client.sidebar.activity.toggle";
        let tip = self.keys.map.tip(tip, name);
        let button = flat_button(ui, flat, Icon::Agents, &tip, self.activity);
        let sessions = views.iter().flat_map(|view| &view.sessions);
        worn(ui, &button, sessions.map(|session| session.activity()).max());
        if button.clicked() {
            self.todo.push((name, None));
        }
    }

    /// What a pane's row says of it. Under its name, what its program is waiting
    /// for or failed at, with what the program says of itself. At its edge, how
    /// far along it is, where it says, or how long it has been as it is.
    pub(super) fn told(&self, ctx: &egui::Context, pane: &Pane) -> (String, Option<String>) {
        let word = pane.activity.word().unwrap_or_default();
        let said = model::line(word, &pane.saying());
        let along = pane.along().map(|percent| format!("{percent}%"));
        let edge = along.or_else(|| {
            kind(pane)?;
            let lasted = self.heard.get(&pane.id)?.since?.elapsed();
            // The time is said anew when it comes to another minute.
            let left = 60 - lasted.as_secs() % 60;
            ctx.request_repaint_after(Duration::from_secs(left));
            Some(ago(lasted))
        });
        (said, edge)
    }

    /// The list itself, in place of the servers and their sessions.
    pub(super) fn agents(&mut self, ui: &mut egui::Ui, views: &[View]) {
        let listed = self.listed(views);
        if listed.iter().all(Vec::is_empty) {
            return caption(ui, egui::RichText::new("No activity."));
        }
        let shown = views[self.current].current_tab();
        for (kind, panes) in KINDS.iter().zip(listed) {
            if panes.is_empty() {
                continue;
            }
            // What they are doing heads them, with how many they are.
            let head = format!("{kind} · {}", panes.len());
            caption(ui, egui::RichText::new(head).font(bold(SMALL)));
            for (_, server, session, tab, pane) in panes {
                let name = place(session, tab);
                let last = if session.tabs.len() == 1 { &session.name } else { tab.name() };
                // With nothing said of it, its title, unless it goes by that
                // already. It is under the pointer otherwise.
                let (said, word) = self.told(ui.ctx(), pane);
                let (under, tip) = match said.is_empty() {
                    true => (pane.title.as_str(), ""),
                    false => (said.as_str(), pane.title.as_str()),
                };
                let under = if under == last { "" } else { under };
                let here = server == self.current && shown.is_some_and(|shown| shown.id == tab.id);
                let host = &self.servers[server].host.label;
                let row = Item {
                    id: Some(egui::Id::unique(("at work", host, &pane.id))),
                    icon: Some(pane.program),
                    label: &name,
                    under,
                    selected: here && self.focused.as_ref() == Some(&pane.id),
                    mark: dot(ui.ctx(), pane.activity),
                    word: word.as_deref(),
                    buttons: if here { &[] } else { &[(Icon::Peek, "Peek")] },
                    tip,
                    ..Default::default()
                };
                let size = Vec2::new(ui.available_width(), ROW);
                // A row goes to its pane. Its button looks at the pane from
                // here, where the pane is not on show already.
                match item(ui, size, row) {
                    (_, Some(_)) => self.peek(server, &pane.id),
                    (row, None) if row.clicked() => _ = self.reveal(server, &pane.id),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ui_agents.rs"]
mod tests;
