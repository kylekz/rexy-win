use super::*;

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(400.0, 300.0));

fn window() -> egui::Context {
    let ctx = egui::Context::default();
    apply(&ctx);
    ctx
}

/// Draw rows by these names, each with a close button, while the pointer does
/// `events`. Returns each row's response, and which of its buttons was pressed.
fn draw(
    ctx: &egui::Context,
    rows: &[&str],
    confirm: Option<&str>,
    events: Vec<egui::Event>,
) -> Vec<(egui::Response, Option<usize>)> {
    let input = egui::RawInput { screen_rect: Some(SCREEN), events, ..Default::default() };
    let mut drawn = Vec::new();
    let output = ctx.run_ui(input, |ui| {
        drawn.clear();
        for name in rows {
            let row = Item {
                id: Some(egui::Id::unique(("row", name))),
                label: name,
                buttons: &[(Icon::Close, "Close")],
                confirm,
                ..Default::default()
            };
            drawn.push(item(ui, egui::vec2(200.0, 32.0), row));
        }
    });
    output.drop_without_applying_deltas();
    drawn
}

/// A click of the main button at `pos`, as the drawings it takes.
fn click(pos: Pos2) -> [Vec<egui::Event>; 2] {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    [vec![button(true)], vec![button(false)]]
}

#[test]
fn a_row_keeps_its_name_when_the_row_before_it_goes() {
    let ctx = window();
    let both = draw(&ctx, &["one", "two"], None, vec![]);
    let left = draw(&ctx, &["two"], None, vec![]);
    assert_eq!(both[1].0.id, left[0].0.id);
    assert_ne!(both[0].0.id, left[0].0.id);
}

#[test]
fn a_close_button_asks_again_from_under_the_pointer() {
    let ctx = window();
    let row = draw(&ctx, &["one"], None, vec![])[0].0.rect;
    // Where the button shows once the pointer is on the row.
    let at = egui::pos2(row.right() - 4.0 - SMALL_BUTTON / 2.0, row.center().y);
    draw(&ctx, &["one"], None, vec![egui::Event::PointerMoved(at)]);
    let [press, release] = click(at);
    assert_eq!(draw(&ctx, &["one"], None, press)[0].1, None);
    assert_eq!(draw(&ctx, &["one"], None, release)[0].1, Some(0));
    // It asks, and a second click with the pointer where it was is the answer.
    let asked = draw(&ctx, &["one"], Some("Close?"), vec![]);
    assert_eq!(asked[0].0.rect, row);
    let [press, release] = click(at);
    assert_eq!(draw(&ctx, &["one"], Some("Close?"), press)[0].1, None);
    assert_eq!(draw(&ctx, &["one"], Some("Close?"), release)[0].1, Some(0));
}

#[test]
fn the_icon_is_drawn() {
    let icon = icon();
    assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    // Its corners are clear, and its middle is not.
    let alpha = |x: u32, y: u32| icon.rgba[((y * icon.width + x) * 4 + 3) as usize];
    assert_eq!((alpha(0, 0), alpha(icon.width / 2, icon.height / 2)), (0, 255));
}

#[test]
fn the_ground_is_a_dark_terminals_background_a_little_lighter() {
    let lifted = |value| ground(Color32::from_gray(value)).to_array()[0];
    assert_eq!((lifted(0), lifted(11)), (14, 24));
    // Its hue is kept.
    let navy = ground(Color32::from_rgb(0, 10, 40));
    assert!(navy.b() > navy.g() && navy.g() > navy.r());
    // Light ink would be lost around a light terminal.
    assert_eq!(ground(Color32::WHITE), VIEW);
}

#[test]
fn what_lies_over_the_window_is_lighter_than_its_ground_and_of_its_hue() {
    let brown = Color32::from_rgb(26, 21, 16);
    let (under, over) = (ground(brown), popover(brown));
    assert!(over.r() > under.r() && under.r() > brown.r());
    assert!(over.r() > over.g() && over.g() > over.b());
    assert_eq!(popover(Color32::WHITE), POPOVER);
}

#[test]
fn a_menu_entry_that_cannot_be_taken_does_not_answer_a_click() {
    let ctx = window();
    let draw = |enabled: bool, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(SCREEN), events, ..Default::default() };
        let mut drawn = None;
        let output = ctx.run_ui(input, |ui| {
            let row = Item { label: "Paste", keys: Some("Ctrl+Shift+V"), ..Default::default() };
            drawn = Some(entry(ui, enabled, row));
        });
        output.drop_without_applying_deltas();
        drawn.expect("the entry was drawn")
    };
    for enabled in [false, true] {
        let at = draw(enabled, vec![]).rect.center();
        draw(enabled, vec![egui::Event::PointerMoved(at)]);
        let [press, release] = click(at);
        draw(enabled, press);
        assert_eq!(draw(enabled, release).clicked(), enabled);
    }
}

#[cfg(windows)]
#[test]
fn an_icon_made_smaller_keeps_its_colours_and_lends_none_to_what_is_clear() {
    // Red on its left half, and clear on its right, though green there.
    let pixel = |at: usize| if at % 4 < 2 { [255, 0, 0, 255] } else { [0, 255, 0, 0] };
    let icon = egui::IconData { rgba: (0..16).flat_map(pixel).collect(), width: 4, height: 4 };
    let small = shrink(&icon, 2);
    assert_eq!(small.pixels, [Color32::RED, Color32::TRANSPARENT].repeat(2));
    let least = shrink(&icon, 1);
    assert_eq!(least.pixels, [Color32::from_rgba_premultiplied(127, 0, 0, 127)]);
}
