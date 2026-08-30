//! Joining a sequence of scrolled screenshots into one tall image.
//!
//! This is the portable half of scrolling capture. Making a window scroll is
//! platform work and needs permissions; working out how far it *did* scroll and
//! joining the frames is arithmetic, so it lives here where it can be tested
//! against images built to have a known answer.
//!
//! # How the overlap is found
//!
//! Comparing every candidate offset pixel by pixel is O(height² · width), which
//! on a 1200×2000 region is billions of operations per frame. Instead each row
//! is reduced to a signature once, offsets are matched on signatures, and only
//! the winning offset is confirmed against real pixels. That turns the search
//! into O(height) work plus one verification pass.
//!
//! # What it refuses to guess
//!
//! Two things are reported rather than papered over, because both produce a
//! wrong picture that looks plausible:
//!
//! - **No overlap.** The window scrolled further than one screenful, so the
//!   content between the frames was never captured. Concatenating anyway would
//!   silently drop a paragraph.
//! - **No movement.** The window did not scroll, usually because it had already
//!   reached the bottom. Appending a duplicate would repeat content.

use image::RgbaImage;

/// Rows sampled per signature.
///
/// Enough that two different rows almost never collide, few enough that
/// building signatures for a tall frame stays trivial.
const SAMPLES: u32 = 24;

/// How closely two rows must match to count as the same, per channel.
///
/// Not zero: a scrolled window re-renders subpixel antialiasing and shadows
/// slightly differently, and demanding exactness finds no overlap at all on
/// real content.
const TOLERANCE: u8 = 12;

/// How many rows of the candidate overlap are verified against real pixels.
const VERIFY_ROWS: u32 = 8;

/// The least overlap worth trusting.
///
/// A handful of matching rows happens by chance on uniform content — a blank
/// margin matches any other blank margin.
const MIN_OVERLAP: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joined {
    /// The frames overlapped by this many rows and were joined.
    Overlap(u32),
    /// The frames are the same picture: the window did not scroll.
    NoMovement,
    /// Nothing matched: the window scrolled more than one screenful and the
    /// content in between was never captured.
    Gap,
}

/// Regions of the frame that do not scroll, and so must not be matched on.
///
/// A sticky header repeats identically in every frame, which makes a full
/// overlap look like a perfect match at every offset. ShareX calls these the
/// trim regions; the effect of getting them wrong is a capture that repeats the
/// toolbar down the page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trim {
    pub top: u32,
    pub bottom: u32,
}

impl Trim {
    /// The rows of `height` that actually scroll.
    fn body(&self, height: u32) -> std::ops::Range<u32> {
        let start = self.top.min(height);
        let end = height.saturating_sub(self.bottom).max(start);
        start..end
    }
}

/// A row reduced to a few sampled pixels.
type Signature = [[u8; 4]; SAMPLES as usize];

fn signatures(image: &RgbaImage, rows: std::ops::Range<u32>) -> Vec<Signature> {
    let width = image.width();
    if width == 0 {
        return Vec::new();
    }

    // Sample across the row rather than from one edge: a page with a wide
    // margin would otherwise reduce every row to the same background colour.
    let columns: Vec<u32> = (0..SAMPLES)
        .map(|i| (i * width / SAMPLES).min(width - 1))
        .collect();

    rows.map(|y| {
        let mut signature = [[0u8; 4]; SAMPLES as usize];
        for (slot, x) in columns.iter().enumerate() {
            signature[slot] = image.get_pixel(*x, y).0;
        }
        signature
    })
    .collect()
}

fn rows_match(a: &Signature, b: &Signature) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.iter()
            .zip(y.iter())
            .all(|(p, q)| p.abs_diff(*q) <= TOLERANCE)
    })
}

