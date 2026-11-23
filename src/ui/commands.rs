//! The client's own actions, under the names Rex gives them. One table says what
//! each is called and which keys it has until a server says otherwise. Keys, menus,
//! the palette and other clients' requests all ask `plan` what doing one comes to.
use eframe::egui;
use serde_json::{Value, json};

use super::{
    App, DEFAULT_FONT_SIZE, FONT_SIZES,
    close::doomed,
    floats, hosts, input,
    keymap::Keys,
    palette::Listing,
    peek,
    remembered::Kept,
    rows::{Choice, Target},
    style::{Flat, Icon, flat_button},
};
use crate::{
    model::{Action, Edit, Float, Pane, Tab, View},
    terminal::{Request, Scroll},
};

/// A command: the name and the title that Rex's action reference gives it, and
/// whether its key, held down, does it again.
pub type Command = (&'static str, &'static str, bool);
const ONCE: bool = false;
const HELD: bool = true;

/// The command that lets a server's other clients have this window do things,
/// or lets them no longer.
pub const REMOTE: &str = "client.remote_control.toggle";
/// The command that has Windows draw Mica behind the window, from when it is
/// next opened, or no longer.
pub const MICA: &str = "client.mica.toggle";
/// The command that gives a pane a tab of its own.
pub const TO_TAB: &str = "client.pane.to_new_tab";
/// The command that copies all a pane has put out.
pub const COPY_ALL: &str = "client.pane.copy_all";
/// A terminal's own action, which a server offers: to clear all it has put out.
pub const CLEAR: &str = "com.superlogical.terminal.clear";
/// The code of the error another client is answered with while they may not.
pub const BARRED: &str = "remote_control_off";

/// Whether a server's other clients may ask this of the window, as remote
/// control stands. With it off they may still have a session shown, which
/// gives nothing away and changes nothing.
pub fn allowed(name: &str, remote: bool) -> bool {
    remote || name == "session.select"
}

