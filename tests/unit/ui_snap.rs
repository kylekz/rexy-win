use super::*;

/// The button that spreads the window, 46 points wide and 48 tall, at the
/// right of a window 1356 points wide, half again as many pixels to a point.
fn button() -> Pixels {
    let rect = egui::Rect::from_x_y_ranges(1356.0 - 92.0..=1356.0 - 46.0, 0.0..=48.0);
    Pixels::of(rect, 1.5)
}

#[test]
fn the_button_is_in_pixels_as_drawn() {
    assert_eq!(button(), Pixels { left: 1896, top: 0, right: 1965, bottom: 72 });
}

#[test]
fn its_points_are_rounded_to_the_nearest_pixel() {
    let rect = egui::Rect::from_x_y_ranges(10.3..=20.7, 5.0..=50.0);
    let pixels = Pixels::of(rect, 1.25);
    assert_eq!(pixels, Pixels { left: 13, top: 6, right: 26, bottom: 63 });
}

#[test]
fn it_holds_every_pixel_just_within_each_edge() {
    let b = button();
    for point in
        [[b.left, b.top], [b.right - 1, b.top], [b.left, b.bottom - 1], [b.right - 1, b.bottom - 1]]
    {
        assert!(b.holds(point), "{point:?}");
    }
    assert!(b.holds([(b.left + b.right) / 2, (b.top + b.bottom) / 2]));
}

#[test]
fn it_holds_none_just_past_any_edge() {
    let b = button();
    let middle = [(b.left + b.right) / 2, (b.top + b.bottom) / 2];
    for point in [
        [b.left - 1, middle[1]],
        [b.right, middle[1]],
        [middle[0], b.top - 1],
        [middle[0], b.bottom],
    ] {
        assert!(!b.holds(point), "{point:?}");
    }
}

#[test]
fn none_holds_nothing() {
    for point in [[0, 0], [-1, -1], [1, 1], [i32::MIN, i32::MAX]] {
        assert!(!Pixels::NONE.holds(point), "{point:?}");
    }
}

#[test]
fn a_rect_with_no_width_holds_nothing() {
    let rect = egui::Rect::from_x_y_ranges(30.0..=30.0, 0.0..=48.0);
    let pixels = Pixels::of(rect, 1.5);
    assert!(!pixels.holds([45, 10]));
}
