//! Rasterising a [`Document`] onto the captured image.
//!
//! Primitives are hand-written rather than taken from `imageproc` because
//! every annotation needs alpha blending and most need anti-aliasing, and
//! `imageproc`'s drawing routines write opaque pixels. Coverage-based drawing
//! (distance to the shape, converted to alpha) gives both for free and keeps
//! the code uniform across primitives.

use image::{Rgba, RgbaImage};

use crate::document::Document;
use crate::font;
use crate::shape::{ArrowHead, Color, Point, Rect, Shape, Stroke};

/// Draw every annotation onto a copy of `base`, then apply the frame.
///
/// The order matters: annotations are positioned in the base image's
/// coordinate space, so cropping or padding has to happen afterwards — else
/// every shape would need adjusting whenever the frame changed.
pub fn render(base: &RgbaImage, document: &Document) -> RgbaImage {
    let mut canvas = base.clone();
    for shape in document.shapes() {
        draw_shape(&mut canvas, shape);
    }
    crate::frame::apply(&canvas, document.frame())
}

fn draw_shape(canvas: &mut RgbaImage, shape: &Shape) {
    match shape {
        Shape::Rectangle {
            rect,
            stroke,
            fill,
            corner_radius,
        } => {
            if !fill.is_transparent() {
                fill_rounded_rect(canvas, *rect, *corner_radius, *fill);
            }
            stroke_rounded_rect(canvas, *rect, *corner_radius, *stroke);
        }
        Shape::Ellipse { rect, stroke, fill } => {
            if !fill.is_transparent() {
                fill_ellipse(canvas, *rect, *fill);
            }
            stroke_ellipse(canvas, *rect, *stroke);
        }
        Shape::Line { from, to, stroke } => {
            thick_line(canvas, *from, *to, stroke.width, stroke.color);
        }
        Shape::Arrow {
            from,
            to,
            stroke,
            head,
        } => draw_arrow(canvas, *from, *to, *stroke, *head),
        Shape::Freehand { points, stroke } => {
            for pair in points.windows(2) {
                thick_line(canvas, pair[0], pair[1], stroke.width, stroke.color);
            }
            // A single tap should still leave a mark.
            if points.len() == 1 {
                thick_line(canvas, points[0], points[0], stroke.width, stroke.color);
            }
        }
        Shape::Highlight { rect, color } => fill_rect(canvas, *rect, *color),
        Shape::Blur { rect, radius } => blur_region(canvas, *rect, *radius),
        Shape::Pixelate { rect, block } => pixelate_region(canvas, *rect, *block),
        Shape::Spotlight { rect, dim } => spotlight(canvas, *rect, *dim),
        Shape::Image {
            rect,
            data,
            opacity,
        } => draw_image(canvas, *rect, data, *opacity),
        Shape::Step {
            center,
            radius,
            number,
            fill,
            text_color,
        } => {
            fill_circle(canvas, *center, *radius, *fill);
            let label = number.to_string();
            // The digits should fill the badge without touching its edge.
            draw_centered_text(canvas, &label, *center, radius * 1.15, *text_color);
        }
        Shape::Text {
            rect,
            content,
            color,
            outline,
            size,
            bold,
            italic,
        } => draw_text(
            canvas,
            content,
            (rect.x, rect.y),
            *size,
            *bold,
            *italic,
            *color,
            *outline,
        ),
        Shape::SpeechBalloon {
            rect,
            tail,
            content,
            stroke,
            fill,
            text_color,
            size,
        } => {
            // The tail is drawn first so the bubble's fill covers where the two
            // meet, leaving no seam.
            draw_balloon_tail(canvas, *rect, *tail, *fill);
            let radius = (rect.height * 0.25).min(18.0);
            fill_rounded_rect(canvas, *rect, radius, *fill);
            stroke_rounded_rect(canvas, *rect, radius, *stroke);
            draw_centered_text(canvas, content, rect.center(), *size, *text_color);
        }
    }
}

// ── Text ────────────────────────────────────────────────────────────────