/// How many rows at the bottom of `previous` are also at the top of `next`.
///
/// Searches from the largest overlap down, so the first match found is the
/// most content shared — a smaller coincidental match further down cannot win.
pub fn overlap(previous: &RgbaImage, next: &RgbaImage, trim: Trim) -> Joined {
    if previous.dimensions() != next.dimensions() {
        return Joined::Gap;
    }

    let body = trim.body(previous.height());
    let rows = body.end.saturating_sub(body.start);
    if rows < MIN_OVERLAP {
        return Joined::Gap;
    }

    let a = signatures(previous, body.clone());
    let b = signatures(next, body.clone());

    // Identical frames mean the window did not move. Checked first, because a
    // full-height overlap is also what "did not move" looks like to the search
    // below, and the two need different answers.
    if a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| rows_match(x, y)) {
        return Joined::NoMovement;
    }

    for candidate in (MIN_OVERLAP..rows).rev() {
        let start = (rows - candidate) as usize;
        let matched = a[start..]
            .iter()
            .zip(b[..candidate as usize].iter())
            .all(|(x, y)| rows_match(x, y));

        if matched && verify(previous, next, &body, candidate) {
            return Joined::Overlap(candidate);
        }
    }

    Joined::Gap
}

/// Confirm a candidate overlap against real pixels.
///
/// Signatures sample two dozen columns; a page of body text can produce the
/// same signature for two different lines. This checks whole rows, spread
/// through the overlap rather than bunched at one end.
fn verify(
    previous: &RgbaImage,
    next: &RgbaImage,
    body: &std::ops::Range<u32>,
    candidate: u32,
) -> bool {
    let rows = body.end - body.start;
    let step = (candidate / VERIFY_ROWS).max(1);

    (0..candidate)
        .step_by(step as usize)
        .take(VERIFY_ROWS as usize)
        .all(|offset| {
            let previous_y = body.start + (rows - candidate) + offset;
            let next_y = body.start + offset;
            (0..previous.width()).all(|x| {
                let p = previous.get_pixel(x, previous_y).0;
                let q = next.get_pixel(x, next_y).0;
                p.iter()
                    .zip(q.iter())
                    .all(|(a, b)| a.abs_diff(*b) <= TOLERANCE)
            })
        })
}

/// What a stitch produced, and what it had to give up.
#[derive(Debug, Clone, PartialEq)]
pub struct Stitched {
    pub image: RgbaImage,
    /// Frames that were joined, including the first.
    pub frames_used: usize,
    /// True when a frame did not overlap the one before it, so content is
    /// missing. Reported rather than hidden: a capture with a paragraph
    /// silently removed is worse than one that says it is incomplete.
    pub had_gap: bool,
    /// True when the last frames were identical, which is how a scroll that
    /// reached the bottom looks.
    pub reached_end: bool,
}

