//! Colour picking and conversion, as ShareX's colour picker.
//!
//! ShareX shows one colour in six notations at once, because the reason you
//! picked it decides which one you need — CSS wants hex, a design tool wants
//! HSL, print wants CMYK. So this converts once and returns all of them rather
//! than making the UI ask again per format.
//!
//! Everything here is arithmetic on a pixel. Getting the pixel off the screen
//! is capture's job.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// One colour in every notation ShareX shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Swatch {
    pub rgb: Rgb,
    /// `#rrggbb`, upper case — what a hex field expects to be pasted into.
    pub hex: String,
    /// Hue 0–360, saturation and lightness 0–100.
    pub hsl: (f32, f32, f32),
    /// Hue 0–360, saturation and value 0–100.
    pub hsv: (f32, f32, f32),
    /// Cyan, magenta, yellow, key — each 0–100.
    pub cmyk: (f32, f32, f32, f32),
    /// Perceived brightness 0–1, from the same luma weights used elsewhere.
    pub luminance: f32,
    /// Black or white, whichever stays readable on this colour.
    ///
    /// The UI needs it to label a swatch, and picking it by eye per colour is
    /// exactly the sort of thing that goes wrong in the dark theme.
    pub contrasting: Rgb,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Parse `#rgb`, `#rrggbb`, or either without the hash.
    ///
    /// Tolerant on input because this is what a pasted value goes through, and
    /// people paste with the hash about as often as without.
    pub fn from_hex(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();

        match hex.len() {
            3 => {
                let nibble = |i: usize| {
                    u8::from_str_radix(hex.get(i..i + 1)?, 16)
                        .ok()
                        // 0xF becomes 0xFF, the CSS shorthand rule.
                        .map(|v| v * 17)
                };
                Some(Self::new(nibble(0)?, nibble(1)?, nibble(2)?))
            }
            6 => Some(Self::new(byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        }
    }

    pub fn to_hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = self.normalised();
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        let delta = max - min;

        if delta == 0.0 {
            // Grey has no hue. Reporting one would be inventing information.
            return (0.0, 0.0, lightness * 100.0);
        }

        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        (
            hue(r, g, b, max, delta),
            (saturation * 100.0).clamp(0.0, 100.0),
            lightness * 100.0,
        )
    }

    pub fn to_hsv(self) -> (f32, f32, f32) {
        let (r, g, b) = self.normalised();
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        if delta == 0.0 {
            return (0.0, 0.0, max * 100.0);
        }
        (hue(r, g, b, max, delta), (delta / max) * 100.0, max * 100.0)
    }

    pub fn to_cmyk(self) -> (f32, f32, f32, f32) {
        let (r, g, b) = self.normalised();
        let key = 1.0 - r.max(g).max(b);

        // Pure black divides by zero in the general formula.
        if (key - 1.0).abs() < f32::EPSILON {
            return (0.0, 0.0, 0.0, 100.0);
        }

        let channel = |value: f32| ((1.0 - value - key) / (1.0 - key)) * 100.0;
        (channel(r), channel(g), channel(b), key * 100.0)
    }

    pub fn luminance(self) -> f32 {
        let (r, g, b) = self.normalised();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// Black or white, whichever is readable against this colour.
    pub fn contrasting(self) -> Rgb {
        if self.luminance() > 0.5 {
            Rgb::new(0, 0, 0)
        } else {
            Rgb::new(255, 255, 255)
        }
    }

    pub fn swatch(self) -> Swatch {
        Swatch {
            rgb: self,
            hex: self.to_hex(),
            hsl: self.to_hsl(),
            hsv: self.to_hsv(),
            cmyk: self.to_cmyk(),
            luminance: self.luminance(),
            contrasting: self.contrasting(),
        }
    }

    fn normalised(self) -> (f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
        )
    }
}

