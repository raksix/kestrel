//! The image effect chain, as ShareX's "image effects".
//!
//! An ordered list of transformations applied to an image. Order matters and is
//! part of the data: blurring then adding a border is not the same picture as
//! adding a border then blurring it.
//!
//! Effects are separate from annotations because they act on the whole image
//! rather than being drawn onto part of it, and separate from the frame because
//! they mostly do not change its size.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::shape::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    None,
    Quarter,
    Half,
    ThreeQuarters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    // ── Manipulations ───────────────────────────────────────────────────
    Resize {
        width: u32,
        height: u32,
        keep_aspect: bool,
    },
    Rotate {
        rotation: Rotation,
    },
    Flip {
        horizontal: bool,
        vertical: bool,
    },
    /// Trim uniform edges, as ShareX's auto-crop.
    AutoCrop {
        /// How different a pixel may be from the corner colour and still count
        /// as background, 0–255.
        tolerance: u8,
    },

    // ── Adjustments ─────────────────────────────────────────────────────
    /// -1.0 to 1.0.
    Brightness {
        amount: f32,
    },
    /// -1.0 to 1.0.
    Contrast {
        amount: f32,
    },
    /// Above 1.0 lightens, below darkens.
    Gamma {
        value: f32,
    },
    /// -1.0 removes all colour, 1.0 doubles it.
    Saturation {
        amount: f32,
    },
    /// 0.0 to 1.0.
    Opacity {
        amount: f32,
    },

    // ── Filters ─────────────────────────────────────────────────────────
    Grayscale,
    Sepia,
    Invert,
    Blur {
        radius: f32,
    },
    Sharpen {
        amount: f32,
    },
    Pixelate {
        block: u32,
    },

    // ── Drawings ────────────────────────────────────────────────────────
    Border {
        width: u32,
        color: Color,
    },
}

impl Effect {
    /// Whether applying this effect moves pixels to different coordinates.
    ///
    /// The editor needs to know because annotations are stored in image
    /// coordinates against the base image. Recolouring the base is harmless,
    /// but rotating or cropping it would leave every existing arrow pointing
    /// somewhere else — so the editor refuses those while annotations exist
    /// rather than silently misplacing them.
    pub fn changes_geometry(&self) -> bool {
        match self {
            Effect::Resize { .. }
            | Effect::Rotate { .. }
            | Effect::Flip { .. }
            | Effect::AutoCrop { .. }
            | Effect::Border { .. } => true,

            Effect::Brightness { .. }
            | Effect::Contrast { .. }
            | Effect::Gamma { .. }
            | Effect::Saturation { .. }
            | Effect::Opacity { .. }
            | Effect::Grayscale
            | Effect::Sepia
            | Effect::Invert
            | Effect::Blur { .. }
            | Effect::Sharpen { .. }
            | Effect::Pixelate { .. } => false,
        }
    }
}

/// An ordered list of effects.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Chain(pub Vec<Effect>);

