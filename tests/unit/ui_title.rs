use super::*;
use crate::ui::LEAST_WINDOW;

/// What the icon and the sidebar's button take at the left of the row, with
/// the room after them: the button is 34 points in.
const LEAD: f32 = 34.0 + TITLED.size.x + GAP;

/// What the window's buttons and the others take at the right of the row,
/// with the room before each: with the button that brings out hidden panes,
/// or without.
fn trail(hidden: bool) -> f32 {
    3.0 * CAPTION + GAP + buttons(hidden) + GAP
}

fn span(range: Rangef) -> [f32; 2] {
    [range.min, range.max]
}

#[test]
fn the_search_box_is_in_the_middle_of_a_wide_window() {
    let [names, search] = places(1356.0, LEAD, trail(false), NAMES);
    assert_eq!(span(search), [488.0, 868.0]);
    assert_eq!(span(names), [LEAD, 480.0]);
}

#[test]
fn it_moves_from_the_middle_before_it_narrows() {
    let trail = trail(false);
    let [names, search] = places(1000.0, LEAD, trail, NAMES);
    assert_eq!(search.span(), SEARCH[1]);
    assert_eq!(search.max, 1000.0 - trail);
    assert!(names.span() >= NAMES, "{names:?}");
}

#[test]
fn the_names_give_way_first_and_then_the_search_box_narrows() {
    let trail = trail(false);
    let [names, search] = places(LEAST_WINDOW[0], LEAD, trail, NAMES);
    assert_eq!(span(search), [85.0, LEAST_WINDOW[0] - trail]);
    assert_eq!(search.span(), SEARCH[0]);
    assert_eq!(names.span(), 0.0);
}

#[test]
fn tabs_keep_their_room_and_the_search_box_narrows_for_them() {
    let trail = trail(false);
    let keep = 150.0 + 3.0 * (TAB.x + 2.0);
    let [names, search] = places(1356.0, LEAD, trail, keep);
    assert_eq!(span(search), [1356.0 - trail - SEARCH[0], 1356.0 - trail]);
    assert_eq!(names.max, search.min - GAP);
}

#[test]
fn with_too_little_room_the_search_box_takes_all_there_is() {
    let width = LEAST_WINDOW[0];
    let room = width - LEAD - trail(true);
    assert!(room < SEARCH[0], "{room}");
    let [names, search] = places(width, LEAD, trail(true), NAMES);
    assert_eq!(span(search), [LEAD, LEAD + room]);
    assert_eq!(names.span(), 0.0);
}

#[test]
fn nothing_overlaps_at_any_width() {
    for width in (LEAST_WINDOW[0] as u32..=3840).step_by(7) {
        let width = width as f32;
        for (keep, hidden) in
            [0.0, NAMES, 900.0].into_iter().flat_map(|keep| [(keep, false), (keep, true)])
        {
            let trail = trail(hidden);
            let [names, search] = places(width, LEAD, trail, keep);
            assert!(names.min == LEAD && names.max >= names.min, "{width}: {names:?}");
            assert!(search.min >= LEAD && search.max <= width - trail, "{width}: {search:?}");
            assert!(names.span() == 0.0 || names.max + GAP <= search.min, "{width}");
            assert!(search.span() <= SEARCH[1], "{width}: {search:?}");
            // Never narrower than its least, but where there is less room than that.
            let room = width - LEAD - trail;
            assert!(search.span() >= SEARCH[0].min(room), "{width}: {search:?}");
        }
    }
}