fn hue(r: f32, g: f32, b: f32, max: f32, delta: f32) -> f32 {
    let hue = if max == r {
        ((g - b) / delta) % 6.0
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;

    // The red sector wraps negative; a hue of -30 is 330.
    if hue < 0.0 {
        hue + 360.0
    } else {
        hue
    }
}

/// Read the colour at a point in an image.
///
/// Out of bounds returns `None` rather than clamping to the edge: a click that
/// missed should say so, not quietly report a different pixel's colour.
pub fn pick(image: &RgbaImage, x: u32, y: u32) -> Option<Swatch> {
    if x >= image.width() || y >= image.height() {
        return None;
    }
    let pixel = image.get_pixel(x, y);
    Some(Rgb::new(pixel[0], pixel[1], pixel[2]).swatch())
}

/// Average the colours in a square around a point.
///
/// Useful on anti-aliased text or a gradient, where the single pixel under the
/// cursor is not the colour anyone means. The square is clipped to the image,
/// so a pick near an edge averages fewer pixels rather than failing.
pub fn pick_average(image: &RgbaImage, x: u32, y: u32, radius: u32) -> Option<Swatch> {
    if x >= image.width() || y >= image.height() {
        return None;
    }

    let left = x.saturating_sub(radius);
    let top = y.saturating_sub(radius);
    let right = (x + radius).min(image.width() - 1);
    let bottom = (y + radius).min(image.height() - 1);

    let mut total = (0u64, 0u64, 0u64);
    let mut count = 0u64;
    for py in top..=bottom {
        for px in left..=right {
            let pixel = image.get_pixel(px, py);
            total.0 += pixel[0] as u64;
            total.1 += pixel[1] as u64;
            total.2 += pixel[2] as u64;
            count += 1;
        }
    }

    Some(
        Rgb::new(
            (total.0 / count) as u8,
            (total.1 / count) as u8,
            (total.2 / count) as u8,
        )
        .swatch(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.6
    }

    #[test]
    fn hex_round_trips() {
        let colour = Rgb::new(18, 52, 86);
        assert_eq!(colour.to_hex(), "#123456");
        assert_eq!(Rgb::from_hex("#123456"), Some(colour));
    }

    #[test]
    fn a_pasted_hex_is_accepted_with_or_without_the_hash() {
        // This is what a pasted value goes through, and people paste both ways.
        assert_eq!(Rgb::from_hex("ff0000"), Some(Rgb::new(255, 0, 0)));
        assert_eq!(Rgb::from_hex("  #FF0000 "), Some(Rgb::new(255, 0, 0)));
    }

    #[test]
    fn the_css_three_digit_shorthand_expands_the_way_css_says() {
        // #f00 is #ff0000, not #f00000.
        assert_eq!(Rgb::from_hex("#f00"), Some(Rgb::new(255, 0, 0)));
        assert_eq!(Rgb::from_hex("#abc"), Some(Rgb::new(170, 187, 204)));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed() {
        assert_eq!(Rgb::from_hex(""), None);
        assert_eq!(Rgb::from_hex("#12345"), None);
        assert_eq!(Rgb::from_hex("#gggggg"), None);
        assert_eq!(Rgb::from_hex("#12345678"), None);
    }

    #[test]
    fn primaries_convert_to_the_expected_hsl() {
        let (h, s, l) = Rgb::new(255, 0, 0).to_hsl();
        assert!(
            close(h, 0.0) && close(s, 100.0) && close(l, 50.0),
            "{h} {s} {l}"
        );

        let (h, ..) = Rgb::new(0, 255, 0).to_hsl();
        assert!(close(h, 120.0), "green hue was {h}");

        let (h, ..) = Rgb::new(0, 0, 255).to_hsl();
        assert!(close(h, 240.0), "blue hue was {h}");
    }

    #[test]
    fn a_hue_in_the_red_sector_wraps_instead_of_going_negative() {
        // Magenta sits just below 360; the naive formula gives -60.
        let (h, ..) = Rgb::new(255, 0, 128).to_hsl();
        assert!(h > 300.0 && h < 360.0, "hue was {h}");
    }

    #[test]
    fn grey_reports_no_hue_rather_than_an_invented_one() {
        let (h, s, l) = Rgb::new(128, 128, 128).to_hsl();
        assert_eq!(h, 0.0);
        assert_eq!(s, 0.0);
        assert!(close(l, 50.2), "lightness was {l}");
    }

    #[test]
    fn hsv_differs_from_hsl_where_it_should() {
        // Pure red is 100% lightness in HSV but 50% in HSL; conflating the two
        // is the classic colour-picker bug.
        let (_, _, v) = Rgb::new(255, 0, 0).to_hsv();
        let (_, _, l) = Rgb::new(255, 0, 0).to_hsl();

        assert!(close(v, 100.0), "value was {v}");
        assert!(close(l, 50.0), "lightness was {l}");
    }

    #[test]
    fn black_does_not_divide_by_zero_in_cmyk() {
        let (c, m, y, k) = Rgb::new(0, 0, 0).to_cmyk();
        assert_eq!((c, m, y, k), (0.0, 0.0, 0.0, 100.0));
    }

    #[test]
    fn cmyk_matches_the_usual_values() {
        let (c, m, y, k) = Rgb::new(255, 0, 0).to_cmyk();
        assert!(close(c, 0.0) && close(m, 100.0) && close(y, 100.0) && close(k, 0.0));

        let (c, m, y, k) = Rgb::new(255, 255, 255).to_cmyk();
        assert!(close(c, 0.0) && close(m, 0.0) && close(y, 0.0) && close(k, 0.0));
    }

    #[test]
    fn the_contrasting_colour_flips_at_the_brightness_threshold() {
        // The UI labels swatches with this; getting it wrong makes text vanish.
        assert_eq!(Rgb::new(255, 255, 255).contrasting(), Rgb::new(0, 0, 0));
        assert_eq!(Rgb::new(0, 0, 0).contrasting(), Rgb::new(255, 255, 255));
        // Yellow is bright despite being saturated.
        assert_eq!(Rgb::new(255, 255, 0).contrasting(), Rgb::new(0, 0, 0));
        // Blue is dark despite being fully saturated.
        assert_eq!(Rgb::new(0, 0, 255).contrasting(), Rgb::new(255, 255, 255));
    }

    #[test]
    fn picking_reads_the_pixel_under_the_point() {
        let mut image = RgbaImage::new(4, 4);
        image.put_pixel(2, 3, Rgba([10, 20, 30, 255]));

        let swatch = pick(&image, 2, 3).unwrap();
        assert_eq!(swatch.rgb, Rgb::new(10, 20, 30));
        assert_eq!(swatch.hex, "#0A141E");
    }

    #[test]
    fn a_pick_outside_the_image_fails_instead_of_clamping() {
        // Clamping would report a different pixel's colour as if it were the
        // one clicked.
        let image = RgbaImage::new(4, 4);
        assert!(pick(&image, 4, 0).is_none());
        assert!(pick(&image, 0, 4).is_none());
    }

    #[test]
    fn averaging_smooths_over_a_single_odd_pixel() {
        // The point of the average is anti-aliased text, where the exact pixel
        // under the cursor is not the colour anyone means.
        let mut image = RgbaImage::from_pixel(9, 9, Rgba([100, 100, 100, 255]));
        image.put_pixel(4, 4, Rgba([0, 0, 0, 255]));

        let single = pick(&image, 4, 4).unwrap();
        let averaged = pick_average(&image, 4, 4, 2).unwrap();

        assert_eq!(single.rgb, Rgb::new(0, 0, 0));
        assert!(averaged.rgb.r > 80, "average was {:?}", averaged.rgb);
    }

    #[test]
    fn averaging_near_an_edge_clips_rather_than_failing() {
        let image = RgbaImage::from_pixel(4, 4, Rgba([60, 60, 60, 255]));
        let swatch = pick_average(&image, 0, 0, 10).unwrap();

        assert_eq!(swatch.rgb, Rgb::new(60, 60, 60));
    }

    #[test]
    fn a_swatch_carries_every_notation_at_once() {
        // The reason you picked a colour decides which notation you need, so
        // the UI should not have to ask again per format.
        let swatch = Rgb::new(255, 128, 0).swatch();

        assert_eq!(swatch.hex, "#FF8000");
        assert!(swatch.hsl.0 > 0.0);
        assert!(swatch.hsv.2 > 99.0);
        assert!(swatch.cmyk.3 < 1.0);
        assert!(swatch.luminance > 0.0 && swatch.luminance < 1.0);
    }

    #[test]
    fn a_swatch_survives_a_json_round_trip() {
        let swatch = Rgb::new(12, 34, 56).swatch();
        let json = serde_json::to_string(&swatch).unwrap();

        assert_eq!(serde_json::from_str::<Swatch>(&json).unwrap(), swatch);
    }
}