/// Draw `text` with its top-left corner at `origin`.
///
/// An outline colour is applied by stamping the glyphs around the fill in eight
/// directions. That is cheap and, at annotation sizes, indistinguishable from a
/// real stroked outline — and it is what makes light text stay legible over an
/// arbitrary screenshot.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    canvas: &mut RgbaImage,
    text: &str,
    origin: (f32, f32),
    size: f32,
    bold: bool,
    italic: bool,
    color: Color,
    outline: Color,
) {
    if text.is_empty() || size <= 0.0 {
        return;
    }
    let Some(fonts) = font::system() else {
        tracing::warn!("no system font is available; text will not be rendered");
        return;
    };

    if !outline.is_transparent() {
        const OFFSETS: [(f32, f32); 8] = [
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
            (-1.0, 1.0),
            (0.0, 1.0),
            (1.0, 1.0),
        ];
        let spread = (size / 16.0).clamp(1.0, 3.0);
        for (dx, dy) in OFFSETS {
            fonts.rasterize(
                text,
                size,
                bold,
                italic,
                (origin.0 + dx * spread, origin.1 + dy * spread),
                |x, y, coverage| blend(canvas, x as i64, y as i64, outline, coverage),
            );
        }
    }

    fonts.rasterize(text, size, bold, italic, origin, |x, y, coverage| {
        blend(canvas, x as i64, y as i64, color, coverage)
    });
}

/// Draw `text` centred on `center`, with no outline.
fn draw_centered_text(canvas: &mut RgbaImage, text: &str, center: Point, size: f32, color: Color) {
    if text.is_empty() || size <= 0.0 {
        return;
    }
    let Some(fonts) = font::system() else { return };

    let (width, height) = fonts.measure(text, size, true, false);
    let origin = (center.x - width / 2.0, center.y - height / 2.0);

    fonts.rasterize(text, size, true, false, origin, |x, y, coverage| {
        blend(canvas, x as i64, y as i64, color, coverage)
    });
}

/// The pointer from a speech balloon towards whatever it is labelling.
fn draw_balloon_tail(canvas: &mut RgbaImage, rect: Rect, tail: Point, fill: Color) {
    let center = rect.center();
    let dx = tail.x - center.x;
    let dy = tail.y - center.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
        return;
    }

    // Base the tail on the bubble edge, perpendicular to the direction it points.
    let (ux, uy) = (dx / len, dy / len);
    let half = (rect.width.min(rect.height) * 0.18).max(6.0);
    let base = Point::new(center.x + ux * (len * 0.2), center.y + uy * (len * 0.2));

    fill_triangle(
        canvas,
        tail,
        Point::new(base.x - uy * half, base.y + ux * half),
        Point::new(base.x + uy * half, base.y - ux * half),
        fill,
    );
}

// ── Pixel plumbing ──────────────────────────────────────────────────────

/// Source-over composite of a straight-alpha colour onto the canvas.
fn blend(canvas: &mut RgbaImage, x: i64, y: i64, color: Color, coverage: f32) {
    if coverage <= 0.0 || color.a == 0 {
        return;
    }
    if x < 0 || y < 0 || x >= canvas.width() as i64 || y >= canvas.height() as i64 {
        return;
    }

    let alpha = (color.a as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }

    let pixel = canvas.get_pixel_mut(x as u32, y as u32);
    let mix = |src: u8, dst: u8| (src as f32 * alpha + dst as f32 * (1.0 - alpha)).round() as u8;

    pixel[0] = mix(color.r, pixel[0]);
    pixel[1] = mix(color.g, pixel[1]);
    pixel[2] = mix(color.b, pixel[2]);
    // The capture is opaque; keep it that way so exports have no stray holes.
    pixel[3] = pixel[3].max((alpha * 255.0).round() as u8);
}

/// Iterate the canvas pixels a rectangle covers, clipped to the image.
fn clipped_bounds(canvas: &RgbaImage, rect: Rect) -> Option<(i64, i64, i64, i64)> {
    let x0 = rect.x.floor() as i64;
    let y0 = rect.y.floor() as i64;
    let x1 = rect.right().ceil() as i64;
    let y1 = rect.bottom().ceil() as i64;

    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(canvas.width() as i64);
    let y1 = y1.min(canvas.height() as i64);

    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

// ── Primitives ──────────────────────────────────────────────────────────

fn fill_rect(canvas: &mut RgbaImage, rect: Rect, color: Color) {
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, rect) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            blend(canvas, x, y, color, 1.0);
        }
    }
}

