//! Text rasterisation.
//!
//! Kestrel loads a font from the operating system rather than bundling one.
//! Bundling would add a megabyte to every build, force a licence decision on
//! contributors, and — worse — make annotations look foreign on each platform.
//! Using the system's own sans-serif means text in a screenshot matches the
//! text in the screenshot.
//!
//! Layout here is deliberately simple: glyph advances plus kerning, broken on
//! explicit newlines. That is correct for Latin, Turkish, Cyrillic and Greek,
//! which covers annotation text. Scripts that need shaping (Arabic, Devanagari)
//! will render as unjoined glyphs; wiring in a shaper is tracked separately and
//! is not worth blocking the text tool on.

use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};

/// The system fonts Kestrel draws with, resolved once per process.
///
/// Scanning the system font directories costs a hundred milliseconds or so, so
/// it happens lazily and only once — never on the capture path.
pub struct Fonts {
    regular: FontVec,
    bold: Option<FontVec>,
    italic: Option<FontVec>,
}

static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();

pub fn system() -> Option<&'static Fonts> {
    FONTS.get_or_init(Fonts::load).as_ref()
}

impl Fonts {
    fn load() -> Option<Fonts> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let regular = query(&db, fontdb::Weight::NORMAL, fontdb::Style::Normal)?;
        Some(Fonts {
            regular,
            bold: query(&db, fontdb::Weight::BOLD, fontdb::Style::Normal),
            italic: query(&db, fontdb::Weight::NORMAL, fontdb::Style::Italic),
        })
    }

    fn face(&self, bold: bool, italic: bool) -> &FontVec {
        // Synthetic bold-italic is not worth the complexity; prefer whichever
        // single variant the user asked for and fall back to regular.
        match (bold, italic) {
            (true, _) => self.bold.as_ref().unwrap_or(&self.regular),
            (false, true) => self.italic.as_ref().unwrap_or(&self.regular),
            _ => &self.regular,
        }
    }

    /// Width and height of `text` at `size`, in pixels.
    pub fn measure(&self, text: &str, size: f32, bold: bool, italic: bool) -> (f32, f32) {
        let font = self.face(bold, italic);
        let scaled = font.as_scaled(PxScale::from(size));
        let line_height = scaled.height() + scaled.line_gap();

        let widest = text
            .lines()
            .map(|line| line_width(font, size, line))
            .fold(0.0f32, f32::max);

        let lines = text.lines().count().max(1) as f32;
        (widest, line_height * lines)
    }

    /// Rasterise `text`, calling `plot` for every covered pixel.
    ///
    /// `origin` is the top-left of the text block, not a baseline — callers
    /// place text inside a rectangle and should not have to know about font
    /// metrics to do it.
    pub fn rasterize(
        &self,
        text: &str,
        size: f32,
        bold: bool,
        italic: bool,
        origin: (f32, f32),
        mut plot: impl FnMut(i32, i32, f32),
    ) {
        let font = self.face(bold, italic);
        let scaled = font.as_scaled(PxScale::from(size));
        let line_height = scaled.height() + scaled.line_gap();

        for (index, line) in text.lines().enumerate() {
            let baseline_y = origin.1 + scaled.ascent() + line_height * index as f32;
            let mut pen_x = origin.0;
            let mut previous = None;

            for ch in line.chars() {
                let glyph_id = font.glyph_id(ch);
                if let Some(previous) = previous {
                    pen_x += scaled.kern(previous, glyph_id);
                }

                let glyph = glyph_id.with_scale_and_position(
                    PxScale::from(size),
                    ab_glyph::point(pen_x, baseline_y),
                );

                if let Some(outlined) = font.outline_glyph(glyph) {
                    let bounds = outlined.px_bounds();
                    outlined.draw(|gx, gy, coverage| {
                        plot(
                            bounds.min.x as i32 + gx as i32,
                            bounds.min.y as i32 + gy as i32,
                            coverage,
                        );
                    });
                }

                pen_x += scaled.h_advance(glyph_id);
                previous = Some(glyph_id);
            }
        }
    }
}

fn line_width(font: &FontVec, size: f32, line: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut previous = None;

    for ch in line.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(previous) = previous {
            width += scaled.kern(previous, glyph_id);
        }
        width += scaled.h_advance(glyph_id);
        previous = Some(glyph_id);
    }
    width
}

/// Concrete sans-serif families to try when the generic one resolves to
/// nothing.
///
/// `Family::SansSerif` is not a font, it is a name fontdb looks up — and its
/// default is Arial, which most Linux installs do not have. Asking only for
/// the generic family meant no text at all on a machine with a perfectly good
/// DejaVu, which is how a CI runner and a plain Debian desktop both behave.
const FALLBACKS: [&str; 9] = [
    "DejaVu Sans",
    "Liberation Sans",
    "Noto Sans",
    "Cantarell",
    "Ubuntu",
    "Helvetica Neue",
    "Helvetica",
    "Segoe UI",
    "Arial",
];