/// What the palette calls a command, where that goes by how the window is set:
/// whether remote control is on, and Mica.
pub fn label(name: &str, title: &'static str, kept: &Kept) -> &'static str {
    match name {
        REMOTE if kept.remote => "Refuse Remote Control",
        REMOTE => "Allow Remote Control",
        MICA if kept.mica => "Turn Off Mica Backdrop",
        MICA => "Turn On Mica Backdrop",
        _ => title,
    }
}

/// Rex has no name for the two scrolls to an end, for going back to a tab, for
/// going to what wants the user or for copying all of a pane. Theirs are this
/// client's own.
pub const COMMANDS: &[Command] = &[
    ("session.new", "New Session", ONCE),
    ("session.switch", "Change Session…", ONCE),
    ("session.select", "Go to Session", ONCE),
    ("session.next", "Next Session", HELD),
    ("session.previous", "Previous Session", HELD),
    ("session.rename", "Rename Session", ONCE),
    // Its sessions are read again by connecting again.
    ("session.refresh", "Reconnect", ONCE),
    (hosts::ADD, "Add Remote Host…", ONCE),
    (hosts::REMOVE, "Remove Host…", ONCE),
    // Rex has no name for these, which turn the window to another server.
    (hosts::NEXT, "Next Host", HELD),
    (hosts::PREVIOUS, "Previous Host", HELD),
    ("client.tab.new", "New Tab", ONCE),
    ("client.tab.close", "Close Tab", ONCE),
    ("client.tab.rename", "Rename Tab", ONCE),
    ("client.tab.next", "Show Next Tab", HELD),
    ("client.tab.previous", "Show Previous Tab", HELD),
    ("client.tab.goto", "Go to Tab", ONCE),
    ("client.tab.back", "Go Back to Last Tab", ONCE),
    ("client.tab.move.backward", "Move Tab Backward", ONCE),
    ("client.tab.move.forward", "Move Tab Forward", ONCE),
    ("pane.split.left", "Split Pane Left", ONCE),
    ("pane.split.right", "Split Pane Right", ONCE),
    ("pane.split.up", "Split Pane Up", ONCE),
    ("pane.split.down", "Split Pane Down", ONCE),
    ("pane.move.left", "Move Pane Left", ONCE),
    ("pane.move.right", "Move Pane Right", ONCE),
    ("pane.move.up", "Move Pane Up", ONCE),
    ("pane.move.down", "Move Pane Down", ONCE),
    // Nor for this, which takes a pane out of a tab it shares.
    (TO_TAB, "Move Pane to New Tab", ONCE),
    // Nor for a pane that floats over the others, though its servers have them.
    (floats::FLOAT, "Float or Dock Pane", ONCE),
    (floats::TOGGLE, "Show or Hide Floating Panes", ONCE),
    (floats::NEW, "New Floating Pane", ONCE),
    ("pane.focus.left", "Focus Pane Left", HELD),
    ("pane.focus.right", "Focus Pane Right", HELD),
    ("pane.focus.up", "Focus Pane Up", HELD),
    ("pane.focus.down", "Focus Pane Down", HELD),
    ("pane.focus_next", "Focus Next Pane", HELD),
    ("pane.focus_previous", "Focus Previous Pane", HELD),
    ("pane.resize.left", "Resize Pane Left", HELD),
    ("pane.resize.right", "Resize Pane Right", HELD),
    ("pane.resize.up", "Resize Pane Up", HELD),
    ("pane.resize.down", "Resize Pane Down", HELD),
    ("pane.balance", "Balance Panes", ONCE),
    ("pane.zoom", "Zoom Pane", ONCE),
    ("pane.close", "Close Pane", ONCE),
    ("pane.copy", "Copy from Pane", ONCE),
    (COPY_ALL, "Copy All from Pane", ONCE),
    ("pane.paste", "Paste into Pane", ONCE),
    ("pane.select_all", "Select All in Pane", ONCE),
    ("pane.send_key", "Send Key", ONCE),
    ("client.prompt.previous", "Jump to Previous Prompt", HELD),
    ("client.prompt.next", "Jump to Next Prompt", HELD),
    ("client.find.open", "Find…", ONCE),
    ("client.find.next", "Find Next", HELD),
    ("client.find.previous", "Find Previous", HELD),
    ("client.scroll.page_up", "Scroll Page Up", HELD),
    ("client.scroll.page_down", "Scroll Page Down", HELD),
    ("client.scroll.top", "Scroll to Top", ONCE),
    ("client.scroll.bottom", "Scroll to Bottom", ONCE),
    ("client.font.increase", "Increase Font Size", HELD),
    ("client.font.decrease", "Decrease Font Size", HELD),
    ("client.font.reset", "Reset Font Size", ONCE),
    ("client.sidebar.toggle", "Toggle Sidebar", ONCE),
    ("client.sidebar.activity.toggle", "Show Activity", ONCE),
    ("client.activity.next", "Go to Next Pane That Needs You", ONCE),
    (peek::NEXT, "Peek at Next Pane That Needs You", ONCE),
    ("client.palette.toggle", "Command Palette…", ONCE),
    // Rex has no name for this either. Its own app keeps it among its settings.
    (REMOTE, "Toggle Remote Control", ONCE),
    // Nor for this, which only Windows draws.
    #[cfg(windows)]
    (MICA, "Toggle Mica Backdrop", ONCE),
    ("client.mode.enter", "Enter Mode", ONCE),
    ("client.mode.exit", "Exit Mode", ONCE),
    ("client.mode.exit_all", "Exit All Modes", ONCE),
];

/// The actions Rex also knows by one name for every direction, which is then
/// among their arguments.
const DIRECTED: [Command; 5] = [
    ("pane.split", "Split Pane", ONCE),
    ("pane.move", "Move Pane", ONCE),
    ("pane.focus", "Focus Pane", ONCE),
    ("pane.resize", "Resize Pane", HELD),
    ("client.tab.move", "Move Tab", ONCE),
];

/// The keys this client has for them until a server says otherwise, as Rex
/// spells keys. They are a Linux keyboard's: Ctrl alone belongs to the terminal's
/// programs. Of several for one action the last is tried first, and is the one shown.
pub(super) const KEYS: &[(&str, &str)] = &[
    ("ctrl+shift+n", "session.new"),
    ("ctrl+shift+k", "session.switch"),
    ("ctrl+shift+t", "client.tab.new"),
    ("ctrl+pagedown", "client.tab.next"),
    ("ctrl+tab", "client.tab.next"),
    ("ctrl+pageup", "client.tab.previous"),
    ("ctrl+shift+tab", "client.tab.previous"),
    ("ctrl+shift+pageup", "client.tab.move.backward"),
    ("ctrl+shift+pagedown", "client.tab.move.forward"),
    ("ctrl+shift+l", "client.tab.back"),
    ("ctrl+shift+a", "client.activity.next"),
    ("ctrl+shift+space", peek::NEXT),
    ("ctrl+shift+d", "pane.split.right"),
    ("ctrl+alt+d", "pane.split.down"),
    ("ctrl+alt+left", "pane.focus.left"),
    ("ctrl+alt+right", "pane.focus.right"),
    ("ctrl+alt+up", "pane.focus.up"),
    ("ctrl+alt+down", "pane.focus.down"),
    ("ctrl+alt+shift+left", "pane.resize.left"),
    ("ctrl+alt+shift+right", "pane.resize.right"),
    ("ctrl+alt+shift+up", "pane.resize.up"),
    ("ctrl+alt+shift+down", "pane.resize.down"),
    ("ctrl+alt+=", "pane.balance"),
    ("ctrl+shift+enter", "pane.zoom"),
    ("ctrl+shift+s", floats::TOGGLE),
    ("ctrl+alt+f", floats::FLOAT),
    ("ctrl+shift+w", "pane.close"),
    // The keys some keyboards have for these. Rex has no spelling for them, and
    // keeps this one as it comes.
    ("[Copy]", "pane.copy"),
    ("[Paste]", "pane.paste"),
    ("ctrl+shift+c", "pane.copy"),
    ("ctrl+shift+v", "pane.paste"),
    ("ctrl+shift+f", "client.find.open"),
    ("ctrl+shift+up", "client.prompt.previous"),
    ("ctrl+shift+down", "client.prompt.next"),
    ("shift+pageup", "client.scroll.page_up"),
    ("shift+pagedown", "client.scroll.page_down"),
    ("shift+home", "client.scroll.top"),
    ("shift+end", "client.scroll.bottom"),
    // On most keyboards a plus is typed with Shift.
    ("ctrl++", "client.font.increase"),
    ("ctrl+shift+=", "client.font.increase"),
    ("ctrl+=", "client.font.increase"),
    ("ctrl+-", "client.font.decrease"),
    ("ctrl+0", "client.font.reset"),
    ("ctrl+shift+b", "client.sidebar.toggle"),
    ("ctrl+shift+p", "client.palette.toggle"),
];

/// The name an action goes by in the table.
pub fn named(name: &str, args: &Value) -> String {
    let directed = DIRECTED.iter().any(|directed| directed.0 == name);
    match args["direction"].as_str().filter(|_| directed) {
        Some(direction) => format!("{name}.{direction}"),
        None => name.to_owned(),
    }
}

/// A command by its name, if it is one.
pub fn command(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.0 == name)
}

