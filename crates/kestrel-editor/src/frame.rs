//! Document-level presentation: cropping, padding, rounded corners, shadow and
//! background. ShareX splits these between the crop tool and the "image
//! beautifier"; they are one thing here because they all change the *size* of
//! the output rather than drawing onto it.
//!
//! This is why annotations and the frame are separate concepts. A shape lives
//! in the base image's coordinate space and does not move when the frame
//! changes; the frame is applied afterwards, to the already-annotated image.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::shape::{Color, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Background {
    #[default]
    Transparent,
    Solid {
        color: Color,
    },
    /// Linear gradient, `angle` in degrees clockwise from left-to-right.
    Gradient {
        from: Color,
        to: Color,
        angle: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub color: Color,
    pub blur: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            color: Color::rgba(0, 0, 0, 110),
            blur: 24.0,
            offset_x: 0.0,
            offset_y: 12.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    /// Region of the base image to keep, in base image pixels.
    pub crop: Option<Rect>,
    pub padding: f32,
    pub corner_radius: f32,
    pub shadow: Option<Shadow>,
    pub background: Background,
}

impl Frame {
    pub fn is_identity(&self) -> bool {
        self.crop.is_none()
            && self.padding <= 0.0
            && self.corner_radius <= 0.0
            && self.shadow.is_none()
            && self.background == Background::Transparent
    }

    /// Size of the output for a given source size.
    pub fn output_size(&self, source: (u32, u32)) -> (u32, u32) {
        let (w, h) = self
            .crop
            .map(|c| (c.width.max(1.0) as u32, c.height.max(1.0) as u32))
            .unwrap_or(source);
        let pad = (self.padding.max(0.0) * 2.0) as u32;
        (w + pad, h + pad)
    }
}

/// Apply the frame to an already-annotated image.
pub fn apply(image: &RgbaImage, frame: &Frame) -> RgbaImage {
    if frame.is_identity() {
        return image.clone();
    }

    let content = match frame.crop {
        Some(rect) => crop(image, rect),
        None => image.clone(),
    };
    let content = round_corners(content, frame.corner_radius);

    let pad = frame.padding.max(0.0).round() as u32;
    let width = content.width() + pad * 2;
    let height = content.height() + pad * 2;
    let mut canvas = RgbaImage::new(width, height);

    paint_background(&mut canvas, frame.background);

    if let Some(shadow) = frame.shadow {
        // The shadow is the content's own silhouette, so rounded corners and
        // any transparency in the capture are respected rather than a plain
        // rectangle being drawn behind it.
        draw_shadow(&mut canvas, &content, shadow, pad as f32);
    }

    overlay(&mut canvas, &content, pad as i64, pad as i64);
    canvas
}

fn crop(image: &RgbaImage, rect: Rect) -> RgbaImage {
    let x = rect.x.max(0.0) as u32;
    let y = rect.y.max(0.0) as u32;
    let width = (rect.width.max(1.0) as u32).min(image.width().saturating_sub(x));
    let height = (rect.height.max(1.0) as u32).min(image.height().saturating_sub(y));

    if width == 0 || height == 0 {
        // A crop entirely outside the image would otherwise produce a
        // zero-sized export, which no encoder accepts.
        return image.clone();
    }
    image::imageops::crop_imm(image, x, y, width, height).to_image()
}

/// Knock the corners out of the alpha channel.
fn round_corners(mut image: RgbaImage, radius: f32) -> RgbaImage {
    if radius <= 0.0 {
        return image;
    }
    let radius = radius
        .min(image.width() as f32 / 2.0)
        .min(image.height() as f32 / 2.0);
    let (w, h) = (image.width() as f32, image.height() as f32);

    for y in 0..image.height() {
        for x in 0..image.width() {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // Distance outside the rounded rectangle, for anti-aliasing.
            let dx = (radius - px).max(px - (w - radius)).max(0.0);
            let dy = (radius - py).max(py - (h - radius)).max(0.0);
            if dx <= 0.0 || dy <= 0.0 {
                continue;
            }

            let distance = (dx * dx + dy * dy).sqrt() - radius;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            let pixel = image.get_pixel_mut(x, y);
            pixel[3] = (pixel[3] as f32 * coverage).round() as u8;
        }
    }
    image
}

