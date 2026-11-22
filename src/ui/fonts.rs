//! The fonts a terminal's text is drawn in: those the user names for Ghostty,
//! the faces that come with this client, and those of this machine that draw
//! what neither can. Pictures of things and the marks of programs each have a
//! font found for them alone, so that neither is ever drawn by whichever font
//! happened to be found first.
use std::sync::{Arc, LazyLock};

use eframe::egui::{
    self, FontData, FontFamily, FontId,
    epaint::text::{
        FallbackRequest, FontInsert, FontPriority, FontProvider, FontTweak, FontsView,
        InsertFontFamily, VariationCoords,
    },
    mutex::Mutex,
};
use fontique::{
    Attributes, Collection, CollectionOptions, FontStyle, FontWeight, FontWidth, GenericFamily,
    QueryFamily, QueryFont, QueryStatus, SourceCache,
};

/// The family that draws emoji, in their colours.
const EMOJI: &str = "emoji";
/// The family that draws the marks of programs and of places: the icons that
/// Nerd Fonts keep where Unicode leaves room for anyone's own.
const ICONS: &str = "icons";
/// Windows' own font of symbols, which Windows itself draws a symbol of no script
/// in. fontique names it as no kind of font, so the desktop's choice never comes to
/// it, and what only it has, as `⏵`, would be drawn as a box.
#[cfg(windows)]
const SYMBOLS: &str = "Segoe UI Symbol";

/// What a cell's text is, by the font that should draw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Letters, and the symbols that are written as letters are.
    Text,
    /// A picture of a thing, in its own colours.
    Emoji,
    /// A mark with no meaning but the one its font gives it.
    Icon,
}

impl Kind {
    /// What the text of a cell is, which is `wide` if it takes two. A symbol is
    /// a picture where its text asks for that, and is not where it asks for
    /// the plain one. Unasked, it is one if the terminal gives it two cells:
    /// that is how the terminal tells the same thing.
    pub fn of(text: &str, wide: bool) -> Self {
        let first = text.chars().next().map_or(0, u32::from);
        match first {
            0xE000..=0xF8FF | 0xF_0000.. => Self::Icon,
            _ if text.contains('\u{FE0E}') => Self::Text,
            _ if text.contains('\u{FE0F}') => Self::Emoji,
            0x2300..=0x2BFF | 0x1_F000..=0x1_FAFF if wide => Self::Emoji,
            _ => Self::Text,
        }
    }

    /// The font that draws text of this kind, where this machine has one for
    /// it: at a `size`, or at the largest below it that draws the text no
    /// wider than `most`. Without one the text is left to the terminal's own
    /// font, and whatever stands behind that.
    pub fn font(
        self,
        fonts: &mut FontsView<'_>,
        text: &str,
        size: f32,
        most: f32,
    ) -> Option<FontId> {
        let family = match self {
            Self::Text => return None,
            Self::Emoji => EMOJI,
            Self::Icon => ICONS,
        };
        let (first, font) =
            (text.chars().next()?, FontId::new(size, FontFamily::Name(family.into())));
        if !fonts.has_glyph(&font, first) {
            return None;
        }
        let fit = (most / fonts.glyph_width(&font, first)).min(1.0);
        Some(FontId::new(size * fit, font.family))
    }
}

/// The family of the terminal's own font that draws text that is `bold`,
/// `italic`, both or neither.
pub fn family(bold: bool, italic: bool) -> FontFamily {
    match (bold, italic) {
        (false, false) => FontFamily::Monospace,
        (true, false) => FontFamily::Name("mono-bold".into()),
        (false, true) => FontFamily::Name("mono-italic".into()),
        (true, true) => FontFamily::Name("mono-bold-italic".into()),
    }
}

/// The fonts of this machine, which are listed when first one is asked for.
struct Machine {
    /// The families the user names for the terminal's text.
    named: Vec<String>,
    listed: Mutex<LazyLock<Listed>>,
}

type Listed = (Collection, SourceCache);