/// Whether a command in the table is one this window can do at all: Mica is
/// only where Windows can draw it.
fn offered(name: &str) -> bool {
    name != MICA || super::remembered::backdrops()
}

/// What menus and the palette call a command.
pub fn title(name: &str) -> &'static str {
    command(name).map_or("", |command| command.1)
}

/// The way a command goes that is named for one: a step along x or y.
fn toward(name: &str) -> Option<[f32; 2]> {
    Some(match name.rsplit_once('.')?.1 {
        "left" => [-1.0, 0.0],
        "right" => [1.0, 0.0],
        "up" => [0.0, -1.0],
        "down" => [0.0, 1.0],
        _ => return None,
    })
}

/// The move through a pane's history that a command makes, if it makes one.
fn scroll(name: &str) -> Option<Scroll> {
    Some(match name {
        "client.scroll.page_up" => Scroll::Pages(-1),
        "client.scroll.page_down" => Scroll::Pages(1),
        // Further than any history goes.
        "client.scroll.top" => Scroll::Lines(i32::MIN),
        "client.scroll.bottom" => Scroll::Bottom,
        "client.prompt.previous" => Scroll::Prompts(-1),
        "client.prompt.next" => Scroll::Prompts(1),
        _ => return None,
    })
}

/// What a server's keymap is asked with: what this client can do itself, and
/// the keys it does it by unless the account's `init.lua` says otherwise.
pub fn request() -> Value {
    let action = |(name, title, repeats): &Command| {
        let kind = "client";
        json!({"name": name, "kind": kind, "title": title, "repeats": repeats})
    };
    let actions: Vec<Value> = COMMANDS.iter().chain(&DIRECTED).map(action).collect();
    let bound = |(key, name): &(&str, &str)| json!({"keys": [key], "action": name});
    // With a key for each of a session's first eight tabs and for its last, as
    // browsers have them.
    let tab = |n: i64| {
        let args = json!({"index": if n == 9 { -1 } else { n }});
        json!({"keys": [format!("alt+{n}")], "action": "client.tab.goto", "args": args})
    };
    let bindings: Vec<Value> = KEYS.iter().map(bound).chain((1..=9).map(tab)).collect();
    json!({"actions": actions, "defaults": [{"name": "default", "bindings": bindings}]})
}