fn paint_background(canvas: &mut RgbaImage, background: Background) {
    match background {
        Background::Transparent => {}
        Background::Solid { color } => {
            let rgba = Rgba([color.r, color.g, color.b, color.a]);
            for pixel in canvas.pixels_mut() {
                *pixel = rgba;
            }
        }
        Background::Gradient { from, to, angle } => {
            let radians = angle.to_radians();
            let (dx, dy) = (radians.cos(), radians.sin());
            let (w, h) = (canvas.width() as f32, canvas.height() as f32);

            // Project each pixel onto the gradient axis and normalise, so the
            // ramp spans the whole canvas at any angle.
            let extent = (w * dx).abs() + (h * dy).abs();
            let extent = if extent <= f32::EPSILON { 1.0 } else { extent };
            let origin_x = if dx < 0.0 { w } else { 0.0 };
            let origin_y = if dy < 0.0 { h } else { 0.0 };

            for y in 0..canvas.height() {
                for x in 0..canvas.width() {
                    let t = (((x as f32 - origin_x) * dx + (y as f32 - origin_y) * dy) / extent)
                        .clamp(0.0, 1.0);
                    canvas.put_pixel(x, y, Rgba(lerp_color(from, to, t)));
                }
            }
        }
    }
}

fn lerp_color(from: Color, to: Color, t: f32) -> [u8; 4] {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    [
        mix(from.r, to.r),
        mix(from.g, to.g),
        mix(from.b, to.b),
        mix(from.a, to.a),
    ]
}

/// Blur the content's alpha silhouette and composite it under the content.
fn draw_shadow(canvas: &mut RgbaImage, content: &RgbaImage, shadow: Shadow, pad: f32) {
    let radius = shadow.blur.max(0.0).round() as i64;
    let (w, h) = (content.width() as i64, content.height() as i64);

    // Alpha mask of the content, blurred with a separable box blur — two
    // passes approximate a Gaussian closely enough for a drop shadow and cost
    // a fraction of a true one.
    let mut mask: Vec<f32> = content.pixels().map(|p| p[3] as f32 / 255.0).collect();
    if radius > 0 {
        mask = box_blur(&mask, w as usize, h as usize, radius as usize);
        mask = box_blur(&mask, w as usize, h as usize, radius as usize);
    }

    let offset_x = pad + shadow.offset_x;
    let offset_y = pad + shadow.offset_y;

    for y in 0..h {
        for x in 0..w {
            let coverage = mask[(y * w + x) as usize];
            if coverage <= 0.002 {
                continue;
            }
            let dx = (x as f32 + offset_x).round() as i64;
            let dy = (y as f32 + offset_y).round() as i64;
            if dx < 0 || dy < 0 || dx >= canvas.width() as i64 || dy >= canvas.height() as i64 {
                continue;
            }

            let alpha = (shadow.color.a as f32 / 255.0) * coverage;
            let pixel = canvas.get_pixel_mut(dx as u32, dy as u32);
            let mix =
                |src: u8, dst: u8| (src as f32 * alpha + dst as f32 * (1.0 - alpha)).round() as u8;
            pixel[0] = mix(shadow.color.r, pixel[0]);
            pixel[1] = mix(shadow.color.g, pixel[1]);
            pixel[2] = mix(shadow.color.b, pixel[2]);
            pixel[3] = pixel[3].max((alpha * 255.0).round() as u8);
        }
    }
}

/// Separable box blur over a single-channel buffer.
fn box_blur(source: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || width == 0 || height == 0 {
        return source.to_vec();
    }
    let mut horizontal = vec![0.0f32; source.len()];

    for y in 0..height {
        for x in 0..width {
            let start = x.saturating_sub(radius);
            let end = (x + radius).min(width - 1);
            let mut sum = 0.0;
            for sx in start..=end {
                sum += source[y * width + sx];
            }
            horizontal[y * width + x] = sum / (end - start + 1) as f32;
        }
    }

    let mut out = vec![0.0f32; source.len()];
    for x in 0..width {
        for y in 0..height {
            let start = y.saturating_sub(radius);
            let end = (y + radius).min(height - 1);
            let mut sum = 0.0;
            for sy in start..=end {
                sum += horizontal[sy * width + x];
            }
            out[y * width + x] = sum / (end - start + 1) as f32;
        }
    }
    out
}