/// Signed distance from a point to a rounded rectangle. Negative inside.
fn rounded_rect_distance(rect: Rect, radius: f32, px: f32, py: f32) -> f32 {
    let radius = radius.max(0.0).min(rect.width / 2.0).min(rect.height / 2.0);
    let center = rect.center();
    let half_w = rect.width / 2.0 - radius;
    let half_h = rect.height / 2.0 - radius;

    let dx = (px - center.x).abs() - half_w;
    let dy = (py - center.y).abs() - half_h;

    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0) - radius
}

fn fill_rounded_rect(canvas: &mut RgbaImage, rect: Rect, radius: f32, color: Color) {
    if radius <= 0.0 {
        fill_rect(canvas, rect, color);
        return;
    }
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, rect) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let d = rounded_rect_distance(rect, radius, x as f32 + 0.5, y as f32 + 0.5);
            blend(canvas, x, y, color, coverage_from_distance(d));
        }
    }
}

fn stroke_rounded_rect(canvas: &mut RgbaImage, rect: Rect, radius: f32, stroke: Stroke) {
    if stroke.width <= 0.0 || stroke.color.is_transparent() {
        return;
    }
    let half = stroke.width / 2.0;
    let outer = Rect::new(
        rect.x - half - 1.0,
        rect.y - half - 1.0,
        rect.width + stroke.width + 2.0,
        rect.height + stroke.width + 2.0,
    );
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, outer) else {
        return;
    };

    for y in y0..y1 {
        for x in x0..x1 {
            let d = rounded_rect_distance(rect, radius, x as f32 + 0.5, y as f32 + 0.5);
            // The stroke straddles the edge, so it is the band |d| < half.
            blend(
                canvas,
                x,
                y,
                stroke.color,
                coverage_from_distance(d.abs() - half),
            );
        }
    }
}

/// Convert a signed distance to anti-aliased coverage over one pixel.
fn coverage_from_distance(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

fn ellipse_distance(rect: Rect, px: f32, py: f32) -> f32 {
    let center = rect.center();
    let rx = (rect.width / 2.0).max(0.0001);
    let ry = (rect.height / 2.0).max(0.0001);
    let nx = (px - center.x) / rx;
    let ny = (py - center.y) / ry;
    // Approximate signed distance: exact for circles, close enough for the
    // eccentricities an annotation uses.
    let k = (nx * nx + ny * ny).sqrt();
    (k - 1.0) * rx.min(ry)
}

fn fill_ellipse(canvas: &mut RgbaImage, rect: Rect, color: Color) {
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, rect) else {
        return;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ellipse_distance(rect, x as f32 + 0.5, y as f32 + 0.5);
            blend(canvas, x, y, color, coverage_from_distance(d));
        }
    }
}

fn stroke_ellipse(canvas: &mut RgbaImage, rect: Rect, stroke: Stroke) {
    if stroke.width <= 0.0 || stroke.color.is_transparent() {
        return;
    }
    let half = stroke.width / 2.0;
    let outer = Rect::new(
        rect.x - half - 1.0,
        rect.y - half - 1.0,
        rect.width + stroke.width + 2.0,
        rect.height + stroke.width + 2.0,
    );
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, outer) else {
        return;
    };

    for y in y0..y1 {
        for x in x0..x1 {
            let d = ellipse_distance(rect, x as f32 + 0.5, y as f32 + 0.5);
            blend(
                canvas,
                x,
                y,
                stroke.color,
                coverage_from_distance(d.abs() - half),
            );
        }
    }
}

fn fill_circle(canvas: &mut RgbaImage, center: Point, radius: f32, color: Color) {
    fill_ellipse(
        canvas,
        Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        ),
        color,
    );
}