/// What doing a command comes to.
pub(super) enum Deed {
    /// Something for the server to do.
    Server(Action),
    /// Showing a session or a tab of some server's, which may be another.
    Show(usize, Action),
    /// Showing a pane of some server's, whatever tab it is in.
    Reveal(usize, String),
    /// Looking at a pane of some server's from the tab on show.
    Peek(usize, String),
    /// Turning the window to another server, where it shows what it last
    /// showed there.
    Turn(usize),
    /// Taking the keyboard to a pane.
    Focus(String),
    Font(f32),
    Sidebar,
    /// Turning the sidebar to the list of what is at work, or back.
    Activity,
    /// Opening the palette on one of the things it lists, or putting it away.
    Palette(Listing),
    /// Letting a server's other clients have this window do things, or no longer.
    Remote,
    /// Having Windows draw Mica behind the window when it is next opened, or not.
    #[cfg(windows)]
    Mica,
    /// Something for a pane's own terminal to do, which no server is told of:
    /// about what is selected or looked for in it, or to move through its history.
    Terminal(String, Request),
    /// Opening the field that finds text in a pane.
    Find(String),
    /// Pasting what the desktop's clipboard holds into a pane.
    Paste(String),
    /// Closing a pane by an action, unless programs may be lost by it and this
    /// is the first asking: by its name, and whether nothing would be lost.
    Close(egui::Id, bool, Action),
    /// Closing a tab of a session, likewise.
    CloseTab(String, String, bool),
    /// Opening the field that renames a session, or the tab of it that is
    /// given, on the name it goes by.
    Rename(String, Option<String>, String),
    /// Entering a mode, for one key only or until it is left.
    Enter(String, bool),
    /// Leaving the last mode entered, or them all.
    Exit(bool),
}

/// How the window stands, as far as what can be done in it goes.
pub(super) struct Standing<'a> {
    /// What the server a command is for has.
    pub view: &'a View,
    /// The pane with the keyboard in the tab on show.
    pub focused: Option<&'a str>,
    pub font_size: f32,
    pub sidebar: bool,
    pub keys: &'a Keys,
    /// The panes that want the user, on every server, each with its server.
    pub wanting: &'a [(usize, String)],
    /// The pane looked at from elsewhere, if one is.
    pub peeked: Option<&'a str>,
    /// The tab shown before the one on show, with its session and that one's server.
    pub back: Option<&'a ((usize, String), String)>,
    /// Some host that home names is its list's own to remove.
    pub removable: bool,
    /// The server on show, and of every server whether it has a session to
    /// turn the window to.
    pub server: usize,
    pub hosts: &'a [bool],
}

