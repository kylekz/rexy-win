use super::*;

#[test]
fn what_is_typed_finds_a_part_of_any_word_in_any_case() {
    let words = ["Split Pane Right", "pane.split.right"];
    assert!(found("", &words) && found(" ", &words));
    assert!(found("split p", &words) && found("PANE.SP", &words));
    assert!(!found("spilt", &words) && !found("split.pane", &words));
}

#[test]
fn a_session_is_made_on_the_server_on_show_first_then_any_other_connected() {
    assert_eq!(makers(0, &[true]), [0]);
    assert_eq!(makers(1, &[true, true, false, true]), [1, 0, 3]);
    // The one on show is offered even while it is down, as it always was.
    assert_eq!(makers(2, &[true, false, false]), [2, 0]);
}

#[test]
fn a_tab_is_found_by_its_panes_and_by_a_session_it_shares() {
    use crate::model::Pane;
    let pane = |title: &str, program: &str| Pane {
        title: title.into(),
        program: Program::named([program]).unwrap_or_default(),
        ..Default::default()
    };
    let tab = |label: &str, panes| Tab { label: Some(label.into()), panes, ..Default::default() };
    let build = tab("build", vec![pane("~/work/site", "zsh"), pane("Fix the footer", "claude")]);
    let session = |tabs| Session { id: "s".into(), name: "website".into(), active: None, tabs };
    let shared = session(vec![build.clone(), tab("logs", Vec::new())]);
    let finds = |session: &Session, typed: &str| found(typed, &words(session, &session.tabs[0]));
    assert!(finds(&shared, "buil") && finds(&shared, "footer") && finds(&shared, "claude"));
    assert!(finds(&shared, "work/site") && finds(&shared, "websi"));
    assert!(!finds(&shared, "logs"));
    // A session's only tab is found with the session, by the session's own row.
    let alone = session(vec![build]);
    assert!(finds(&alone, "claude") && !finds(&alone, "websi"));
}