impl Machine {
    /// Put the families the user names at the head of each of the terminal's
    /// faces, so that this client's own draw only what those cannot. A family
    /// with no face of a look draws that look in the nearest it has, as it
    /// does in Ghostty: its italic text is upright where it has none that leans.
    fn lead(&self, ctx: &egui::Context) {
        let mut listed = self.listed.lock();
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let weight = if bold { FontWeight::BOLD } else { FontWeight::NORMAL };
            let style = if italic { FontStyle::Italic } else { FontStyle::Normal };
            let look = Attributes::new(FontWidth::NORMAL, style, weight);
            let ahead =
                InsertFontFamily { family: family(bold, italic), priority: FontPriority::Highest };
            // Each goes ahead of all before it, so the first named goes last.
            for name in self.named.iter().rev() {
                let Some(font) = find(&mut listed, [QueryFamily::Named(name)], look, |_| true)
                else {
                    continue;
                };
                // A font of many weights is told which of them this face is.
                let coords =
                    VariationCoords::new(font.synthesis.variation_settings().iter().copied());
                ctx.add_font(FontInsert {
                    name: format!("{} of {name}", ahead.family),
                    data: data(font).tweak(FontTweak { coords, ..Default::default() }),
                    families: vec![ahead.clone()],
                });
            }
        }
    }

    /// Windows' font of symbols, for text that is neither a picture nor a mark and
    /// begins with a symbol of no script, where it has one. It is asked before the
    /// desktop's choice, which would draw some such symbols as pictures.
    #[cfg(windows)]
    fn symbol(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        use unicode_script::{Script, UnicodeScript};
        if let FontFamily::Name(family) = request.family
            && [EMOJI, ICONS].contains(&&**family)
        {
            return None;
        }
        let wanted = request.cluster.chars().next()?;
        if !matches!(wanted.script(), Script::Common | Script::Inherited | Script::Unknown) {
            return None;
        }
        let mut listed = self.listed.lock();
        let symbols = [QueryFamily::Named(SYMBOLS)];
        let has = |font: &QueryFont| {
            font.charmap().and_then(|map| map.map(wanted)).is_some_and(|glyph| glyph != 0)
        };
        let font = find(&mut listed, symbols, Attributes::default(), has)?;
        Some(FontInsert {
            name: format!("{SYMBOLS}:{}", font.index),
            data: data(font),
            families: Vec::new(),
        })
    }
}

/// The first font among `families` in their order, of a `look`, that `fits`.
fn find<'a>(
    (fonts, sources): &mut Listed,
    families: impl IntoIterator<Item = QueryFamily<'a>>,
    look: Attributes,
    fits: impl Fn(&QueryFont) -> bool,
) -> Option<QueryFont> {
    let mut query = fonts.query(sources);
    query.set_families(families);
    query.set_attributes(look);
    let mut found = None;
    query.matches_with(|font| {
        found = fits(font).then(|| font.clone());
        if found.is_some() { QueryStatus::Stop } else { QueryStatus::Continue }
    });
    found
}

/// A font of the machine's as egui takes one: the file is shared, not copied.
fn data(font: QueryFont) -> FontData {
    FontData::from_blob(font.blob.into_raw_parts().0, font.index)
}

impl FontProvider for Machine {
    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        #[cfg(windows)]
        if let Some(symbol) = self.symbol(request) {
            return Some(symbol);
        }
        let FontFamily::Name(family) = request.family else {
            return None;
        };
        let icons = match &**family {
            EMOJI => false,
            ICONS => true,
            _ => return None,
        };
        let wanted = request.cluster.chars().next()?;
        let mut listed = self.listed.lock();
        let mut nerds: Vec<String>;
        let families = if icons {
            // The user's own fonts come first, as they do in Ghostty. Then a
            // font of symbols alone, then one whose marks are as wide as they
            // were drawn, and last one that has squeezed them into a letter's
            // width. Of two alike the shorter name is the plainer font.
            let names = listed.0.family_names().filter(|name| name.contains("Nerd Font"));
            nerds = names.map(str::to_owned).collect();
            nerds.sort_by_key(|name| {
                (!name.starts_with("Symbols"), name.ends_with("Mono"), name.len())
            });
            self.named.iter().chain(&nerds).map(|name| QueryFamily::Named(name)).collect()
        } else {
            vec![QueryFamily::Generic(GenericFamily::Emoji)]
        };
        let has = |font: &QueryFont| {
            font.charmap().and_then(|map| map.map(wanted)).is_some_and(|glyph| glyph != 0)
        };
        let font = find(&mut listed, families, Attributes::default(), has)?;
        Some(FontInsert {
            name: format!("{family}:{:?}:{}:{}", font.family.0, font.family.1, font.index),
            data: data(font),
            families: Vec::new(),
        })
    }
}

/// Have a window find fonts on this machine: the families the user has
/// `named`, which go ahead of the terminal's own, and behind every font of the
/// window's whatever draws what none of them can. This client's own finder is
/// asked first, for its two families, and on Windows for symbols, and then the
/// desktop's choice is taken.
pub fn install(ctx: &egui::Context, named: Vec<String>) {
    let list = || {
        let all = CollectionOptions { shared: false, system_fonts: true };
        (Collection::new(all), SourceCache::default())
    };
    let machine = Arc::new(Machine { named, listed: Mutex::new(LazyLock::new(list)) });
    // A cell is as large as its font's letters, so a font that is named is
    // found before anything is drawn.
    machine.lead(ctx);
    // Listing a machine's fonts takes a moment, and so does finding the one
    // for emoji among them. Both are done ahead, off to one side, so that the
    // first emoji on screen does not hold the window up.
    let ahead = Arc::clone(&machine);
    std::thread::spawn(move || {
        let family = FontFamily::Name(EMOJI.into());
        ahead.font_for(&FallbackRequest { cluster: "😀", family: &family });
    });
    let desktop = egui_system_font_provider::SystemFontProvider::new();
    ctx.set_font_providers(vec![machine, Arc::new(desktop)]);
}

#[cfg(test)]
#[path = "../../tests/unit/ui_fonts.rs"]
mod tests;