impl Standing<'_> {
    /// What a command would do just now: to the `pane` given, or else to the
    /// one with the keyboard. In place of that, the code of the error another
    /// client is answered with: `not_supported` for a name that is not in the
    /// table, or for what this Windows cannot do, and `refused` for what
    /// cannot be done just now.
    pub fn plan(&self, name: &str, args: &Value, pane: Option<&str>) -> Result<Deed, &'static str> {
        let name = named(name, args);
        if command(&name).is_none() || !offered(&name) {
            return Err("not_supported");
        }
        self.deed(&name, args, pane).ok_or("refused")
    }

    /// Whether a key bound to a command is the window's. It is whether or not
    /// the command can be done just now: to a program, Ctrl+Shift+C is Ctrl+C.
    /// Only the keys that move through a pane's history are the program's
    /// when there is none to move through.
    pub fn keeps(&self, name: &str, args: &Value) -> bool {
        match self.plan(name, args, None) {
            Ok(_) => true,
            Err(code) => code == "refused" && scroll(name).is_none(),
        }
    }

    /// The pane that wants the user after one of them, round and round, or
    /// the first of them: but not that pane itself.
    fn after(&self, pane: Option<&str>) -> Option<&(usize, String)> {
        let here = |(_, held): &(usize, String)| Some(held.as_str()) == pane;
        let after = self.wanting.iter().position(here).map_or(0, |at| at + 1);
        self.wanting.iter().cycle().nth(after).filter(|next| !here(next))
    }

    fn deed(&self, name: &str, args: &Value, pane: Option<&str>) -> Option<Deed> {
        let view = self.view;
        let text = |key: &str| args[key].as_str().map(str::to_owned);
        // The next of some things or the one before, round and round.
        let on = name.ends_with("next");
        let round = |at: usize, of: usize| (at + if on { 1 } else { of - 1 }) % of;
        let (tab, tabs) = (view.current_tab(), view.tabs());
        let at = tabs.iter().position(|tab| Some(&tab.id) == view.selected_tab.as_ref());
        let pane = pane.or(self.focused);
        let pane = pane.and_then(|id| tab?.panes.iter().find(|pane| pane.id == id));
        // What the pane shows, once its terminal has drawn.
        let frame = pane.and_then(|pane| pane.frame.as_ref());
        let session = || view.selected_session.clone();
        let show = |index: usize| {
            let tab = tabs.get(index)?.id.clone();
            let session = session()?;
            Some(Deed::Server(Action::SelectTab { session, tab }))
        };
        let moved = |before: Option<&Tab>| {
            Some(Deed::Server(Action::Edit {
                session: session()?,
                tab: Some(tab?.id.clone()),
                edit: Edit::MoveBefore(before.map(|tab| tab.id.clone())),
            }))
        };
        // First what needs no server.
        Some(match name {
            "client.sidebar.toggle" => Deed::Sidebar,
            "client.sidebar.activity.toggle" => Deed::Activity,
            "client.activity.next" => {
                let (server, pane) = self.after(self.focused)?.clone();
                Deed::Reveal(server, pane)
            }
            // The same, from the pane looked at where one is, and looked at
            // from here: but one in the tab on show is gone to. With no other
            // there is nothing to do, and a look is over.
            peek::NEXT => {
                let (server, pane) = self.after(self.peeked.or(self.focused))?.clone();
                match tab.is_some_and(|tab| tab.every().any(|held| held.id == pane)) {
                    true => Deed::Reveal(server, pane),
                    false => Deed::Peek(server, pane),
                }
            }
            // What is pasted is typed, and a pane looked at has the keyboard.
            "pane.paste" if let Some(pane) = self.peeked => Deed::Paste(pane.to_owned()),
            "client.tab.back" => {
                let ((server, session), tab) = self.back?.clone();
                Deed::Show(server, Action::SelectTab { session, tab })
            }
            "client.palette.toggle" => Deed::Palette(Listing::Commands),
            "session.switch" => Deed::Palette(Listing::Places),
            hosts::ADD => Deed::Palette(Listing::Host(None)),
            hosts::REMOVE if self.removable => Deed::Palette(Listing::Hosts),
            // The next server with a session, or the one before, round and
            // round: but not the one on show.
            hosts::NEXT | hosts::PREVIOUS => {
                let of = self.hosts.len();
                let mut at = self.server;
                let next = (1..of).find_map(|_| {
                    at = round(at, of);
                    self.hosts[at].then_some(at)
                });
                Deed::Turn(next?)
            }
            REMOTE => Deed::Remote,
            #[cfg(windows)]
            MICA => Deed::Mica,
            "client.font.increase" => Deed::Font(self.font_size + 1.0),
            "client.font.decrease" => Deed::Font(self.font_size - 1.0),
            "client.font.reset" => Deed::Font(DEFAULT_FONT_SIZE),
            "session.refresh" => Deed::Server(Action::Reconnect),
            "client.mode.enter" => {
                let name = text("name").filter(|name| self.keys.map.has_mode(name))?;
                Deed::Enter(name, args["once"] == true)
            }
            "client.mode.exit" | "client.mode.exit_all" if !self.keys.modes.is_empty() => {
                Deed::Exit(name.ends_with("all"))
            }
            // Then what a pane's own terminal does with what it shows, which may
            // be the last screen of a connection that was lost.
            "pane.copy" => {
                frame.filter(|frame| !frame.selected.is_empty())?;
                Deed::Terminal(pane?.id.clone(), Request::Copy)
            }
            "pane.select_all" => Deed::Terminal(frame.and(pane)?.id.clone(), Request::SelectAll),
            "client.find.open" => Deed::Find(frame.and(pane)?.id.clone()),
            // The match before the one shown, or the one after it, of a search
            // under way.
            "client.find.next" | "client.find.previous" => {
                frame?.found?;
                Deed::Terminal(pane?.id.clone(), Request::Step { older: on })
            }
            // And what moves through a pane's history: unless its program has the
            // whole screen, which keeps none.
            _ if scroll(name).is_some() => {
                frame.filter(|frame| !frame.alternate_screen)?;
                Deed::Terminal(pane?.id.clone(), Request::Scroll(scroll(name)?))
            }
            // The rest is done by a server.
            _ if !view.connected => return None,
            "session.new" => Deed::Server(Action::CreateSession(None)),
            "session.select" => {
                let listed = |id: &String| view.sessions.iter().any(|session| &session.id == id);
                let session = text("session_id").filter(listed)?;
                Deed::Server(match text("window_id") {
                    Some(tab) => Action::SelectTab { session, tab },
                    None => Action::SelectSession(session),
                })
            }
            "session.next" | "session.previous" => {
                let sessions = &view.sessions;
                let at = sessions.iter().position(|s| Some(&s.id) == session().as_ref())?;
                let to = &sessions[round(at, sessions.len())];
                Deed::Server(Action::SelectSession(to.id.clone()))
            }
            // A session's name is in the sidebar, and nowhere else.
            "session.rename" => {
                let session = view.session().filter(|_| self.sidebar)?;
                Deed::Rename(session.id.clone(), None, session.name.clone())
            }
            "client.tab.new" => Deed::Server(Action::CreateTab(session()?)),
            "client.tab.close" => Deed::CloseTab(session()?, tab?.id.clone(), tab?.idle()),
            // The field opens on the name it was given, if it was given one:
            // what it goes by otherwise is its terminal's title, and no name.
            "client.tab.rename" => {
                let (id, name) = (tab?.id.clone(), tab?.label.clone().unwrap_or_default());
                Deed::Rename(session()?, Some(id), name)
            }
            "client.tab.next" | "client.tab.previous" => show(round(at?, tabs.len()))?,
            // From the first, which is 1, or from the last, which is -1.
            "client.tab.goto" => {
                let index = args["index"].as_i64()?;
                let from = if index < 0 { tabs.len() as i64 } else { -1 };
                show(usize::try_from(index + from).ok()?)?
            }
            "client.tab.move.backward" => moved(Some(tabs.get(at?.checked_sub(1)?)?))?,
            // Before the one after the next, or last of all.
            "client.tab.move.forward" if at? + 1 < tabs.len() => moved(tabs.get(at? + 2))?,
            "pane.split.left" | "pane.split.right" | "pane.split.up" | "pane.split.down" => {
                Deed::Server(Action::Split { pane: pane?.id.clone(), toward: toward(name)? })
            }
            // It changes places with the pane on that side of it.
            "pane.move.left" | "pane.move.right" | "pane.move.up" | "pane.move.down" => {
                let with = tab?.beside(&pane?.id, toward(name)?)?.id.clone();
                Deed::Server(Action::Swap { pane: pane?.id.clone(), with })
            }
            // Another pane floats over the tab: and with none that does, the
            // command that goes to them makes one.
            floats::NEW | floats::TOGGLE if name == floats::NEW || tab?.floats.is_empty() => {
                let (tab, bounds) = (tab?.id.clone(), Float::first(tab?.floats.len()));
                Deed::Server(Action::NewFloat { session: session()?, tab, bounds })
            }
            // From a pane that floats over the tab, the layers that do are put
            // away together, where that leaves a pane on show. From another
            // it goes to the one in front, behind whatever hides it: those put
            // away are brought back, and a pane that fills the tab makes way.
            floats::TOGGLE => {
                let (tab, session) = (tab?, session()?);
                let layers = |away: bool| {
                    let layers = tab.floats.iter().filter(|float| float.away == away);
                    layers.map(|float| float.id.clone()).collect()
                };
                Deed::Server(if pane.is_some_and(|pane| pane.float.is_some()) {
                    tab.tiled().next()?;
                    Action::Stow { session, layers: layers(false), back: None }
                } else {
                    let held = tab.fore(tab.floats.last()?)?;
                    Action::Stow { session, layers: layers(true), back: Some(held.id.clone()) }
                })
            }
            // A pane of the layout floats over it, but for the last one there.
            // One that floats goes back into it, beside the largest pane
            // there, which has the most room to give: or, the layout empty,
            // is given all of the tab.
            floats::FLOAT => {
                let (pane, tab) = (pane?, tab?);
                let room = |pane: &&Pane| pane.width * pane.height;
                let largest = tab.tiled().min_by(|a, b| room(b).total_cmp(&room(a)));
                Deed::Server(match (&pane.float, largest) {
                    (None, _) => {
                        tab.tiled().nth(1)?;
                        let bounds = Float::first(tab.floats.len());
                        Action::Float { pane: pane.id.clone(), bounds }
                    }
                    (Some(_), Some(beside)) => {
                        Action::Dock { pane: pane.id.clone(), beside: beside.id.clone() }
                    }
                    (Some(layer), None) => {
                        let (session, layer) = (session()?, layer.clone());
                        Action::Place { session, layer, bounds: [0.0, 0.0, 1.0, 1.0] }
                    }
                })
            }
            // Alone in its tab, it has one already.
            TO_TAB if tab?.every().count() > 1 => {
                Deed::Server(Action::MovePane { pane: pane?.id.clone(), tab: None })
            }
            "pane.focus.left" | "pane.focus.right" | "pane.focus.up" | "pane.focus.down" => {
                Deed::Focus(tab?.beside(&pane?.id, toward(name)?)?.id.clone())
            }
            // The border of the split the pane is in moves that way.
            "pane.resize.left" | "pane.resize.right" | "pane.resize.up" | "pane.resize.down" => {
                Deed::Server(Action::Divide(vec![tab?.nudged(&pane?.id, toward(name)?)?]))
            }
            // Even already, there is nothing to ask.
            "pane.balance" => {
                let borders = Some(tab?.balanced()).filter(|borders| !borders.is_empty());
                Deed::Server(Action::Divide(borders?))
            }
            "pane.focus_next" | "pane.focus_previous" if tab?.panes.len() > 1 => {
                let (pane, panes) = (pane?, &tab?.panes);
                let at = panes.iter().position(|held| held.id == pane.id)?;
                Deed::Focus(panes[round(at, panes.len())].id.clone())
            }
            "pane.zoom" => {
                let pane = pane.filter(|_| tab.is_some_and(Tab::split))?.id.clone();
                Deed::Server(Action::Zoom { pane })
            }
            "pane.close" => {
                let (what, spared, pane) = (doomed(&pane?.id), pane?.idle, pane?.id.clone());
                Deed::Close(what, spared, Action::ClosePane { pane })
            }
            "pane.paste" => Deed::Paste(pane?.id.clone()),
            // The server keeps it all, and what the pane shows here is part of it.
            COPY_ALL => Deed::Server(Action::CopyAll { pane: frame.and(pane)?.id.clone() }),
            "pane.send_key" => Deed::Server(input::press(&pane?.id, args["key"].as_str()?)?),
            _ => return None,
        })
    }
}

