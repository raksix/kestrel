//! Glues the platform capture backend to the after-capture pipeline.
//!
//! This is the Rust half of the flow described in `docs/00-PLAN.md` §3.1.
//! Only the tasks needed for the phase 1 slice are wired up so far; the rest
//! of `AfterCaptureTask` lands with the editor and uploader work.

use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::Local;
use image::{ImageFormat as ImageIoFormat, RgbaImage};
use kestrel_capture::{Capture, Region};
use kestrel_core::{
    model::{AfterCaptureTask, ImageFormat, TaskSettings},
    name_pattern::{self, NameContext},
};
use serde::Serialize;

/// The maximum edge length of the thumbnail handed back to the UI. Sending a
/// full 6K screenshot through IPC as base64 would stall the webview.
const PREVIEW_MAX_EDGE: u32 = 480;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOutput {
    pub path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub region: Region,
    /// A small PNG data URL for immediate display in the post-capture card.
    pub preview: String,
    pub copied_to_clipboard: bool,
    pub window_title: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Capture(#[from] kestrel_capture::CaptureError),
    #[error("could not determine an output directory")]
    NoOutputDirectory,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image encoding failed: {0}")]
    Encode(#[from] image::ImageError),
    #[error("clipboard error: {0}")]
    Clipboard(String),
}

pub type Result<T> = std::result::Result<T, ServiceError>;

/// Run the enabled after-capture tasks over a fresh capture.
pub fn process(capture: Capture, settings: &TaskSettings) -> Result<CaptureOutput> {
    let width = capture.width();
    let height = capture.height();
    let preview = encode_preview(&capture.image)?;

    let mut copied = false;
    if settings
        .after_capture
        .contains(&AfterCaptureTask::CopyImageToClipboard)
    {
        // A clipboard failure must not lose the capture — record it and move on.
        match copy_to_clipboard(&capture.image) {
            Ok(()) => copied = true,
            Err(err) => tracing::warn!(%err, "could not copy capture to the clipboard"),
        }
    }

    let mut path = None;
    if settings
        .after_capture
        .contains(&AfterCaptureTask::SaveImageToFile)
    {
        let saved = save(&capture, settings)?;
        path = Some(saved.to_string_lossy().into_owned());
    }

    Ok(CaptureOutput {
        path,
        width,
        height,
        region: capture.region,
        preview,
        copied_to_clipboard: copied,
        window_title: capture.window_title.clone(),
    })
}

/// Expand the workflow's filename pattern and write the image to disk.
fn save(capture: &Capture, settings: &TaskSettings) -> Result<PathBuf> {
    let now = Local::now();
    let ctx = NameContext {
        datetime: now,
        window_title: capture.window_title.clone(),
        app_name: capture.app_name.clone(),
        width: Some(capture.width()),
        height: Some(capture.height()),
        ..Default::default()
    };

    let stem = {
        let expanded = name_pattern::expand_sanitized(&settings.filename_pattern, &ctx);
        if expanded.trim().is_empty() {
            // Never write a nameless file — fall back to a timestamp.
            now.format("%Y-%m-%d_%H-%M-%S").to_string()
        } else {
            expanded
        }
    };

    let dir = output_directory(settings, now)?;
    std::fs::create_dir_all(&dir)?;

    let path = unique_path(&dir, &stem, settings.image_format.extension());
    write_image(
        &capture.image,
        &path,
        settings.image_format,
        settings.quality,
    )?;
    tracing::info!(path = %path.display(), "capture saved");
    Ok(path)
}

fn output_directory(settings: &TaskSettings, now: chrono::DateTime<Local>) -> Result<PathBuf> {
    let base = match &settings.output_directory {
        Some(dir) => PathBuf::from(dir),
        None => dirs::picture_dir()
            .or_else(dirs::home_dir)
            .ok_or(ServiceError::NoOutputDirectory)?
            .join("Kestrel"),
    };
    Ok(base
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string()))
}

/// Append ` (2)`, ` (3)`… rather than silently overwriting an existing file.
fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{extension}"));
    if !candidate.exists() {
        return candidate;
    }
    for n in 2..10_000 {
        let candidate = dir.join(format!("{stem} ({n}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!(
        "{stem}-{}.{extension}",
        Local::now().timestamp_millis()
    ))
}

fn write_image(image: &RgbaImage, path: &Path, format: ImageFormat, quality: u8) -> Result<()> {
    match format {
        // JPEG has no alpha channel, so drop it explicitly instead of letting
        // the encoder reinterpret the bytes.
        ImageFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
            let mut file = std::fs::File::create(path)?;
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut file,
                quality.clamp(1, 100),
            );
            encoder.encode_image(&image::DynamicImage::ImageRgb8(rgb))?;
            Ok(())
        }
        other => {
            let io_format = match other {
                ImageFormat::Png => ImageIoFormat::Png,
                ImageFormat::Webp => ImageIoFormat::WebP,
                ImageFormat::Gif => ImageIoFormat::Gif,
                ImageFormat::Bmp => ImageIoFormat::Bmp,
                ImageFormat::Tiff => ImageIoFormat::Tiff,
                ImageFormat::Jpeg => unreachable!("handled above"),
            };
            image.save_with_format(path, io_format)?;
            Ok(())
        }
    }
}