/// Distance from `p` to the segment `a`–`b`.
fn distance_to_segment(a: Point, b: Point, px: f32, py: f32) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;

    // A zero-length segment is a point — a single freehand tap hits this.
    if len_sq <= f32::EPSILON {
        return ((px - a.x).powi(2) + (py - a.y).powi(2)).sqrt();
    }

    let t = (((px - a.x) * dx + (py - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    let cx = a.x + t * dx;
    let cy = a.y + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn thick_line(canvas: &mut RgbaImage, a: Point, b: Point, width: f32, color: Color) {
    if width <= 0.0 || color.is_transparent() {
        return;
    }
    let half = width / 2.0;
    let bounds = Rect::from_corners(
        Point::new(a.x.min(b.x) - half - 1.0, a.y.min(b.y) - half - 1.0),
        Point::new(a.x.max(b.x) + half + 1.0, a.y.max(b.y) + half + 1.0),
    );
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, bounds) else {
        return;
    };

    for y in y0..y1 {
        for x in x0..x1 {
            let d = distance_to_segment(a, b, x as f32 + 0.5, y as f32 + 0.5);
            blend(canvas, x, y, color, coverage_from_distance(d - half));
        }
    }
}

fn draw_arrow(canvas: &mut RgbaImage, from: Point, to: Point, stroke: Stroke, head: ArrowHead) {
    let head_len = (stroke.width * 4.0).max(12.0);
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();

    if len <= f32::EPSILON {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);

    // Stop the shaft short of the head so the tip stays sharp.
    let shaft_end = match head {
        ArrowHead::None => to,
        _ => Point::new(to.x - ux * head_len * 0.6, to.y - uy * head_len * 0.6),
    };
    let shaft_start = match head {
        ArrowHead::Both => Point::new(from.x + ux * head_len * 0.6, from.y + uy * head_len * 0.6),
        _ => from,
    };

    thick_line(canvas, shaft_start, shaft_end, stroke.width, stroke.color);

    if matches!(head, ArrowHead::End | ArrowHead::Both) {
        arrow_head(canvas, to, ux, uy, head_len, stroke.color);
    }
    if matches!(head, ArrowHead::Both) {
        arrow_head(canvas, from, -ux, -uy, head_len, stroke.color);
    }
}

fn arrow_head(canvas: &mut RgbaImage, tip: Point, ux: f32, uy: f32, len: f32, color: Color) {
    let half_width = len * 0.42;
    let base = Point::new(tip.x - ux * len, tip.y - uy * len);
    // Perpendicular to the direction of travel.
    let (px, py) = (-uy, ux);
    let left = Point::new(base.x + px * half_width, base.y + py * half_width);
    let right = Point::new(base.x - px * half_width, base.y - py * half_width);

    fill_triangle(canvas, tip, left, right, color);
}

fn fill_triangle(canvas: &mut RgbaImage, a: Point, b: Point, c: Point, color: Color) {
    let bounds = Rect::bounding(&[a, b, c]).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, inflate(bounds, 1.0)) else {
        return;
    };

    let edge =
        |p: Point, q: Point, px: f32, py: f32| (q.x - p.x) * (py - p.y) - (q.y - p.y) * (px - p.x);

    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(b, c, px, py);
            let w1 = edge(c, a, px, py);
            let w2 = edge(a, b, px, py);
            // Accept either winding so callers need not care about vertex order.
            let inside =
                (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
            if inside {
                blend(canvas, x, y, color, 1.0);
            }
        }
    }
}

fn inflate(rect: Rect, by: f32) -> Rect {
    Rect::new(
        rect.x - by,
        rect.y - by,
        rect.width + by * 2.0,
        rect.height + by * 2.0,
    )
}

// ── Redaction ───────────────────────────────────────────────────────────

/// Box blur applied only inside `rect`, run twice to approximate a Gaussian.
///
/// Sampling is clamped to the rectangle, not the whole image: pulling in
/// neighbouring pixels would bleed unredacted content back into the area the
/// user is trying to hide.
fn blur_region(canvas: &mut RgbaImage, rect: Rect, radius: f32) {
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, rect) else {
        return;
    };
    let radius = radius.round().max(1.0) as i64;

    for _ in 0..2 {
        let snapshot = canvas.clone();
        for y in y0..y1 {
            for x in x0..x1 {
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for sy in (y - radius).max(y0)..=(y + radius).min(y1 - 1) {
                    for sx in (x - radius).max(x0)..=(x + radius).min(x1 - 1) {
                        let p = snapshot.get_pixel(sx as u32, sy as u32);
                        r += p[0] as u32;
                        g += p[1] as u32;
                        b += p[2] as u32;
                        n += 1;
                    }
                }
                if n == 0 {
                    continue;
                }
                let pixel = canvas.get_pixel_mut(x as u32, y as u32);
                *pixel = Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, pixel[3]]);
            }
        }
    }
}

