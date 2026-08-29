//! Comparing two images, as ShareX's image comparer.
//!
//! The question people actually ask is "did this change, and where" — so the
//! answer is a number *and* a picture. A percentage alone tells you something
//! moved without telling you what, and a diff image alone makes you hunt for a
//! change that might be one pixel.
//!
//! Differently-sized images are compared over their overlap rather than being
//! refused. Two screenshots of the same window at different scales are exactly
//! the case someone wants to compare, and refusing them would be pedantry.

use image::{Rgba, RgbaImage};
use serde::Serialize;

/// How different a channel may be and still count as unchanged.
///
/// Zero would flag JPEG ringing and any re-encode as a difference, which makes
/// the tool useless for the common case of "is this the same screenshot".
pub const DEFAULT_TOLERANCE: u8 = 4;

/// The colour a changed pixel is painted in the diff image.
const HIGHLIGHT: Rgba<u8> = Rgba([255, 45, 85, 255]);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Size of the region actually compared — the overlap of the two images.
    pub compared_width: u32,
    pub compared_height: u32,
    /// True when the two images are not the same size, so the UI can say that
    /// only the overlap was compared instead of implying a full match.
    pub sizes_differ: bool,
    pub changed_pixels: u64,
    pub total_pixels: u64,
    /// 0–100, over the compared region.
    pub difference_percent: f32,
    /// The largest single-channel difference found, so "0.1% changed" can be
    /// told apart from "0.1% changed, but completely".
    pub max_channel_delta: u8,
    /// The rectangle containing every change, or `None` when nothing changed.
    pub bounds: Option<Bounds>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Comparison {
    pub fn identical(&self) -> bool {
        self.changed_pixels == 0
    }
}