/// Join frames top to bottom.
///
/// Frames are expected in scroll order. A frame that does not overlap its
/// predecessor is still appended — dropping it would lose *more* content, not
/// less — but `had_gap` says so.
pub fn stitch(frames: &[RgbaImage], trim: Trim) -> Option<Stitched> {
    let first = frames.first()?;
    let width = first.width();
    if width == 0 || first.height() == 0 {
        return None;
    }

    let mut rows: Vec<RgbaImage> = vec![first.clone()];
    let mut offsets: Vec<u32> = vec![0];
    let mut had_gap = false;
    let mut reached_end = false;

    for pair in frames.windows(2) {
        match overlap(&pair[0], &pair[1], trim) {
            Joined::Overlap(shared) => {
                offsets.push(shared);
                rows.push(pair[1].clone());
            }
            Joined::NoMovement => {
                // Nothing new below this point; anything after would repeat.
                reached_end = true;
                break;
            }
            Joined::Gap => {
                had_gap = true;
                offsets.push(0);
                rows.push(pair[1].clone());
            }
        }
    }

    let total: u32 = rows
        .iter()
        .zip(offsets.iter())
        .map(|(frame, shared)| frame.height() - shared)
        .sum();

    let mut canvas = RgbaImage::new(width, total);
    let mut y = 0u32;
    for (frame, shared) in rows.iter().zip(offsets.iter()) {
        for row in *shared..frame.height() {
            for x in 0..width {
                canvas.put_pixel(x, y, *frame.get_pixel(x, row));
            }
            y += 1;
        }
    }

    Some(Stitched {
        image: canvas,
        frames_used: rows.len(),
        had_gap,
        reached_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A tall page of distinguishable rows, the way real content is.
    fn page(width: u32, height: u32) -> RgbaImage {
        let mut image = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                // Vary with both axes so no two rows share a signature and no
                // row is uniform.
                let r = (y % 251) as u8;
                let g = ((y / 251 + x) % 253) as u8;
                let b = ((x * 7 + y * 13) % 249) as u8;
                image.put_pixel(x, y, Rgba([r, g, b, 255]));
            }
        }
        image
    }

    /// A viewport onto `source`, scrolled down by `offset` rows.
    fn viewport(source: &RgbaImage, offset: u32, height: u32) -> RgbaImage {
        let mut frame = RgbaImage::new(source.width(), height);
        for y in 0..height {
            for x in 0..source.width() {
                let source_y = (offset + y).min(source.height() - 1);
                frame.put_pixel(x, y, *source.get_pixel(x, source_y));
            }
        }
        frame
    }

    #[test]
    fn a_known_scroll_is_measured_exactly() {
        // The whole feature rests on this number. A frame 100 rows tall
        // scrolled by 40 shares its last 60 rows with the next one's first 60.
        let source = page(80, 400);
        let a = viewport(&source, 0, 100);
        let b = viewport(&source, 40, 100);

        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Overlap(60));
    }

    #[test]
    fn several_scroll_distances_all_measure_correctly() {
        let source = page(60, 600);
        for scrolled in [20u32, 35, 64, 99] {
            let a = viewport(&source, 0, 120);
            let b = viewport(&source, scrolled, 120);
            assert_eq!(
                overlap(&a, &b, Trim::default()),
                Joined::Overlap(120 - scrolled),
                "scrolled by {scrolled}"
            );
        }
    }

    #[test]
    fn an_unmoved_window_is_reported_as_such_not_as_a_full_overlap() {
        // This is how "already at the bottom" looks, and appending the frame
        // would repeat the content that is already there.
        let frame = page(50, 90);
        assert_eq!(
            overlap(&frame, &frame.clone(), Trim::default()),
            Joined::NoMovement
        );
    }

    #[test]
    fn scrolling_past_a_whole_screen_is_reported_as_a_gap() {
        // Concatenating here would silently drop everything in between.
        let source = page(50, 900);
        let a = viewport(&source, 0, 100);
        let b = viewport(&source, 400, 100);

        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Gap);
    }

    #[test]
    fn frames_of_different_sizes_do_not_pretend_to_overlap() {
        let a = page(50, 100);
        let b = page(60, 100);
        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Gap);
    }

    #[test]
    fn a_sticky_header_does_not_defeat_the_match() {
        // A toolbar that stays put is identical in every frame. Without
        // trimming it the search finds a perfect match at the wrong offset and
        // the result repeats the toolbar down the page.
        let source = page(70, 500);
        let header = page(70, 20);

        let mut a = viewport(&source, 0, 120);
        let mut b = viewport(&source, 45, 120);
        for y in 0..20 {
            for x in 0..70 {
                a.put_pixel(x, y, *header.get_pixel(x, y));
                b.put_pixel(x, y, *header.get_pixel(x, y));
            }
        }

        let trim = Trim { top: 20, bottom: 0 };
        assert_eq!(overlap(&a, &b, trim), Joined::Overlap(120 - 20 - 45));
    }

    #[test]
    fn small_differences_from_re_rendering_are_tolerated() {
        // A scrolled window redraws antialiasing and shadows slightly
        // differently; demanding exact pixels finds no overlap on real content.
        let source = page(60, 400);
        let a = viewport(&source, 0, 100);
        let mut b = viewport(&source, 30, 100);
        for pixel in b.pixels_mut() {
            pixel[0] = pixel[0].saturating_add(3);
        }

        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Overlap(70));
    }

    #[test]
    fn a_difference_too_large_to_be_re_rendering_is_not_tolerated() {
        let source = page(60, 400);
        let a = viewport(&source, 0, 100);
        let mut b = viewport(&source, 30, 100);
        for pixel in b.pixels_mut() {
            pixel[0] = pixel[0].wrapping_add(90);
        }

        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Gap);
    }

    #[test]
    fn two_frames_join_into_one_taller_image() {
        let source = page(40, 400);
        let frames = vec![viewport(&source, 0, 100), viewport(&source, 40, 100)];

        let result = stitch(&frames, Trim::default()).expect("stitches");

        assert_eq!(result.image.dimensions(), (40, 140));
        assert_eq!(result.frames_used, 2);
        assert!(!result.had_gap);
    }

    #[test]
    fn the_joined_image_is_the_original_page() {
        // The point of the whole exercise: the result has to be what the page
        // looked like, not merely the right size.
        let source = page(40, 400);
        let frames = vec![
            viewport(&source, 0, 100),
            viewport(&source, 40, 100),
            viewport(&source, 80, 100),
        ];

        let result = stitch(&frames, Trim::default()).expect("stitches");

        assert_eq!(result.image.height(), 180);
        for y in 0..180 {
            for x in 0..40 {
                assert_eq!(
                    result.image.get_pixel(x, y),
                    source.get_pixel(x, y),
                    "row {y} column {x}"
                );
            }
        }
    }

    #[test]
    fn stitching_stops_when_the_page_stops_moving() {
        // Everything after a repeat is a repeat; the extra frames are dropped
        // rather than appended.
        let source = page(40, 300);
        let last = viewport(&source, 60, 100);
        let frames = vec![
            viewport(&source, 0, 100),
            viewport(&source, 60, 100),
            last.clone(),
            last,
        ];

        let result = stitch(&frames, Trim::default()).expect("stitches");

        assert_eq!(result.frames_used, 2);
        assert!(result.reached_end);
        assert_eq!(result.image.height(), 160);
    }

    #[test]
    fn a_gap_is_reported_rather_than_hidden() {
        // The frame is still appended — dropping it would lose more, not less
        // — but the caller is told the result is incomplete.
        let source = page(40, 1200);
        let frames = vec![viewport(&source, 0, 100), viewport(&source, 700, 100)];

        let result = stitch(&frames, Trim::default()).expect("stitches");

        assert!(result.had_gap);
        assert_eq!(result.frames_used, 2);
        assert_eq!(result.image.height(), 200);
    }

    #[test]
    fn one_frame_stitches_to_itself() {
        let frame = page(30, 50);
        let result = stitch(std::slice::from_ref(&frame), Trim::default()).expect("stitches");

        assert_eq!(result.image.dimensions(), (30, 50));
        assert_eq!(result.frames_used, 1);
        assert!(!result.had_gap);
    }

    #[test]
    fn no_frames_stitch_to_nothing() {
        assert!(stitch(&[], Trim::default()).is_none());
    }

    #[test]
    fn a_zero_sized_frame_is_refused_rather_than_producing_an_empty_image() {
        // An empty image cannot be encoded, so returning one would fail later
        // with a confusing message about PNG.
        assert!(stitch(&[RgbaImage::new(0, 10)], Trim::default()).is_none());
        assert!(stitch(&[RgbaImage::new(10, 0)], Trim::default()).is_none());
    }

    #[test]
    fn a_frame_shorter_than_the_minimum_overlap_is_a_gap_not_a_panic() {
        let a = page(20, 8);
        let b = page(20, 8);
        assert_eq!(overlap(&a, &b, Trim::default()), Joined::Gap);
    }

    #[test]
    fn trimming_more_than_the_frame_does_not_panic() {
        let a = page(20, 40);
        let b = page(20, 40);
        let trim = Trim {
            top: 100,
            bottom: 100,
        };
        assert_eq!(overlap(&a, &b, trim), Joined::Gap);
    }
}
