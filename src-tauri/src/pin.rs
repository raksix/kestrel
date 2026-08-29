//! Pin to screen.
//!
//! A capture floated above everything else, so it can be referred to while
//! working in another application. ShareX's version, with the same keys.
//!
//! Each pin is its own window: they stack, and closing one must not disturb the
//! others. The image is staged on disk and loaded over the asset protocol for
//! the same reason the editor's is — a full-resolution data URL is megabytes of
//! base64 through IPC.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const PIN_PREFIX: &str = "pin-";

/// Pinned windows need distinct labels, and reusing one would replace an
/// existing pin instead of adding to it.
static NEXT_PIN: AtomicU32 = AtomicU32::new(1);

/// How large a pin may be before it is scaled down to open.
///
/// A full-screen capture pinned at 1:1 would cover the screen it is meant to
/// float above, which is the opposite of useful.
const MAX_INITIAL_EDGE: f64 = 640.0;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pinned {
    pub label: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("there is nothing to pin yet")]
    NothingToPin,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image encoding failed: {0}")]
    Encode(#[from] image::ImageError),
    #[error("could not open the pin window: {0}")]
    Window(#[from] tauri::Error),
}

pub type Result<T> = std::result::Result<T, PinError>;

/// Float `image` above everything else.
pub fn pin(app: &AppHandle, image: RgbaImage) -> Result<Pinned> {
    let (width, height) = (image.width(), image.height());
    let index = NEXT_PIN.fetch_add(1, Ordering::Relaxed);
    let label = format!("{PIN_PREFIX}{index}");
    let staged = stage(app, &image, index)?;

    // Fit within a sensible size, keeping the aspect ratio.
    let scale = (MAX_INITIAL_EDGE / width.max(height) as f64).min(1.0);
    let window_width = (width as f64 * scale).max(64.0);
    let window_height = (height as f64 * scale).max(64.0);

    let url = WebviewUrl::App(
        format!(
            "index.html?view=pin&path={}&w={width}&h={height}",
            urlencode(&staged.to_string_lossy())
        )
        .into(),
    );

    let handle = app.clone();
    let built_label = label.clone();
    app.run_on_main_thread(move || {
        let built = WebviewWindowBuilder::new(&handle, &built_label, url)
            .title("Kestrel")
            .inner_size(window_width, window_height)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(true)
            .visible(true)
            .build();

        if let Err(err) = built {
            tracing::error!(%err, "could not open the pin window");
        }
    })?;

    Ok(Pinned {
        label,
        path: staged.to_string_lossy().into_owned(),
        width,
        height,
    })
}

/// Pin the most recent capture.
pub fn pin_last(app: &AppHandle) -> Result<Pinned> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or(PinError::NothingToPin)?;
    pin(app, image)
}

/// Close a pin and remove its staged file.
pub fn close(app: &AppHandle, label: &str) {
    // Only ever act on a pin label; a stray call must not be able to close the
    // main window or an overlay.
    if !label.starts_with(PIN_PREFIX) {
        tracing::warn!(label, "refusing to close a window that is not a pin");
        return;
    }

    if let Some(path) = staged_path(app, label) {
        let _ = std::fs::remove_file(path);
    }

    let handle = app.clone();
    let label = label.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(&label) {
            let _ = window.close();
        }
    });
}

fn pins_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("pins")
}

fn staged_path(app: &AppHandle, label: &str) -> Option<PathBuf> {
    let index = label.strip_prefix(PIN_PREFIX)?;
    Some(pins_dir(app).join(format!("{index}.png")))
}

fn stage(app: &AppHandle, image: &RgbaImage, index: u32) -> Result<PathBuf> {
    let dir = pins_dir(app);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{index}.png"));
    image.save(&path)?;
    Ok(path)
}

/// Percent-encode a path for use in a query string.
///
/// A capture folder can contain spaces, `#`, or non-ASCII characters, any of
/// which would truncate or corrupt the URL if passed through raw.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_with_awkward_characters_survive_encoding() {
        assert_eq!(
            urlencode("/Users/a b/Kestrel #1/ö.png"),
            "/Users/a%20b/Kestrel%20%231/%C3%B6.png"
        );
    }

    #[test]
    fn encoding_leaves_a_plain_path_alone() {
        let path = "/Users/me/Pictures/Kestrel/2026/08/shot.png";
        assert_eq!(urlencode(path), path);
    }

    #[test]
    fn pin_labels_are_unique() {
        // Reusing a label would replace an existing pin rather than add one.
        let first = NEXT_PIN.fetch_add(1, Ordering::Relaxed);
        let second = NEXT_PIN.fetch_add(1, Ordering::Relaxed);
        assert_ne!(first, second);
    }

    #[test]
    fn only_pin_labels_map_to_a_staged_file() {
        assert!(PIN_PREFIX.ends_with('-'));
        assert!("main".strip_prefix(PIN_PREFIX).is_none());
        assert_eq!("pin-7".strip_prefix(PIN_PREFIX), Some("7"));
    }
}