fn copy_to_clipboard(image: &RgbaImage) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| ServiceError::Clipboard(e.to_string()))?;
    let data = arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: std::borrow::Cow::Borrowed(image.as_raw()),
    };
    clipboard
        .set_image(data)
        .map_err(|e| ServiceError::Clipboard(e.to_string()))
}

pub fn encode_preview(image: &RgbaImage) -> Result<String> {
    let (w, h) = (image.width().max(1), image.height().max(1));
    let scale = (PREVIEW_MAX_EDGE as f32 / w.max(h) as f32).min(1.0);
    let thumb = if scale < 1.0 {
        image::imageops::thumbnail(
            image,
            ((w as f32 * scale) as u32).max(1),
            ((h as f32 * scale) as u32).max(1),
        )
    } else {
        image.clone()
    };

    let mut buffer = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut buffer, ImageIoFormat::Png)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());
    Ok(format!("data:image/png;base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([12, 34, 56, 255]))
    }

    #[test]
    fn preview_is_a_png_data_url() {
        let preview = encode_preview(&solid(32, 32)).unwrap();
        assert!(preview.starts_with("data:image/png;base64,"));
        assert!(preview.len() > "data:image/png;base64,".len());
    }

    #[test]
    fn preview_downscales_large_captures() {
        // A 6K-wide capture must not be shipped through IPC at full size.
        let preview = encode_preview(&solid(6016, 3384)).unwrap();
        let small = encode_preview(&solid(64, 36)).unwrap();
        assert!(
            preview.len() < 400_000,
            "preview should be a thumbnail, got {} bytes",
            preview.len()
        );
        assert!(small.len() < preview.len());
    }

    #[test]
    fn unique_path_avoids_clobbering_an_existing_file() {
        let dir = std::env::temp_dir().join(format!(
            "kestrel-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let first = unique_path(&dir, "shot", "png");
        assert_eq!(first.file_name().unwrap(), "shot.png");
        std::fs::write(&first, b"x").unwrap();

        let second = unique_path(&dir, "shot", "png");
        assert_eq!(second.file_name().unwrap(), "shot (2).png");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_jpeg_drops_the_alpha_channel_without_erroring() {
        let dir = std::env::temp_dir().join("kestrel-jpeg-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.jpg");

        write_image(&solid(8, 8), &path, ImageFormat::Jpeg, 85).unwrap();

        let decoded = image::open(&path).unwrap();
        assert_eq!(decoded.width(), 8);
        std::fs::remove_dir_all(&dir).ok();
    }
}