impl App {
    pub(super) fn standing<'a>(&'a self, view: &'a View) -> Standing<'a> {
        Standing {
            view,
            focused: self.focused.as_deref(),
            font_size: self.remembered.kept.font_size,
            sidebar: self.remembered.kept.sidebar,
            keys: &self.keys,
            wanting: &self.wanting,
            peeked: self.peek.as_ref().map(|peek| peek.pane.as_str()),
            back: self.stood[1].as_ref(),
            removable: self.servers[1..].iter().any(|server| server.host.source.is_empty()),
            server: self.current,
            hosts: &self.hosts,
        }
    }

    /// A button that does a command, when it can be done. Under the pointer it
    /// says what it does, with the keys that do the same.
    pub(super) fn button(
        &mut self,
        ui: &mut egui::Ui,
        view: &View,
        flat: Flat,
        icon: Icon,
        label: &str,
        name: &'static str,
    ) {
        let can = self.standing(view).plan(name, &Value::Null, None).is_ok();
        ui.add_enabled_ui(can, |ui| {
            if flat_button(ui, flat, icon, &self.keys.map.tip(label, name), false).clicked() {
                self.todo.push((name, None));
            }
        });
    }

    /// Do what a command comes to, on the server it was planned for. `near` says
    /// the pointer asked.
    pub(super) fn act(&mut self, ctx: &egui::Context, server: usize, deed: Deed, near: bool) {
        let row = |session, tab| Target { server, session, tab };
        match deed {
            Deed::Server(action) => self.open(server, action),
            Deed::Show(server, action) => self.open(server, action),
            Deed::Reveal(server, pane) => _ = self.reveal(server, &pane),
            Deed::Peek(server, pane) => self.peek(server, &pane),
            Deed::Turn(server) => self.next = Some(server),
            Deed::Focus(pane) => self.focus(pane),
            Deed::Font(size) => {
                self.remembered.kept.font_size = size.clamp(FONT_SIZES[0], FONT_SIZES[1]);
            }
            Deed::Sidebar => self.remembered.kept.sidebar ^= true,
            // The list is the sidebar's, which is brought out for it.
            Deed::Activity => {
                self.activity ^= true;
                self.remembered.kept.sidebar |= self.activity;
            }
            Deed::Palette(listing) => self.list(ctx, listing),
            Deed::Remote => {
                let remote = &mut self.remembered.kept.remote;
                *remote ^= true;
                let said = match *remote {
                    true => "Other clients of a server may now control this window.",
                    false => "Other clients may no longer control this window.",
                };
                self.toast.note(said.into());
            }
            #[cfg(windows)]
            Deed::Mica => {
                let mica = &mut self.remembered.kept.mica;
                *mica ^= true;
                let said = match *mica {
                    true => "Mica shows behind the window when Rexy is next opened.",
                    false => "The window has no Mica when Rexy is next opened.",
                };
                self.toast.note(said.into());
            }
            Deed::Terminal(pane, request) => self.ask(server, &pane, request),
            Deed::Find(pane) => self.find(&pane),
            // The clipboard answers in a moment, to the pane with the keyboard.
            Deed::Paste(pane) => {
                if self.peek.is_none() {
                    self.focused = Some(pane);
                }
                self.paste(ctx, true);
            }
            Deed::Close(what, spared, action) => self.close(what, spared, near, server, action),
            // It asks again by the name of its row, wherever that is said: on
            // the row, on its session's where it is the only tab, or over the
            // terminals while the sidebar lists what is at work.
            Deed::CloseTab(session, tab, spared) => {
                self.shut(row(session, Some(tab)), spared, near);
            }
            // In a field where its name is, likewise.
            Deed::Rename(session, tab, name) => {
                self.choose(row(session, tab), &name, Choice::Rename);
            }
            Deed::Enter(name, once) => self.keys.modes.push((name, once)),
            Deed::Exit(all) => {
                let left = self.keys.modes.len().saturating_sub(1);
                self.keys.modes.truncate(if all { 0 } else { left });
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ui_commands.rs"]
mod tests;