fn query(db: &fontdb::Database, weight: fontdb::Weight, style: fontdb::Style) -> Option<FontVec> {
    let ask = |families: &[fontdb::Family]| {
        db.query(&fontdb::Query {
            families,
            weight,
            stretch: fontdb::Stretch::Normal,
            style,
        })
    };

    let id = ask(&[fontdb::Family::SansSerif])
        .or_else(|| {
            FALLBACKS
                .iter()
                .find_map(|name| ask(&[fontdb::Family::Name(name)]))
        })
        // Last resort: whatever the system has. A capture annotated in a serif
        // face is a cosmetic disappointment; one annotated in nothing at all is
        // a lost annotation.
        .or_else(|| db.faces().next().map(|face| face.id))?;

    // A font collection (.ttc) holds several faces; the index matters.
    db.with_face_data(id, |data, index| {
        FontVec::try_from_vec_and_index(data.to_vec(), index).ok()
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every desktop OS ships a sans-serif font. If this fails, text
    /// annotations silently disappear, so it is worth asserting loudly.
    #[test]
    fn a_system_font_is_available() {
        assert!(
            system().is_some(),
            "no system sans-serif font could be loaded"
        );
    }

    #[test]
    fn a_font_is_found_without_the_generic_family_resolving() {
        // fontdb's generic sans-serif defaults to Arial, which most Linux
        // installs do not have — so asking only for the generic family meant no
        // text at all there. This is that case: a database holding one font
        // whose name is in neither the generic mapping nor anything else.
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db.set_sans_serif_family("A Font Nobody Has");

        assert!(
            query(&db, fontdb::Weight::NORMAL, fontdb::Style::Normal).is_some(),
            "a face should still be found when the generic family resolves to nothing"
        );
    }

    #[test]
    fn measuring_scales_with_size() {
        let Some(fonts) = system() else { return };

        let (small, small_h) = fonts.measure("merhaba", 12.0, false, false);
        let (large, large_h) = fonts.measure("merhaba", 24.0, false, false);

        assert!(large > small, "wider at a larger size");
        assert!(large_h > small_h, "taller at a larger size");
    }

    #[test]
    fn measuring_grows_with_content() {
        let Some(fonts) = system() else { return };

        let (short, _) = fonts.measure("a", 16.0, false, false);
        let (long, _) = fonts.measure("aaaaaaaaaa", 16.0, false, false);

        assert!(long > short * 5.0);
    }

    #[test]
    fn an_empty_string_has_no_width_but_still_has_a_line() {
        let Some(fonts) = system() else { return };

        let (width, height) = fonts.measure("", 16.0, false, false);
        assert_eq!(width, 0.0);
        assert!(height > 0.0, "an empty text box still occupies a line");
    }

    #[test]
    fn multiple_lines_stack_vertically() {
        let Some(fonts) = system() else { return };

        let (_, one) = fonts.measure("bir", 16.0, false, false);
        let (_, two) = fonts.measure("bir\niki", 16.0, false, false);

        assert!(two > one * 1.5, "two lines should be about twice as tall");
    }

    #[test]
    fn rasterising_covers_pixels_below_and_right_of_the_origin() {
        let Some(fonts) = system() else { return };

        let mut covered = Vec::new();
        fonts.rasterize("Ag", 32.0, false, false, (10.0, 10.0), |x, y, coverage| {
            if coverage > 0.1 {
                covered.push((x, y));
            }
        });

        assert!(!covered.is_empty(), "glyphs should produce coverage");
        // Placed by its top-left corner, so nothing may appear above the origin.
        assert!(
            covered.iter().all(|(_, y)| *y >= 9),
            "text must be laid out from its top edge, not its baseline"
        );
        assert!(covered.iter().all(|(x, _)| *x >= 9));
    }

    #[test]
    fn turkish_characters_rasterise() {
        let Some(fonts) = system() else { return };

        for text in ["ğ", "İ", "ş", "ı", "ö", "ç"] {
            let mut covered = 0;
            fonts.rasterize(text, 32.0, false, false, (0.0, 0.0), |_, _, c| {
                if c > 0.1 {
                    covered += 1;
                }
            });
            assert!(covered > 0, "{text} produced no glyph coverage");
        }
    }

    #[test]
    fn whitespace_advances_without_drawing() {
        let Some(fonts) = system() else { return };

        let mut covered = 0;
        fonts.rasterize("   ", 32.0, false, false, (0.0, 0.0), |_, _, c| {
            if c > 0.1 {
                covered += 1;
            }
        });
        assert_eq!(covered, 0);

        let (width, _) = fonts.measure("   ", 32.0, false, false);
        assert!(width > 0.0, "spaces still take horizontal room");
    }
}
