//! What the window remembers from one opening to the next: how the user set it, and
//! where it was. One small file in the user's state directory, on this machine
//! alone: no server is told.
use std::{
    collections::{BTreeMap, HashSet},
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use serde_json::{Value, from_value, json};

use super::{App, DEFAULT_FONT_SIZE, FONT_SIZES, sidebar::SIDEBAR_WIDTH};
use crate::model::{Action, View};

/// The window's size when nothing is remembered of it, and the least it may be.
const WINDOW: [f32; 2] = [1120.0, 760.0];
pub const LEAST_WINDOW: [f32; 2] = [640.0, 420.0];
/// How long a change waits to be saved. A drag makes one at every step.
const PAUSE: Duration = Duration::from_secs(1);

/// Where the user's programs keep what they remember.
#[cfg(windows)]
pub fn state_home() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Where the user's programs keep what they remember.
#[cfg(not(windows))]
pub fn state_home() -> Option<PathBuf> {
    let home = || Some(std::env::home_dir()?.join(".local/state"));
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    state.or_else(home)
}

/// Put a key among those kept, or take it out if it is there.
pub fn flip(kept: &mut HashSet<String>, key: &str) {
    if !kept.remove(key) {
        kept.insert(key.to_owned());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub font_size: f32,
    /// Whether the tabs are in the sidebar, and how wide that is.
    pub sidebar: bool,
    pub sidebar_width: f32,
    /// The window's size.
    pub window: [f32; 2],
    /// The session and the tab last on show on each server, by its address.
    pub shown: BTreeMap<String, [String; 2]>,
    /// The tabs the user wants to hear nothing from.
    pub muted: HashSet<String>,
    /// The servers, by address, and the sessions whose rows are folded away
    /// under their names.
    pub folded: HashSet<String>,
    /// Whether the server's other clients may have this window do things.
    pub remote: bool,
    /// Whether Windows' Mica shows through the window's ground, as it is read
    /// when the window opens: a change shows in the next window.
    pub mica: bool,
}

/// Whether Windows can draw Mica: and so whether it is on where nothing says
/// otherwise, and offered at all.
pub(super) fn backdrops() -> bool {
    #[cfg(windows)]
    return super::frame::backdrops();
    #[cfg(not(windows))]
    false
}

/// As at first, with nothing saved.
impl Default for Kept {
    fn default() -> Self {
        Self::read(&Value::Null)
    }
}

impl Kept {
    /// What a saved file holds. Whatever it lacks, or has wrong, is as it is at first.
    fn read(saved: &Value) -> Self {
        let saved = |key: &str| saved[key].clone();
        let window: [f32; 2] = from_value(saved("window")).unwrap_or(WINDOW);
        let font = |size: f32| size.clamp(FONT_SIZES[0], FONT_SIZES[1]);
        Self {
            font_size: from_value(saved("font_size")).map_or(DEFAULT_FONT_SIZE, font),
            sidebar: from_value(saved("sidebar")).unwrap_or(true),
            sidebar_width: from_value(saved("sidebar_width")).unwrap_or(SIDEBAR_WIDTH),
            // No larger than any screen, whatever the file says.
            window: [0, 1].map(|side| window[side].clamp(LEAST_WINDOW[side], 16384.0)),
            shown: from_value(saved("shown")).unwrap_or_default(),
            muted: from_value(saved("muted")).unwrap_or_default(),
            folded: from_value(saved("folded")).unwrap_or_default(),
            remote: from_value(saved("remote")).unwrap_or(false),
            mica: from_value(saved("mica")).unwrap_or_else(|_| backdrops()),
        }
    }

    fn write(&self) -> Value {
        json!({
            "font_size": self.font_size,
            "sidebar": self.sidebar,
            "sidebar_width": self.sidebar_width,
            "window": self.window,
            "shown": self.shown,
            "muted": self.muted,
            "folded": self.folded,
            "remote": self.remote,
            "mica": self.mica,
        })
    }

    /// What to ask of a server to be back where the window last was on it: at the
    /// tab, or at its session if the tab has gone. Nothing if the session has.
    fn place(&self, server: &str, view: &View) -> Option<Action> {
        let [session, tab] = self.shown.get(server)?.clone();
        let held = view.sessions.iter().find(|held| held.id == session)?;
        Some(match held.tabs.iter().any(|held| held.id == tab) {
            true => Action::SelectTab { session, tab },
            false => Action::SelectSession(session),
        })
    }
}

pub struct Remembered {
    pub kept: Kept,
    /// As it was last saved, or read.
    saved: Kept,
    /// When to save what has changed since.
    due: Option<Instant>,
    /// Where it is kept. With nowhere to, nothing can be saved.
    directory: Option<PathBuf>,
}

impl Remembered {
    /// What was remembered before, in the user's state directory.
    pub fn load() -> Self {
        Self::open(state_home().map(|state| state.join("rexy")))
    }

    /// A file that is missing or damaged is no file: the window starts afresh.
    fn open(directory: Option<PathBuf>) -> Self {
        let file = directory.as_ref().map(|directory| directory.join("state.json"));
        let saved = file.and_then(|file| std::fs::read_to_string(file).ok());
        let saved = saved.and_then(|saved| serde_json::from_str(&saved).ok());
        let kept = Kept::read(&saved.unwrap_or_default());
        Self { saved: kept.clone(), kept, due: None, directory }
    }

    /// Save what has changed, a moment after it first did, and say how long until
    /// then if it is not yet time.
    fn settle(&mut self, now: Instant) -> io::Result<Option<Duration>> {
        if self.kept == self.saved {
            // What changed has been put back, and its wait is over.
            self.due = None;
            return Ok(None);
        }
        let due = *self.due.get_or_insert(now + PAUSE);
        if now < due {
            return Ok(Some(due - now));
        }
        self.save().map(|()| None)
    }

    /// Whole or not at all: the file is written beside its place, then moved there.
    fn save(&mut self) -> io::Result<()> {
        self.due = None;
        self.saved = self.kept.clone();
        let nowhere = || io::Error::other("there is no state directory");
        let directory = self.directory.as_ref().ok_or_else(nowhere)?;
        std::fs::create_dir_all(directory)?;
        let new = directory.join("state.new");
        std::fs::write(&new, format!("{:#}\n", self.kept.write()))?;
        std::fs::rename(new, directory.join("state.json"))
    }
}

impl Drop for Remembered {
    /// The window is closing, and what changed in its last moment is yet to be saved.
    fn drop(&mut self) {
        if self.kept != self.saved {
            let _ = self.save();
        }
    }
}

impl App {
    /// What to ask of the server on show to be back where the window last was on
    /// it. Asked for once: if it cannot be had, the first session stands.
    pub(super) fn returning(&mut self, view: &View) -> Option<Action> {
        let address = self.servers[self.current].host.endpoints.first()?;
        let back = self.remembered.kept.place(address, view);
        self.remembered.kept.shown.remove(address);
        back
    }

    /// Keep what there is to remember as it now stands, and see that it is saved.
    pub(super) fn remember(&mut self, ctx: &egui::Context, views: &[View]) {
        let view = &views[self.current];
        let kept = &mut self.remembered.kept;
        let address = self.servers[self.current].host.endpoints.first();
        if let Some((address, session)) = address.zip(view.selected_session.as_ref()) {
            // A session may have no tab.
            let tab = view.selected_tab.as_deref().unwrap_or_default();
            let same = |[held, shown]: &[String; 2]| held == session && shown == tab;
            if !kept.shown.get(address).is_some_and(same) {
                let place = [session.clone(), tab.to_owned()];
                kept.shown.insert(address.clone(), place);
            }
        }
        // A window that fills the screen is not the size it is otherwise.
        let filling = ctx.input(|i| [i.viewport().maximized, i.viewport().fullscreen]);
        if !filling.contains(&Some(true)) {
            kept.window = ctx.viewport_rect().size().into();
        }
        match self.remembered.settle(Instant::now()) {
            Ok(Some(wait)) => ctx.request_repaint_after(wait),
            Ok(None) => {}
            Err(error) => self.toast.say(format!("Settings could not be saved: {error}")),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ui_remembered.rs"]
mod tests;
