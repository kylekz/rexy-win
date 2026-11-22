use super::*;

#[test]
fn a_cell_is_drawn_by_the_font_for_what_it_holds() {
    // Letters, and symbols written as letters are, whatever room they take.
    assert_eq!(Kind::of("a", false), Kind::Text);
    assert_eq!(Kind::of("日", true), Kind::Text);
    assert_eq!(Kind::of("✓", false), Kind::Text);
    assert_eq!(Kind::of("⚠", false), Kind::Text);
    // A picture is what the terminal gives two cells, of the symbols.
    assert_eq!(Kind::of("😀", true), Kind::Emoji);
    assert_eq!(Kind::of("✅", true), Kind::Emoji);
    assert_eq!(Kind::of("🇺🇸", true), Kind::Emoji);
    assert_eq!(Kind::of("👨\u{200D}👩\u{200D}👧", true), Kind::Emoji);
    // And what asks to be one, or asks not to be.
    assert_eq!(Kind::of("⚠\u{FE0F}", false), Kind::Emoji);
    assert_eq!(Kind::of("❤\u{FE0F}", true), Kind::Emoji);
    assert_eq!(Kind::of("✅\u{FE0E}", true), Kind::Text);
    // A mark is its font's own, from where Unicode leaves room for those.
    assert_eq!(Kind::of("\u{F07C}", false), Kind::Icon);
    assert_eq!(Kind::of("\u{E0A0}", false), Kind::Icon);
    assert_eq!(Kind::of("\u{F0318}", false), Kind::Icon);
}

/// Windows draws the symbols programs print, as `⏵` in Claude Code's status line,
/// with a font of its own that neither egui nor this client brings: none is drawn
/// as a box. A symbol written as a letter is drawn as one, not as a picture.
#[cfg(windows)]
#[test]
fn symbols_are_drawn_by_windows_own_font_for_them() {
    let ctx = egui::Context::default();
    crate::ui::style::apply(&ctx);
    install(&ctx, Vec::new());
    // Braille, and then lines for boxes, which the terminal's own font draws.
    let symbols = ['\u{23F5}', '\u{2714}', '\u{25B6}', '\u{2800}', '\u{2502}', '\u{256C}'];
    let mut output = ctx.run_ui(Default::default(), |ui| {
        ui.fonts_mut(|fonts| {
            for family in [family(false, false), family(true, true)] {
                let font = FontId::new(14.0, family.clone());
                for symbol in symbols {
                    assert!(fonts.has_glyph(&font, symbol), "{family} has no {symbol:?}");
                }
                for symbol in ['\u{23F5}', '\u{2714}'] {
                    let by = fonts.characters(&family).get(&symbol);
                    let by = by.and_then(|by| by.first()).map(String::as_str);
                    assert_eq!(by, Some("Segoe UI Symbol:0"), "{family} {symbol:?}");
                }
            }
        });
    });
    output.textures_delta.clear();
}