/// Replace each block with its average colour.
fn pixelate_region(canvas: &mut RgbaImage, rect: Rect, block: u32) {
    let Some((x0, y0, x1, y1)) = clipped_bounds(canvas, rect) else {
        return;
    };
    let block = block.max(2) as i64;

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in by..(by + block).min(y1) {
                for x in bx..(bx + block).min(x1) {
                    let p = canvas.get_pixel(x as u32, y as u32);
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    n += 1;
                }
            }
            if n > 0 {
                let average = Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, 255]);
                for y in by..(by + block).min(y1) {
                    for x in bx..(bx + block).min(x1) {
                        let pixel = canvas.get_pixel_mut(x as u32, y as u32);
                        *pixel = Rgba([average[0], average[1], average[2], pixel[3]]);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

/// Darken everything outside the rectangle.
/// Composite a pasted image into `rect`.
///
/// A pasted image that cannot be decoded is skipped rather than drawn as a
/// placeholder. The document is data, so it can arrive from a file someone
/// edited by hand; a broken entry should cost that one image, not the export.
fn draw_image(canvas: &mut RgbaImage, rect: Rect, data: &str, opacity: f32) {
    let Some(source) = decode_png(data) else {
        tracing::warn!("skipping a pasted image that could not be decoded");
        return;
    };

    let width = rect.width.round().max(1.0) as u32;
    let height = rect.height.round().max(1.0) as u32;
    let scaled = image::imageops::resize(
        &source,
        width,
        height,
        image::imageops::FilterType::CatmullRom,
    );

    let opacity = opacity.clamp(0.0, 1.0);
    let left = rect.x.round() as i64;
    let top = rect.y.round() as i64;

    for (x, y, pixel) in scaled.enumerate_pixels() {
        let target_x = left + x as i64;
        let target_y = top + y as i64;
        // Coverage carries the image's own alpha *and* the shape's opacity, so
        // a transparent logo stays transparent instead of being flattened onto
        // an opaque box.
        let coverage = (pixel[3] as f32 / 255.0) * opacity;
        blend(
            canvas,
            target_x,
            target_y,
            Color::rgb(pixel[0], pixel[1], pixel[2]),
            coverage,
        );
    }
}

fn decode_png(data: &str) -> Option<RgbaImage> {
    use base64::Engine as _;

    // Accept a bare base64 payload or a full data URL, because the webview
    // produces the latter and a hand-written document is likely to hold the
    // former.
    let payload = data.rsplit_once(",").map(|(_, rest)| rest).unwrap_or(data);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()?;

    image::load_from_memory(&bytes)
        .ok()
        .map(|image| image.to_rgba8())
}

fn spotlight(canvas: &mut RgbaImage, rect: Rect, dim: f32) {
    let alpha = (dim.clamp(0.0, 1.0) * 255.0).round() as u8;
    let shade = Color::rgba(0, 0, 0, alpha);

    for y in 0..canvas.height() as i64 {
        for x in 0..canvas.width() as i64 {
            let inside = rect.contains(Point::new(x as f32 + 0.5, y as f32 + 0.5));
            if !inside {
                blend(canvas, x, y, shade, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    /// A solid PNG, base64 encoded, the way a paste arrives.
    fn png_data_url(width: u32, height: u32, colour: [u8; 4]) -> String {
        use base64::Engine as _;
        let image = RgbaImage::from_pixel(width, height, Rgba(colour));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode");
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        )
    }

    #[test]
    fn a_pasted_image_lands_where_it_was_put() {
        let mut canvas = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(10.0, 10.0, 10.0, 10.0),
                data: png_data_url(4, 4, [255, 0, 0, 255]),
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(15, 15), &Rgba([255, 0, 0, 255]));
        assert_eq!(
            canvas.get_pixel(2, 2),
            &Rgba([0, 0, 0, 255]),
            "outside untouched"
        );
    }

    #[test]
    fn a_pasted_image_is_scaled_to_its_rectangle() {
        // The source is 4x4 and the rect is 20x20; without scaling only a
        // corner would be painted.
        let mut canvas = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                data: png_data_url(4, 4, [0, 255, 0, 255]),
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(19, 19), &Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn opacity_blends_rather_than_replacing() {
        let mut canvas = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: png_data_url(2, 2, [255, 255, 255, 255]),
                opacity: 0.5,
            },
        );

        let pixel = canvas.get_pixel(5, 5);
        assert!(
            pixel[0] > 100 && pixel[0] < 160,
            "should be mid grey: {pixel:?}"
        );
    }

    #[test]
    fn a_transparent_source_stays_transparent() {
        // A logo with an alpha channel must not be flattened onto an opaque box.
        let mut canvas = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 255, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: png_data_url(2, 2, [255, 0, 0, 0]),
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(5, 5), &Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn an_image_pasted_partly_off_canvas_is_clipped_not_wrapped() {
        // Painting past the edge would either panic or wrap around to the other
        // side, and both look like corruption.
        let mut canvas = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(15.0, 15.0, 20.0, 20.0),
                data: png_data_url(2, 2, [255, 255, 0, 255]),
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(18, 18), &Rgba([255, 255, 0, 255]));
        assert_eq!(
            canvas.get_pixel(1, 1),
            &Rgba([0, 0, 0, 255]),
            "no wrap-around"
        );
    }

    #[test]
    fn an_undecodable_image_costs_only_itself() {
        // Documents are data and can be hand-edited; one broken entry must not
        // take the export with it.
        let mut canvas = RgbaImage::from_pixel(20, 20, Rgba([7, 7, 7, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: "not base64 at all".into(),
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(5, 5), &Rgba([7, 7, 7, 255]));
    }

    #[test]
    fn a_bare_base64_payload_works_as_well_as_a_data_url() {
        let url = png_data_url(2, 2, [10, 200, 10, 255]);
        let bare = url.rsplit_once(',').unwrap().1.to_string();

        let mut canvas = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        draw_shape(
            &mut canvas,
            &Shape::Image {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: bare,
                opacity: 1.0,
            },
        );

        assert_eq!(canvas.get_pixel(5, 5), &Rgba([10, 200, 10, 255]));
    }

    use super::*;
    use crate::shape::Stroke;

    fn canvas(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
    }

    fn noisy(w: u32, h: u32) -> RgbaImage {
        // A deterministic checkerboard: blur and pixelate must visibly change it.
        RgbaImage::from_fn(w, h, |x, y| {
            if (x + y) % 2 == 0 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        })
    }

    fn stroke(width: f32) -> Stroke {
        Stroke {
            color: Color::rgb(255, 0, 0),
            width,
        }
    }

    #[test]
    fn an_empty_document_leaves_the_image_untouched() {
        let base = canvas(16, 16);
        let out = render(&base, &Document::new());
        assert_eq!(out, base);
    }

    #[test]
    fn a_filled_rectangle_paints_inside_and_not_outside() {
        let mut image = canvas(40, 40);
        fill_rect(
            &mut image,
            Rect::new(10.0, 10.0, 10.0, 10.0),
            Color::rgb(0, 0, 255),
        );

        assert_eq!(image.get_pixel(15, 15), &Rgba([0, 0, 255, 255]));
        assert_eq!(image.get_pixel(5, 5), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn half_transparent_fill_blends_rather_than_replaces() {
        let mut image = canvas(10, 10);
        fill_rect(
            &mut image,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgba(0, 0, 0, 128),
        );

        let pixel = image.get_pixel(5, 5);
        assert!(
            pixel[0] > 100 && pixel[0] < 160,
            "expected a mid grey, got {pixel:?}"
        );
    }

    #[test]
    fn drawing_is_clipped_at_the_image_edge() {
        let mut image = canvas(8, 8);
        // Mostly off-canvas in both directions.
        fill_rect(
            &mut image,
            Rect::new(-50.0, -50.0, 100.0, 100.0),
            Color::rgb(0, 255, 0),
        );
        thick_line(
            &mut image,
            Point::new(-100.0, -100.0),
            Point::new(200.0, 200.0),
            5.0,
            Color::rgb(0, 0, 255),
        );
        // Reaching this line without a panic is the assertion.
        assert_eq!(image.width(), 8);
    }

    #[test]
    fn a_rectangle_outline_draws_its_border_and_leaves_the_middle_alone() {
        let mut image = canvas(60, 60);
        stroke_rounded_rect(
            &mut image,
            Rect::new(10.0, 10.0, 40.0, 40.0),
            0.0,
            stroke(4.0),
        );

        // On the top edge.
        assert!(image.get_pixel(30, 10)[0] > 200 && image.get_pixel(30, 10)[1] < 100);
        // Well inside stays untouched.
        assert_eq!(image.get_pixel(30, 30), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn a_line_marks_pixels_along_its_path() {
        let mut image = canvas(50, 50);
        thick_line(
            &mut image,
            Point::new(5.0, 25.0),
            Point::new(45.0, 25.0),
            3.0,
            Color::rgb(255, 0, 0),
        );

        assert!(image.get_pixel(25, 25)[1] < 100, "the line should be here");
        assert_eq!(
            image.get_pixel(25, 45),
            &Rgba([255, 255, 255, 255]),
            "and nowhere near here"
        );
    }

    #[test]
    fn an_arrow_head_is_wider_than_its_shaft() {
        let mut image = canvas(80, 80);
        draw_arrow(
            &mut image,
            Point::new(10.0, 40.0),
            Point::new(70.0, 40.0),
            stroke(3.0),
            ArrowHead::End,
        );

        let painted_at = |x: u32, y: u32| image.get_pixel(x, y)[1] < 200;
        let column_height = |x: u32| (0..80).filter(|y| painted_at(x, *y)).count();

        assert!(
            column_height(60) > column_height(25),
            "the head must flare out beyond the shaft"
        );
    }

    #[test]
    fn pixelate_flattens_each_block_to_one_colour() {
        let mut image = noisy(16, 16);
        pixelate_region(&mut image, Rect::new(0.0, 0.0, 16.0, 16.0), 4);

        let first = *image.get_pixel(0, 0);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(*image.get_pixel(x, y), first, "a block must be uniform");
            }
        }
    }

    #[test]
    fn pixelate_leaves_pixels_outside_the_rectangle_alone() {
        let original = noisy(20, 20);
        let mut image = original.clone();
        pixelate_region(&mut image, Rect::new(0.0, 0.0, 8.0, 8.0), 4);

        assert_eq!(image.get_pixel(15, 15), original.get_pixel(15, 15));
    }

    #[test]
    fn blur_removes_high_frequency_detail() {
        let original = noisy(24, 24);
        let mut image = original.clone();
        blur_region(&mut image, Rect::new(4.0, 4.0, 16.0, 16.0), 3.0);

        let centre = image.get_pixel(12, 12);
        assert!(
            centre[0] > 40 && centre[0] < 215,
            "a blurred checkerboard should average towards grey, got {centre:?}"
        );
        assert_eq!(
            image.get_pixel(0, 0),
            original.get_pixel(0, 0),
            "outside the rectangle must be untouched"
        );
    }

    #[test]
    fn blur_does_not_pull_in_pixels_from_outside_the_redacted_area() {
        // Left half black, right half white. Blurring only the right half must
        // not drag the black in — that would mean unredacted content leaking
        // into the area, and the reverse leak is what makes redaction unsafe.
        let mut image = RgbaImage::from_fn(40, 20, |x, _| {
            if x < 20 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        blur_region(&mut image, Rect::new(20.0, 0.0, 20.0, 20.0), 5.0);

        assert_eq!(
            image.get_pixel(21, 10),
            &Rgba([255, 255, 255, 255]),
            "sampling must be clamped to the rectangle"
        );
    }

    #[test]
    fn spotlight_dims_outside_and_preserves_inside() {
        let mut image = canvas(40, 40);
        spotlight(&mut image, Rect::new(10.0, 10.0, 20.0, 20.0), 0.6);

        assert_eq!(image.get_pixel(20, 20), &Rgba([255, 255, 255, 255]));
        assert!(image.get_pixel(2, 2)[0] < 150, "outside must be darkened");
    }

    #[test]
    fn shapes_paint_in_document_order() {
        let base = canvas(40, 40);
        let mut document = Document::new();
        document.push(Shape::Rectangle {
            rect: Rect::new(5.0, 5.0, 30.0, 30.0),
            stroke: Stroke {
                color: Color::TRANSPARENT,
                width: 0.0,
            },
            fill: Color::rgb(255, 0, 0),
            corner_radius: 0.0,
        });
        document.push(Shape::Rectangle {
            rect: Rect::new(5.0, 5.0, 30.0, 30.0),
            stroke: Stroke {
                color: Color::TRANSPARENT,
                width: 0.0,
            },
            fill: Color::rgb(0, 0, 255),
            corner_radius: 0.0,
        });

        let out = render(&base, &document);

        assert_eq!(
            out.get_pixel(20, 20),
            &Rgba([0, 0, 255, 255]),
            "the later shape wins"
        );
    }

    fn painted_pixels(image: &RgbaImage, reference: &RgbaImage) -> usize {
        image
            .pixels()
            .zip(reference.pixels())
            .filter(|(a, b)| a != b)
            .count()
    }

    #[test]
    fn text_marks_the_image() {
        let base = canvas(200, 60);
        let mut document = Document::new();
        document.push(Shape::Text {
            rect: Rect::new(10.0, 10.0, 180.0, 30.0),
            content: "merhaba".into(),
            color: Color::BLACK,
            outline: Color::TRANSPARENT,
            size: 24.0,
            bold: false,
            italic: false,
        });

        let out = render(&base, &document);
        assert!(painted_pixels(&out, &base) > 20, "text should be visible");
    }

    #[test]
    fn empty_text_draws_nothing() {
        let base = canvas(80, 40);
        let mut document = Document::new();
        document.push(Shape::Text {
            rect: Rect::new(5.0, 5.0, 70.0, 20.0),
            content: String::new(),
            color: Color::BLACK,
            outline: Color::WHITE,
            size: 16.0,
            bold: false,
            italic: false,
        });

        assert_eq!(render(&base, &document), base);
    }

    #[test]
    fn an_outline_makes_text_cover_more_pixels() {
        let base = canvas(220, 70);

        let mut plain = Document::new();
        plain.push(Shape::Text {
            rect: Rect::new(10.0, 10.0, 200.0, 40.0),
            content: "merhaba".into(),
            color: Color::WHITE,
            outline: Color::TRANSPARENT,
            size: 28.0,
            bold: false,
            italic: false,
        });

        let mut outlined = Document::new();
        outlined.push(Shape::Text {
            rect: Rect::new(10.0, 10.0, 200.0, 40.0),
            content: "merhaba".into(),
            color: Color::WHITE,
            outline: Color::BLACK,
            size: 28.0,
            bold: false,
            italic: false,
        });

        // White text on white is invisible without an outline, which is exactly
        // why the outline exists.
        let without = painted_pixels(&render(&base, &plain), &base);
        let with = painted_pixels(&render(&base, &outlined), &base);
        assert!(with > without, "the outline must add coverage");
    }

    #[test]
    fn a_step_badge_draws_its_number_inside_the_circle() {
        let base = canvas(80, 80);
        let mut document = Document::new();
        document.push(Shape::Step {
            center: Point::new(40.0, 40.0),
            radius: 20.0,
            number: 7,
            fill: Color::rgb(255, 0, 0),
            text_color: Color::WHITE,
        });

        let out = render(&base, &document);

        // The badge is red, so any white pixel inside it is the numeral.
        let white_inside = (25..55)
            .flat_map(|y| (25..55).map(move |x| (x, y)))
            .filter(|(x, y)| out.get_pixel(*x, *y) == &Rgba([255, 255, 255, 255]))
            .count();
        assert!(white_inside > 0, "the number should be drawn on the badge");
    }
}