/// Compare two images and describe what differs.
pub fn compare(a: &RgbaImage, b: &RgbaImage, tolerance: u8) -> Comparison {
    let width = a.width().min(b.width());
    let height = a.height().min(b.height());
    let sizes_differ = a.dimensions() != b.dimensions();

    let mut changed = 0u64;
    let mut max_delta = 0u8;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for y in 0..height {
        for x in 0..width {
            let delta = channel_delta(a.get_pixel(x, y), b.get_pixel(x, y));
            max_delta = max_delta.max(delta);

            if delta > tolerance {
                changed += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    let total = u64::from(width) * u64::from(height);
    Comparison {
        compared_width: width,
        compared_height: height,
        sizes_differ,
        changed_pixels: changed,
        total_pixels: total,
        difference_percent: if total == 0 {
            0.0
        } else {
            (changed as f32 / total as f32) * 100.0
        },
        max_channel_delta: max_delta,
        bounds: (changed > 0).then(|| Bounds {
            x: min_x,
            y: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
        }),
    }
}

/// A picture of the difference: the first image, dimmed, with changes marked.
///
/// Keeping the original underneath is what makes the result readable — a
/// standalone mask of red dots tells you where without telling you what.
pub fn diff_image(a: &RgbaImage, b: &RgbaImage, tolerance: u8) -> RgbaImage {
    let width = a.width().min(b.width());
    let height = a.height().min(b.height());
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let original = a.get_pixel(x, y);
            let pixel = if channel_delta(original, b.get_pixel(x, y)) > tolerance {
                HIGHLIGHT
            } else {
                // Dimmed, not greyed: keeping the hue makes the underlying
                // content easier to recognise than a flat grey would.
                Rgba([
                    dim(original[0]),
                    dim(original[1]),
                    dim(original[2]),
                    original[3],
                ])
            };
            out.put_pixel(x, y, pixel);
        }
    }
    out
}

fn dim(value: u8) -> u8 {
    // Halfway to white, so highlights stay visible against light content too.
    (value as u16 / 2 + 110).min(255) as u8
}

/// The largest difference across the four channels.
///
/// Alpha counts: two images that look identical but differ in transparency are
/// genuinely different files, and hiding that would be the wrong answer for a
/// tool people use to check an export.
fn channel_delta(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    (0..4)
        .map(|i| a[i].abs_diff(b[i]))
        .max()
        .expect("four channels")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(colour))
    }

    #[test]
    fn identical_images_report_no_difference() {
        let image = solid(8, 8, [10, 20, 30, 255]);
        let result = compare(&image, &image, DEFAULT_TOLERANCE);

        assert!(result.identical());
        assert_eq!(result.difference_percent, 0.0);
        assert_eq!(result.bounds, None);
    }

    #[test]
    fn a_single_changed_pixel_is_found_and_located() {
        let a = solid(10, 10, [0, 0, 0, 255]);
        let mut b = a.clone();
        b.put_pixel(3, 7, Rgba([255, 255, 255, 255]));

        let result = compare(&a, &b, DEFAULT_TOLERANCE);

        assert_eq!(result.changed_pixels, 1);
        assert_eq!(
            result.bounds,
            Some(Bounds {
                x: 3,
                y: 7,
                width: 1,
                height: 1
            })
        );
    }

    #[test]
    fn the_bounds_cover_every_change_not_just_the_first() {
        let a = solid(10, 10, [0, 0, 0, 255]);
        let mut b = a.clone();
        b.put_pixel(2, 2, Rgba([255, 0, 0, 255]));
        b.put_pixel(6, 8, Rgba([255, 0, 0, 255]));

        let bounds = compare(&a, &b, DEFAULT_TOLERANCE).bounds.unwrap();

        assert_eq!(bounds.x, 2);
        assert_eq!(bounds.y, 2);
        assert_eq!(bounds.width, 5);
        assert_eq!(bounds.height, 7);
    }

    #[test]
    fn tolerance_absorbs_a_re_encode_without_hiding_a_real_change() {
        // Zero tolerance would flag JPEG ringing, which makes the tool useless
        // for "is this the same screenshot".
        let a = solid(4, 4, [100, 100, 100, 255]);
        let noise = solid(4, 4, [102, 98, 101, 255]);
        let real = solid(4, 4, [140, 100, 100, 255]);

        assert!(compare(&a, &noise, DEFAULT_TOLERANCE).identical());
        assert!(!compare(&a, &real, DEFAULT_TOLERANCE).identical());
    }

    #[test]
    fn a_zero_tolerance_catches_everything() {
        let a = solid(4, 4, [100, 100, 100, 255]);
        let b = solid(4, 4, [101, 100, 100, 255]);

        assert!(compare(&a, &b, DEFAULT_TOLERANCE).identical());
        assert!(!compare(&a, &b, 0).identical());
    }

    #[test]
    fn alpha_differences_count() {
        // Two images that look the same but differ in transparency are
        // different files, and someone checking an export needs to know.
        let a = solid(4, 4, [255, 0, 0, 255]);
        let b = solid(4, 4, [255, 0, 0, 0]);

        assert!(!compare(&a, &b, DEFAULT_TOLERANCE).identical());
    }

    #[test]
    fn different_sizes_are_compared_over_the_overlap_and_flagged() {
        // Two screenshots of the same window at different scales is exactly
        // the case someone wants to compare.
        let a = solid(10, 10, [5, 5, 5, 255]);
        let b = solid(6, 8, [5, 5, 5, 255]);

        let result = compare(&a, &b, DEFAULT_TOLERANCE);

        assert!(result.sizes_differ);
        assert_eq!((result.compared_width, result.compared_height), (6, 8));
        assert!(result.identical(), "the overlap is the same");
    }

    #[test]
    fn the_max_delta_separates_a_slight_change_from_a_total_one() {
        // "0.1% changed" means something different depending on how much.
        let a = solid(10, 10, [100, 100, 100, 255]);
        let mut slight = a.clone();
        slight.put_pixel(0, 0, Rgba([120, 100, 100, 255]));
        let mut total = a.clone();
        total.put_pixel(0, 0, Rgba([255, 255, 255, 255]));

        assert_eq!(
            compare(&a, &slight, DEFAULT_TOLERANCE).max_channel_delta,
            20
        );
        assert_eq!(
            compare(&a, &total, DEFAULT_TOLERANCE).max_channel_delta,
            155
        );
    }

    #[test]
    fn the_percentage_is_over_the_compared_region() {
        let a = solid(10, 10, [0, 0, 0, 255]);
        let mut b = a.clone();
        for x in 0..10 {
            b.put_pixel(x, 0, Rgba([255, 255, 255, 255]));
        }

        let result = compare(&a, &b, DEFAULT_TOLERANCE);
        assert_eq!(result.changed_pixels, 10);
        assert!((result.difference_percent - 10.0).abs() < 0.01);
    }

    #[test]
    fn the_diff_image_marks_changes_and_keeps_the_original_underneath() {
        // A standalone mask of red dots says where without saying what.
        let a = solid(6, 6, [20, 40, 60, 255]);
        let mut b = a.clone();
        b.put_pixel(1, 1, Rgba([255, 255, 255, 255]));

        let diff = diff_image(&a, &b, DEFAULT_TOLERANCE);

        assert_eq!(diff.get_pixel(1, 1), &HIGHLIGHT);
        let unchanged = diff.get_pixel(4, 4);
        assert_ne!(unchanged, &HIGHLIGHT);
        assert!(
            unchanged[2] > 60,
            "unchanged content should be dimmed, not left alone: {unchanged:?}"
        );
    }

    #[test]
    fn the_diff_image_is_the_size_of_the_overlap() {
        let a = solid(10, 4, [0, 0, 0, 255]);
        let b = solid(6, 9, [0, 0, 0, 255]);

        assert_eq!(diff_image(&a, &b, DEFAULT_TOLERANCE).dimensions(), (6, 4));
    }

    #[test]
    fn images_with_no_overlap_do_not_divide_by_zero() {
        let a = solid(4, 4, [0, 0, 0, 255]);
        let b = RgbaImage::new(0, 4);

        let result = compare(&a, &b, DEFAULT_TOLERANCE);

        assert_eq!(result.total_pixels, 0);
        assert_eq!(result.difference_percent, 0.0);
    }
}
