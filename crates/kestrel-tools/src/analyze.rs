//! Image analysis, and combining or splitting images.
//!
//! ShareX's "analyze image", "image combiner" and "image splitter", which are
//! pure pixel work and so live together.

use image::{Rgba, RgbaImage};
use serde::Serialize;

/// What "analyze image" reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    /// Distinct colours, capped — see [`MAX_TRACKED_COLOURS`].
    pub unique_colours: usize,
    pub unique_colours_capped: bool,
    pub has_transparency: bool,
    /// Most common colours, as `#rrggbb`, commonest first.
    pub dominant: Vec<String>,
    /// Mean luminance, 0.0 to 1.0. Tells the UI whether to caption in black.
    pub average_luminance: f32,
}

/// Counting every distinct colour in a photographic screenshot means millions
/// of map entries for a number nobody reads past "lots". This is high enough to
/// be exact for screenshots of interfaces, which is the case that matters.
const MAX_TRACKED_COLOURS: usize = 100_000;

/// How many dominant colours to report.
const DOMINANT_COUNT: usize = 5;

pub fn analyze(image: &RgbaImage) -> Analysis {
    let mut counts: std::collections::HashMap<[u8; 3], u32> = std::collections::HashMap::new();
    let mut capped = false;
    let mut has_transparency = false;
    let mut luminance_total = 0.0f64;

    for pixel in image.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 255 {
            has_transparency = true;
        }

        // Rec. 709 luma: matches how bright a colour actually looks.
        luminance_total += (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0;

        if counts.len() < MAX_TRACKED_COLOURS {
            *counts.entry([r, g, b]).or_insert(0) += 1;
        } else if let Some(count) = counts.get_mut(&[r, g, b]) {
            *count += 1;
        } else {
            capped = true;
        }
    }

    let mut ranked: Vec<([u8; 3], u32)> = counts.iter().map(|(k, v)| (*k, *v)).collect();
    // Sort by count, then by colour, so the same image always reports the same
    // order — a list that shuffles between runs looks like a bug.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let pixels = (image.width() as f64 * image.height() as f64).max(1.0);

    Analysis {
        width: image.width(),
        height: image.height(),
        unique_colours: counts.len(),
        unique_colours_capped: capped,
        has_transparency,
        dominant: ranked
            .iter()
            .take(DOMINANT_COUNT)
            .map(|([r, g, b], _)| format!("#{r:02x}{g:02x}{b:02x}"))
            .collect(),
        average_luminance: (luminance_total / pixels) as f32,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Vertical,
    Horizontal,
}

/// Stack images into one, as ShareX's image combiner.
///
/// Images of different sizes are aligned to the start and the gaps are left
/// transparent, rather than being stretched: a stretched screenshot is
/// unreadable, and unreadable is worse than uneven.
pub fn combine(images: &[RgbaImage], direction: Direction, spacing: u32) -> Option<RgbaImage> {
    if images.is_empty() {
        return None;
    }

    let gaps = spacing * (images.len() as u32 - 1);
    let (width, height) = match direction {
        Direction::Vertical => (
            images.iter().map(|i| i.width()).max()?,
            images.iter().map(|i| i.height()).sum::<u32>() + gaps,
        ),
        Direction::Horizontal => (
            images.iter().map(|i| i.width()).sum::<u32>() + gaps,
            images.iter().map(|i| i.height()).max()?,
        ),
    };

    let mut canvas = RgbaImage::new(width.max(1), height.max(1));
    let mut offset = 0u32;

    for image in images {
        let (x, y) = match direction {
            Direction::Vertical => (0, offset),
            Direction::Horizontal => (offset, 0),
        };
        image::imageops::replace(&mut canvas, image, x as i64, y as i64);
        offset += match direction {
            Direction::Vertical => image.height() + spacing,
            Direction::Horizontal => image.width() + spacing,
        };
    }

    Some(canvas)
}

/// Cut an image into a grid, as ShareX's image splitter.
///
/// The last row and column keep the remainder rather than being dropped, so
/// splitting and recombining loses nothing.
pub fn split(image: &RgbaImage, columns: u32, rows: u32) -> Vec<RgbaImage> {
    let columns = columns.max(1);
    let rows = rows.max(1);
    let tile_width = image.width() / columns;
    let tile_height = image.height() / rows;

    if tile_width == 0 || tile_height == 0 {
        // More tiles than pixels: one tile is the honest answer.
        return vec![image.clone()];
    }

    let mut tiles = Vec::with_capacity((columns * rows) as usize);
    for row in 0..rows {
        for column in 0..columns {
            let x = column * tile_width;
            let y = row * tile_height;
            let width = if column == columns - 1 {
                image.width() - x
            } else {
                tile_width
            };
            let height = if row == rows - 1 {
                image.height() - y
            } else {
                tile_height
            };
            tiles.push(image::imageops::crop_imm(image, x, y, width, height).to_image());
        }
    }
    tiles
}

/// A thumbnail that fits inside `max_width` x `max_height`, keeping the ratio.
pub fn thumbnail(image: &RgbaImage, max_width: u32, max_height: u32) -> RgbaImage {
    let scale = (max_width as f32 / image.width().max(1) as f32)
        .min(max_height as f32 / image.height().max(1) as f32)
        .min(1.0);

    if scale >= 1.0 {
        return image.clone();
    }
    image::imageops::thumbnail(
        image,
        ((image.width() as f32 * scale) as u32).max(1),
        ((image.height() as f32 * scale) as u32).max(1),
    )
}

/// Average colour of a region, for the colour picker's magnifier readout.
pub fn average_colour(image: &RgbaImage, x: u32, y: u32, size: u32) -> Rgba<u8> {
    let half = size / 2;
    let x0 = x.saturating_sub(half);
    let y0 = y.saturating_sub(half);
    let x1 = (x + half + 1).min(image.width());
    let y1 = (y + half + 1).min(image.height());

    let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
    for py in y0..y1 {
        for px in x0..x1 {
            let pixel = image.get_pixel(px, py);
            r += pixel[0] as u32;
            g += pixel[1] as u32;
            b += pixel[2] as u32;
            n += 1;
        }
    }
    if n == 0 {
        return Rgba([0, 0, 0, 0]);
    }
    Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(colour))
    }

    #[test]
    fn a_solid_image_has_one_colour() {
        let analysis = analyze(&solid(10, 10, [255, 0, 0, 255]));

        assert_eq!(analysis.unique_colours, 1);
        assert_eq!(analysis.dominant, ["#ff0000"]);
        assert!(!analysis.has_transparency);
        assert_eq!((analysis.width, analysis.height), (10, 10));
    }

    #[test]
    fn dominant_colours_are_ordered_by_how_much_of_the_image_they_cover() {
        // Mostly red, a little blue.
        let mut image = solid(10, 10, [255, 0, 0, 255]);
        for x in 0..3 {
            image.put_pixel(x, 0, Rgba([0, 0, 255, 255]));
        }

        let analysis = analyze(&image);
        assert_eq!(analysis.dominant[0], "#ff0000");
        assert_eq!(analysis.dominant[1], "#0000ff");
    }

    #[test]
    fn the_dominant_order_is_stable_for_ties() {
        // Two colours covering equal area must not swap between runs; a list
        // that reshuffles reads as a bug.
        let mut image = solid(4, 2, [10, 10, 10, 255]);
        for x in 0..4 {
            image.put_pixel(x, 1, Rgba([200, 200, 200, 255]));
        }

        let first = analyze(&image).dominant;
        for _ in 0..5 {
            assert_eq!(analyze(&image).dominant, first);
        }
    }

    #[test]
    fn transparency_is_detected() {
        assert!(analyze(&solid(4, 4, [0, 0, 0, 128])).has_transparency);
        assert!(!analyze(&solid(4, 4, [0, 0, 0, 255])).has_transparency);
    }

    #[test]
    fn luminance_separates_light_from_dark() {
        let dark = analyze(&solid(4, 4, [0, 0, 0, 255])).average_luminance;
        let light = analyze(&solid(4, 4, [255, 255, 255, 255])).average_luminance;

        assert!(dark < 0.05);
        assert!(light > 0.95);
    }

    #[test]
    fn combining_vertically_stacks_and_keeps_every_pixel() {
        let red = solid(10, 5, [255, 0, 0, 255]);
        let blue = solid(10, 7, [0, 0, 255, 255]);

        let combined = combine(&[red, blue], Direction::Vertical, 0).unwrap();

        assert_eq!(combined.dimensions(), (10, 12));
        assert_eq!(combined.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(combined.get_pixel(0, 6), &Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn combining_uneven_images_pads_rather_than_stretching() {
        // A stretched screenshot is unreadable; an uneven one is merely uneven.
        let wide = solid(20, 4, [255, 0, 0, 255]);
        let narrow = solid(8, 4, [0, 255, 0, 255]);

        let combined = combine(&[wide, narrow], Direction::Vertical, 0).unwrap();

        assert_eq!(combined.width(), 20);
        assert_eq!(combined.get_pixel(0, 4), &Rgba([0, 255, 0, 255]));
        assert_eq!(combined.get_pixel(19, 4)[3], 0, "the gap stays transparent");
    }

    #[test]
    fn spacing_is_added_between_images_but_not_around_them() {
        let a = solid(4, 4, [1, 1, 1, 255]);
        let b = solid(4, 4, [2, 2, 2, 255]);

        let combined = combine(&[a, b], Direction::Horizontal, 6).unwrap();
        assert_eq!(combined.width(), 4 + 6 + 4);
    }

    #[test]
    fn combining_nothing_yields_nothing() {
        assert!(combine(&[], Direction::Vertical, 0).is_none());
    }

    #[test]
    fn splitting_covers_the_whole_image_including_the_remainder() {
        // 10 does not divide by 3; the last tile has to keep what is left, or
        // splitting and recombining silently loses a strip.
        let image = solid(10, 10, [9, 9, 9, 255]);
        let tiles = split(&image, 3, 1);

        assert_eq!(tiles.len(), 3);
        let total: u32 = tiles.iter().map(|t| t.width()).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn splitting_into_more_tiles_than_pixels_yields_one() {
        let image = solid(2, 2, [0, 0, 0, 255]);
        assert_eq!(split(&image, 50, 50).len(), 1);
    }

    #[test]
    fn a_thumbnail_fits_inside_the_bounds_and_keeps_its_ratio() {
        let image = solid(800, 400, [0, 0, 0, 255]);
        let thumb = thumbnail(&image, 200, 200);

        assert!(thumb.width() <= 200 && thumb.height() <= 200);
        let ratio = thumb.width() as f32 / thumb.height() as f32;
        assert!((ratio - 2.0).abs() < 0.1);
    }

    #[test]
    fn a_small_image_is_not_enlarged() {
        // Upscaling a thumbnail just makes a blurry bigger one.
        let image = solid(40, 30, [0, 0, 0, 255]);
        assert_eq!(thumbnail(&image, 200, 200).dimensions(), (40, 30));
    }

    #[test]
    fn the_average_colour_of_a_region_is_its_mean() {
        let mut image = solid(10, 10, [0, 0, 0, 255]);
        for y in 0..10 {
            for x in 0..5 {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        // A window centred on the boundary sees half of each.
        let average = average_colour(&image, 4, 5, 3);
        assert!(average[0] > 60 && average[0] < 200, "got {average:?}");
    }

    #[test]
    fn sampling_at_the_edge_stays_in_bounds() {
        let image = solid(5, 5, [10, 20, 30, 255]);
        assert_eq!(average_colour(&image, 0, 0, 9), Rgba([10, 20, 30, 255]));
        assert_eq!(average_colour(&image, 4, 4, 9), Rgba([10, 20, 30, 255]));
    }
}