impl Chain {
    pub fn new(effects: Vec<Effect>) -> Self {
        Self(effects)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether any effect in the chain moves pixels to different coordinates.
    pub fn changes_geometry(&self) -> bool {
        self.0.iter().any(Effect::changes_geometry)
    }

    /// Apply every effect in order.
    pub fn apply(&self, image: &RgbaImage) -> RgbaImage {
        self.0
            .iter()
            .fold(image.clone(), |current, effect| apply_one(&current, effect))
    }
}

fn apply_one(image: &RgbaImage, effect: &Effect) -> RgbaImage {
    match effect {
        Effect::Resize {
            width,
            height,
            keep_aspect,
        } => resize(image, *width, *height, *keep_aspect),

        Effect::Rotate { rotation } => match rotation {
            Rotation::None => image.clone(),
            Rotation::Quarter => image::imageops::rotate90(image),
            Rotation::Half => image::imageops::rotate180(image),
            Rotation::ThreeQuarters => image::imageops::rotate270(image),
        },

        Effect::Flip {
            horizontal,
            vertical,
        } => {
            let mut out = image.clone();
            if *horizontal {
                out = image::imageops::flip_horizontal(&out);
            }
            if *vertical {
                out = image::imageops::flip_vertical(&out);
            }
            out
        }

        Effect::AutoCrop { tolerance } => auto_crop(image, *tolerance),

        Effect::Brightness { amount } => {
            let shift = (amount.clamp(-1.0, 1.0) * 255.0) as i32;
            map_channels(image, |value| (value as i32 + shift).clamp(0, 255) as u8)
        }

        Effect::Contrast { amount } => {
            // The standard contrast curve, pivoting around mid grey so the
            // image gets more contrast rather than just brighter.
            let amount = amount.clamp(-1.0, 1.0);
            let factor = (1.015 * (amount + 1.0)) / (1.0 * (1.015 - amount));
            map_channels(image, move |value| {
                (((value as f32 / 255.0 - 0.5) * factor + 0.5) * 255.0).clamp(0.0, 255.0) as u8
            })
        }

        Effect::Gamma { value } => {
            let gamma = value.max(0.01);
            map_channels(image, move |channel| {
                ((channel as f32 / 255.0).powf(1.0 / gamma) * 255.0).clamp(0.0, 255.0) as u8
            })
        }

        Effect::Saturation { amount } => saturate(image, *amount),

        Effect::Opacity { amount } => {
            let alpha = amount.clamp(0.0, 1.0);
            let mut out = image.clone();
            for pixel in out.pixels_mut() {
                pixel[3] = (pixel[3] as f32 * alpha) as u8;
            }
            out
        }

        Effect::Grayscale => {
            let mut out = image.clone();
            for pixel in out.pixels_mut() {
                let grey = luma(pixel);
                pixel[0] = grey;
                pixel[1] = grey;
                pixel[2] = grey;
            }
            out
        }

        Effect::Sepia => {
            let mut out = image.clone();
            for pixel in out.pixels_mut() {
                let (r, g, b) = (pixel[0] as f32, pixel[1] as f32, pixel[2] as f32);
                pixel[0] = (r * 0.393 + g * 0.769 + b * 0.189).min(255.0) as u8;
                pixel[1] = (r * 0.349 + g * 0.686 + b * 0.168).min(255.0) as u8;
                pixel[2] = (r * 0.272 + g * 0.534 + b * 0.131).min(255.0) as u8;
            }
            out
        }

        // Alpha is deliberately untouched: inverting it would turn a
        // transparent background opaque, which is never what "invert" means.
        Effect::Invert => map_channels(image, |value| 255 - value),

        Effect::Blur { radius } => image::imageops::blur(image, radius.max(0.1)),

        Effect::Sharpen { amount } => {
            image::imageops::unsharpen(image, 1.0, (amount.max(0.0) * 10.0) as i32)
        }

        Effect::Pixelate { block } => pixelate(image, (*block).max(2)),

        Effect::Border { width, color } => border(image, *width, *color),
    }
}

fn luma(pixel: &Rgba<u8>) -> u8 {
    (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32) as u8
}

/// Apply a function to red, green and blue, leaving alpha alone.
fn map_channels(image: &RgbaImage, f: impl Fn(u8) -> u8) -> RgbaImage {
    let mut out = image.clone();
    for pixel in out.pixels_mut() {
        pixel[0] = f(pixel[0]);
        pixel[1] = f(pixel[1]);
        pixel[2] = f(pixel[2]);
    }
    out
}

fn saturate(image: &RgbaImage, amount: f32) -> RgbaImage {
    let factor = 1.0 + amount.clamp(-1.0, 1.0);
    let mut out = image.clone();

    for pixel in out.pixels_mut() {
        let grey = luma(pixel) as f32;
        for channel in 0..3 {
            let value = pixel[channel] as f32;
            pixel[channel] = (grey + (value - grey) * factor).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn resize(image: &RgbaImage, width: u32, height: u32, keep_aspect: bool) -> RgbaImage {
    let width = width.max(1);
    let height = height.max(1);

    if !keep_aspect {
        return image::imageops::resize(image, width, height, image::imageops::Lanczos3);
    }
    let scale = (width as f32 / image.width().max(1) as f32)
        .min(height as f32 / image.height().max(1) as f32);
    image::imageops::resize(
        image,
        ((image.width() as f32 * scale) as u32).max(1),
        ((image.height() as f32 * scale) as u32).max(1),
        image::imageops::Lanczos3,
    )
}

fn pixelate(image: &RgbaImage, block: u32) -> RgbaImage {
    let mut out = image.clone();
    let mut y = 0;

    while y < image.height() {
        let mut x = 0;
        while x < image.width() {
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for py in y..(y + block).min(image.height()) {
                for px in x..(x + block).min(image.width()) {
                    let pixel = image.get_pixel(px, py);
                    r += pixel[0] as u32;
                    g += pixel[1] as u32;
                    b += pixel[2] as u32;
                    a += pixel[3] as u32;
                    n += 1;
                }
            }
            if n > 0 {
                let average = Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8]);
                for py in y..(y + block).min(image.height()) {
                    for px in x..(x + block).min(image.width()) {
                        out.put_pixel(px, py, average);
                    }
                }
            }
            x += block;
        }
        y += block;
    }
    out
}

fn border(image: &RgbaImage, width: u32, color: Color) -> RgbaImage {
    if width == 0 {
        return image.clone();
    }
    let mut out = RgbaImage::from_pixel(
        image.width() + width * 2,
        image.height() + width * 2,
        color.to_rgba(),
    );
    image::imageops::replace(&mut out, image, width as i64, width as i64);
    out
}

/// Trim edges that match the corner colour within `tolerance`.
fn auto_crop(image: &RgbaImage, tolerance: u8) -> RgbaImage {
    if image.width() == 0 || image.height() == 0 {
        return image.clone();
    }
    let background = *image.get_pixel(0, 0);
    let similar = |pixel: &Rgba<u8>| {
        (0..4).all(|channel| pixel[channel].abs_diff(background[channel]) <= tolerance)
    };

    let mut top = 0;
    let mut bottom = image.height();
    let mut left = 0;
    let mut right = image.width();

    while top < bottom && (0..image.width()).all(|x| similar(image.get_pixel(x, top))) {
        top += 1;
    }
    while bottom > top && (0..image.width()).all(|x| similar(image.get_pixel(x, bottom - 1))) {
        bottom -= 1;
    }
    while left < right && (top..bottom).all(|y| similar(image.get_pixel(left, y))) {
        left += 1;
    }
    while right > left && (top..bottom).all(|y| similar(image.get_pixel(right - 1, y))) {
        right -= 1;
    }

    // An image that is entirely background would crop to nothing, which no
    // encoder accepts; returning it unchanged is the honest answer.
    if right <= left || bottom <= top {
        return image.clone();
    }
    image::imageops::crop_imm(image, left, top, right - left, bottom - top).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_changing_effects_are_flagged_and_colour_ones_are_not() {
        // The editor gates on this to avoid leaving annotations pointing at the
        // wrong pixels, so a wrong answer here is a silently wrong export.
        assert!(Effect::Rotate {
            rotation: Rotation::Quarter
        }
        .changes_geometry());
        assert!(Effect::AutoCrop { tolerance: 0 }.changes_geometry());
        assert!(Effect::Border {
            width: 2,
            color: Color::BLACK
        }
        .changes_geometry());

        assert!(!Effect::Grayscale.changes_geometry());
        assert!(!Effect::Blur { radius: 4.0 }.changes_geometry());
        assert!(!Effect::Pixelate { block: 8 }.changes_geometry());
    }

    #[test]
    fn a_chain_changes_geometry_if_any_effect_does() {
        assert!(!Chain::new(vec![Effect::Grayscale, Effect::Sepia]).changes_geometry());
        assert!(Chain::new(vec![
            Effect::Grayscale,
            Effect::Flip {
                horizontal: true,
                vertical: false
            }
        ])
        .changes_geometry());
        assert!(!Chain::default().changes_geometry());
    }

    #[test]
    fn effects_flagged_as_safe_really_do_keep_the_size() {
        // The flag is a claim about behaviour; check it against the behaviour
        // rather than trusting the match arms to stay in step.
        let image = solid(9, 5, [120, 90, 60, 255]);
        let safe = [
            Effect::Grayscale,
            Effect::Sepia,
            Effect::Invert,
            Effect::Blur { radius: 3.0 },
            Effect::Sharpen { amount: 0.5 },
            Effect::Pixelate { block: 4 },
            Effect::Brightness { amount: 0.2 },
            Effect::Contrast { amount: 0.2 },
            Effect::Gamma { value: 1.5 },
            Effect::Saturation { amount: 0.5 },
            Effect::Opacity { amount: 0.5 },
        ];

        for effect in safe {
            assert!(!effect.changes_geometry(), "{effect:?} claims to be safe");
            let result = Chain::new(vec![effect.clone()]).apply(&image);
            assert_eq!(result.dimensions(), (9, 5), "{effect:?} changed the size");
        }
    }

    fn solid(w: u32, h: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(colour))
    }

    fn chain(effect: Effect) -> Chain {
        Chain::new(vec![effect])
    }

    #[test]
    fn an_empty_chain_changes_nothing() {
        let image = solid(8, 8, [10, 20, 30, 255]);
        assert_eq!(Chain::default().apply(&image), image);
    }

    #[test]
    fn effects_apply_in_order_and_the_order_matters() {
        // Blur-then-border and border-then-blur are different pictures; if the
        // chain were a set this test would fail.
        let image = RgbaImage::from_fn(16, 16, |x, _| {
            Rgba([if x < 8 { 0 } else { 255 }, 0, 0, 255])
        });

        let blur_then_border = Chain::new(vec![
            Effect::Blur { radius: 2.0 },
            Effect::Border {
                width: 2,
                color: Color::WHITE,
            },
        ])
        .apply(&image);

        let border_then_blur = Chain::new(vec![
            Effect::Border {
                width: 2,
                color: Color::WHITE,
            },
            Effect::Blur { radius: 2.0 },
        ])
        .apply(&image);

        assert_ne!(blur_then_border, border_then_blur);
    }

    #[test]
    fn brightness_lightens_and_darkens() {
        let image = solid(4, 4, [100, 100, 100, 255]);

        let lighter = chain(Effect::Brightness { amount: 0.2 }).apply(&image);
        let darker = chain(Effect::Brightness { amount: -0.2 }).apply(&image);

        assert!(lighter.get_pixel(0, 0)[0] > 100);
        assert!(darker.get_pixel(0, 0)[0] < 100);
    }

    #[test]
    fn brightness_saturates_rather_than_wrapping() {
        // Wrapping would turn a nearly white image black, which looks like
        // corruption rather than an adjustment.
        let image = solid(4, 4, [250, 250, 250, 255]);
        let result = chain(Effect::Brightness { amount: 1.0 }).apply(&image);
        assert_eq!(result.get_pixel(0, 0)[0], 255);

        let dark = chain(Effect::Brightness { amount: -1.0 }).apply(&solid(4, 4, [5, 5, 5, 255]));
        assert_eq!(dark.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn grayscale_makes_every_channel_equal_and_keeps_alpha() {
        let image = solid(4, 4, [200, 50, 20, 128]);
        let result = chain(Effect::Grayscale).apply(&image);
        let pixel = result.get_pixel(0, 0);

        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert_eq!(pixel[3], 128, "alpha is not a colour channel");
    }

    #[test]
    fn invert_leaves_transparency_alone() {
        // Inverting alpha would make a transparent background opaque, which is
        // never what "invert" means.
        let image = solid(4, 4, [0, 0, 0, 0]);
        let result = chain(Effect::Invert).apply(&image);

        assert_eq!(result.get_pixel(0, 0), &Rgba([255, 255, 255, 0]));
    }

    #[test]
    fn saturation_can_remove_all_colour() {
        let image = solid(4, 4, [200, 50, 20, 255]);
        let result = chain(Effect::Saturation { amount: -1.0 }).apply(&image);
        let pixel = result.get_pixel(0, 0);

        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
    }

    #[test]
    fn opacity_scales_alpha_only() {
        let image = solid(4, 4, [10, 20, 30, 200]);
        let result = chain(Effect::Opacity { amount: 0.5 }).apply(&image);
        let pixel = result.get_pixel(0, 0);

        assert_eq!(pixel[3], 100);
        assert_eq!(&pixel.0[..3], &[10, 20, 30]);
    }

    #[test]
    fn rotating_a_quarter_turn_swaps_the_dimensions() {
        let image = solid(10, 4, [0, 0, 0, 255]);
        let rotated = chain(Effect::Rotate {
            rotation: Rotation::Quarter,
        })
        .apply(&image);

        assert_eq!(rotated.dimensions(), (4, 10));
    }

    #[test]
    fn four_quarter_turns_return_the_original() {
        let image = RgbaImage::from_fn(6, 4, |x, y| Rgba([x as u8 * 20, y as u8 * 20, 0, 255]));
        let turned = Chain::new(vec![
            Effect::Rotate {
                rotation: Rotation::Quarter,
            };
            4
        ])
        .apply(&image);

        assert_eq!(turned, image);
    }

    #[test]
    fn flipping_twice_returns_the_original() {
        let image = RgbaImage::from_fn(6, 4, |x, y| Rgba([x as u8 * 20, y as u8 * 20, 0, 255]));
        let flip = Effect::Flip {
            horizontal: true,
            vertical: true,
        };
        assert_eq!(Chain::new(vec![flip.clone(), flip]).apply(&image), image);
    }

    #[test]
    fn resizing_without_keeping_the_ratio_hits_the_exact_size() {
        let image = solid(100, 50, [0, 0, 0, 255]);
        let resized = chain(Effect::Resize {
            width: 40,
            height: 40,
            keep_aspect: false,
        })
        .apply(&image);

        assert_eq!(resized.dimensions(), (40, 40));
    }

    #[test]
    fn resizing_with_the_ratio_kept_fits_inside_the_box() {
        let image = solid(100, 50, [0, 0, 0, 255]);
        let resized = chain(Effect::Resize {
            width: 40,
            height: 40,
            keep_aspect: true,
        })
        .apply(&image);

        assert_eq!(resized.dimensions(), (40, 20));
    }

    #[test]
    fn a_border_grows_the_image_on_every_side() {
        let image = solid(10, 10, [0, 0, 0, 255]);
        let bordered = chain(Effect::Border {
            width: 3,
            color: Color::rgb(255, 0, 0),
        })
        .apply(&image);

        assert_eq!(bordered.dimensions(), (16, 16));
        assert_eq!(bordered.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(bordered.get_pixel(8, 8), &Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn auto_crop_trims_a_uniform_margin() {
        let mut image = solid(20, 20, [255, 255, 255, 255]);
        for y in 5..15 {
            for x in 5..15 {
                image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }

        let cropped = chain(Effect::AutoCrop { tolerance: 0 }).apply(&image);
        assert_eq!(cropped.dimensions(), (10, 10));
    }

    #[test]
    fn auto_crop_of_a_blank_image_returns_it_unchanged() {
        // Cropping to nothing would produce a zero-sized image that no encoder
        // will write.
        let image = solid(10, 10, [255, 255, 255, 255]);
        assert_eq!(
            chain(Effect::AutoCrop { tolerance: 0 }).apply(&image),
            image
        );
    }

    #[test]
    fn pixelate_flattens_each_block() {
        let noisy = RgbaImage::from_fn(16, 16, |x, y| {
            Rgba([if (x + y) % 2 == 0 { 0 } else { 255 }, 0, 0, 255])
        });
        let result = chain(Effect::Pixelate { block: 4 }).apply(&noisy);

        let first = *result.get_pixel(0, 0);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(*result.get_pixel(x, y), first);
            }
        }
    }

    #[test]
    fn a_zero_block_size_does_not_divide_by_zero() {
        let image = solid(8, 8, [1, 2, 3, 255]);
        let result = chain(Effect::Pixelate { block: 0 }).apply(&image);
        assert_eq!(result.dimensions(), (8, 8));
    }

    #[test]
    fn a_zero_gamma_does_not_produce_infinity() {
        // 1.0 / 0.0 is infinity, and `x.powf(inf)` is 0 below 1.0 but NaN at
        // exactly 1.0 — so a white pixel is the case that would go wrong.
        // Gamma is clamped to 0.01 instead, which leaves both defined.
        let grey = chain(Effect::Gamma { value: 0.0 }).apply(&solid(4, 4, [128, 128, 128, 255]));
        assert_eq!(grey.get_pixel(0, 0), &Rgba([0, 0, 0, 255]));

        let white = chain(Effect::Gamma { value: 0.0 }).apply(&solid(4, 4, [255, 255, 255, 255]));
        assert_eq!(white.get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn chains_survive_a_json_round_trip() {
        let chain = Chain::new(vec![
            Effect::Grayscale,
            Effect::Border {
                width: 4,
                color: Color::ACCENT,
            },
            Effect::Resize {
                width: 100,
                height: 100,
                keep_aspect: true,
            },
        ]);

        let json = serde_json::to_string(&chain).unwrap();
        assert_eq!(serde_json::from_str::<Chain>(&json).unwrap(), chain);
        assert!(json.contains("\"kind\":\"grayscale\""));
    }
}