/// Source-over composite of `src` onto `dst`, respecting the source's alpha.
fn overlay(dst: &mut RgbaImage, src: &RgbaImage, offset_x: i64, offset_y: i64) {
    for (sx, sy, pixel) in src.enumerate_pixels() {
        let dx = offset_x + sx as i64;
        let dy = offset_y + sy as i64;
        if dx < 0 || dy < 0 || dx >= dst.width() as i64 || dy >= dst.height() as i64 {
            continue;
        }
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            continue;
        }

        let target = dst.get_pixel_mut(dx as u32, dy as u32);
        let mix =
            |src: u8, dst: u8| (src as f32 * alpha + dst as f32 * (1.0 - alpha)).round() as u8;
        target[0] = mix(pixel[0], target[0]);
        target[1] = mix(pixel[1], target[1]);
        target[2] = mix(pixel[2], target[2]);
        target[3] = target[3].max(pixel[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(color))
    }

    #[test]
    fn an_identity_frame_returns_the_image_unchanged() {
        let image = solid(10, 10, [1, 2, 3, 255]);
        let frame = Frame::default();

        assert!(frame.is_identity());
        assert_eq!(apply(&image, &frame), image);
    }

    #[test]
    fn cropping_keeps_only_the_requested_region() {
        let mut image = solid(20, 20, [0, 0, 0, 255]);
        image.put_pixel(12, 12, Rgba([255, 0, 0, 255]));

        let frame = Frame {
            crop: Some(Rect::new(10.0, 10.0, 5.0, 5.0)),
            ..Default::default()
        };
        let out = apply(&image, &frame);

        assert_eq!(out.dimensions(), (5, 5));
        assert_eq!(out.get_pixel(2, 2), &Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn a_crop_reaching_past_the_edge_is_clipped() {
        let image = solid(10, 10, [9, 9, 9, 255]);
        let frame = Frame {
            crop: Some(Rect::new(6.0, 6.0, 50.0, 50.0)),
            ..Default::default()
        };

        assert_eq!(apply(&image, &frame).dimensions(), (4, 4));
    }

    #[test]
    fn a_crop_entirely_off_image_falls_back_rather_than_producing_nothing() {
        // A zero-sized image cannot be encoded, so this must never happen.
        let image = solid(10, 10, [9, 9, 9, 255]);
        let frame = Frame {
            crop: Some(Rect::new(500.0, 500.0, 10.0, 10.0)),
            ..Default::default()
        };

        let out = apply(&image, &frame);
        assert!(out.width() > 0 && out.height() > 0);
    }

    #[test]
    fn padding_grows_the_canvas_on_every_side() {
        let image = solid(10, 10, [0, 0, 255, 255]);
        let frame = Frame {
            padding: 8.0,
            background: Background::Solid {
                color: Color::rgb(255, 255, 255),
            },
            ..Default::default()
        };

        let out = apply(&image, &frame);

        assert_eq!(out.dimensions(), (26, 26));
        assert_eq!(out.get_pixel(0, 0), &Rgba([255, 255, 255, 255]), "padding");
        assert_eq!(out.get_pixel(13, 13), &Rgba([0, 0, 255, 255]), "content");
    }

    #[test]
    fn output_size_is_predicted_before_rendering() {
        let frame = Frame {
            crop: Some(Rect::new(0.0, 0.0, 40.0, 30.0)),
            padding: 10.0,
            ..Default::default()
        };
        assert_eq!(frame.output_size((100, 100)), (60, 50));

        let image = solid(100, 100, [0, 0, 0, 255]);
        assert_eq!(
            apply(&image, &frame).dimensions(),
            frame.output_size((100, 100))
        );
    }

    #[test]
    fn rounded_corners_clear_the_corner_alpha_but_not_the_middle() {
        let image = solid(40, 40, [255, 0, 0, 255]);
        let frame = Frame {
            corner_radius: 12.0,
            ..Default::default()
        };

        let out = apply(&image, &frame);

        assert_eq!(out.get_pixel(0, 0)[3], 0, "corner must be cut away");
        assert_eq!(out.get_pixel(20, 20)[3], 255, "middle must be untouched");
        assert_eq!(out.get_pixel(20, 0)[3], 255, "edge midpoint stays");
    }

    #[test]
    fn a_solid_background_fills_everything_not_covered_by_content() {
        let image = solid(4, 4, [0, 0, 0, 255]);
        let frame = Frame {
            padding: 4.0,
            background: Background::Solid {
                color: Color::rgb(10, 200, 30),
            },
            ..Default::default()
        };

        let out = apply(&image, &frame);
        assert_eq!(out.get_pixel(1, 1), &Rgba([10, 200, 30, 255]));
    }

    #[test]
    fn a_gradient_varies_along_its_axis_and_not_across_it() {
        let image = solid(2, 2, [0, 0, 0, 0]);
        let frame = Frame {
            padding: 20.0,
            background: Background::Gradient {
                from: Color::rgb(0, 0, 0),
                to: Color::rgb(255, 255, 255),
                angle: 0.0, // left to right
            },
            ..Default::default()
        };

        let out = apply(&image, &frame);
        let left = out.get_pixel(0, 5)[0];
        let right = out.get_pixel(out.width() - 1, 5)[0];
        let top = out.get_pixel(0, 0)[0];
        let bottom = out.get_pixel(0, out.height() - 1)[0];

        assert!(right > left, "the ramp should run along the axis");
        assert_eq!(top, bottom, "and be constant across it");
    }

    #[test]
    fn a_shadow_darkens_the_padding_around_the_content() {
        let image = solid(20, 20, [255, 255, 255, 255]);
        let frame = Frame {
            padding: 24.0,
            background: Background::Solid {
                color: Color::rgb(255, 255, 255),
            },
            shadow: Some(Shadow {
                color: Color::rgba(0, 0, 0, 200),
                blur: 8.0,
                offset_x: 0.0,
                offset_y: 6.0,
            }),
            ..Default::default()
        };

        let out = apply(&image, &frame);

        // Just below the content, inside the padding, must be darker than the
        // far corner, which the shadow should not reach.
        let below = out.get_pixel(out.width() / 2, 48)[0];
        let corner = out.get_pixel(1, 1)[0];
        assert!(below < corner, "expected a shadow below the content");
    }

    #[test]
    fn a_shadow_follows_rounded_corners_rather_than_the_bounding_box() {
        let image = solid(40, 40, [255, 255, 255, 255]);
        let frame = Frame {
            padding: 20.0,
            corner_radius: 16.0,
            background: Background::Solid {
                color: Color::rgb(255, 255, 255),
            },
            shadow: Some(Shadow {
                color: Color::rgba(0, 0, 0, 255),
                blur: 2.0,
                offset_x: 0.0,
                offset_y: 0.0,
            }),
            ..Default::default()
        };

        let out = apply(&image, &frame);

        // Diagonally outside the rounded corner but inside its bounding box.
        let outside_corner = out.get_pixel(21, 21)[0];
        let under_middle = out.get_pixel(out.width() / 2, out.height() / 2)[0];
        assert!(
            outside_corner > 100,
            "the shadow must be cut away with the corner, got {outside_corner}"
        );
        assert_eq!(under_middle, 255, "content still covers the middle");
    }

    #[test]
    fn frames_survive_a_json_round_trip() {
        let frame = Frame {
            crop: Some(Rect::new(1.0, 2.0, 3.0, 4.0)),
            padding: 16.0,
            corner_radius: 8.0,
            shadow: Some(Shadow::default()),
            background: Background::Gradient {
                from: Color::BLACK,
                to: Color::WHITE,
                angle: 45.0,
            },
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(serde_json::from_str::<Frame>(&json).unwrap(), frame);
    }

    #[test]
    fn an_older_document_without_a_frame_still_loads() {
        let frame: Frame = serde_json::from_str("{}").unwrap();
        assert!(frame.is_identity());
    }
}
