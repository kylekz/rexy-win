use super::*;
use crate::model::{Session, Tab};

fn fresh() -> Kept {
    Kept::read(&Value::Null)
}

/// A directory of this test's own, never the user's.
fn directory(test: &str) -> PathBuf {
    let directory = format!("rexy-{test}-{}", std::process::id());
    let directory = std::env::temp_dir().join(directory);
    let _ = std::fs::remove_dir_all(&directory);
    directory
}

#[test]
fn what_changed_is_saved_after_a_moment_and_read_back() {
    let directory = directory("remembered");
    let open = || Remembered::open(Some(directory.join("state")));
    let mut first = open();
    assert_eq!(first.kept, fresh());
    let start = Instant::now();
    // Nothing has changed, so nothing is written.
    assert_eq!(first.settle(start).unwrap(), None);
    assert!(!directory.exists());
    first.kept.font_size = 18.0;
    first.kept.window = [900.0, 600.0];
    assert_eq!(first.settle(start).unwrap(), Some(PAUSE));
    // What changes meanwhile is saved with it, at the time first set.
    first.kept.sidebar = false;
    // Other clients have no say over the window until they are given it.
    assert!(!first.kept.remote);
    first.kept.remote = true;
    first.kept.mica ^= true;
    first.kept.muted.insert("window:a".into());
    first.kept.folded.extend(["session:s".to_owned(), "unix:/a".to_owned()]);
    let place = ["session:s".to_owned(), "window:a".to_owned()];
    first.kept.shown.insert("unix:/a".into(), place);
    let soon = start + PAUSE / 2;
    assert_eq!(first.settle(soon).unwrap(), Some(PAUSE / 2));
    assert_eq!(open().kept, fresh());
    assert_eq!(first.settle(start + PAUSE).unwrap(), None);
    assert_eq!(open().kept, first.kept);
    // And what changed in the window's last moment, as it closes.
    first.kept.sidebar_width = 300.0;
    let last = first.kept.clone();
    drop(first);
    assert_eq!(open().kept, last);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_damaged_file_starts_afresh() {
    let directory = directory("damaged");
    std::fs::create_dir_all(&directory).unwrap();
    let open = || Remembered::open(Some(directory.clone())).kept.clone();
    let write = |text: &str| std::fs::write(directory.join("state.json"), text).unwrap();
    write("{\"font_size\": 18, \"sideb");
    assert_eq!(open(), fresh());
    // What a file has wrong is as at first, and what it has right is kept.
    let odd = r#"{"font_size": 500, "sidebar": "yes", "sidebar_width": 300,
        "window": [10, 1e9], "shown": [], "muted": ["window:c"], "folded": {"a": 1}}"#;
    write(odd);
    let kept = open();
    assert_eq!((kept.font_size, kept.sidebar), (32.0, true));
    assert_eq!((kept.sidebar_width, kept.window), (300.0, [640.0, 16384.0]));
    assert!(kept.shown.is_empty());
    assert_eq!(kept.muted, HashSet::from(["window:c".to_owned()]));
    assert!(kept.folded.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
    // With nowhere to keep anything, the window still opens, and says so on saving.
    let mut nowhere = Remembered::open(None);
    nowhere.kept.sidebar = false;
    assert!(nowhere.settle(Instant::now() + PAUSE).is_ok());
    assert!(nowhere.settle(Instant::now() + 2 * PAUSE).is_err());
}

#[test]
fn a_window_returns_to_where_it_was_if_that_is_still_there() {
    let tab = |id: &str| Tab { id: id.into(), ..Tab::default() };
    let session = |id: &str, tabs: &[&str]| Session {
        id: id.into(),
        tabs: tabs.iter().copied().map(tab).collect(),
        ..Session::default()
    };
    let mut view = View {
        sessions: vec![session("first", &["a"]), session("second", &["b", "c"])],
        ..View::default()
    };
    let mut kept = fresh();
    let place = ["second".to_owned(), "c".to_owned()];
    kept.shown.insert("unix:/here".into(), place);
    let asked = |kept: &Kept, view: &View| format!("{:?}", kept.place("unix:/here", view));
    let tab = r#"Some(SelectTab { session: "second", tab: "c" })"#;
    assert_eq!(asked(&kept, &view), tab);
    // Each server has its own place.
    assert!(kept.place("unix:/there", &view).is_none());
    // A tab that has gone leaves its session, which shows the tab the server has.
    view.sessions[1].tabs.pop();
    assert_eq!(asked(&kept, &view), r#"Some(SelectSession("second"))"#);
    view.sessions.pop();
    assert_eq!(asked(&kept, &view), "None");
}

#[test]
fn mica_is_as_the_file_says_or_else_as_windows_can_draw_it() {
    let read = |text: &str| Kept::read(&serde_json::from_str(text).unwrap()).mica;
    assert!(read(r#"{"mica": true}"#));
    assert!(!read(r#"{"mica": false}"#));
    // Where the file says nothing of it, or nothing it can mean, Mica is on
    // only where Windows draws it: never off Windows.
    let fresh = fresh().mica;
    assert_eq!(read("{}"), fresh);
    assert_eq!(read(r#"{"mica": "yes"}"#), fresh);
    if !cfg!(windows) {
        assert!(!fresh);
    }
}
