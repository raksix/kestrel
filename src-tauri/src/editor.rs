//! Editor sessions.
//!
//! Ownership is split deliberately. The webview owns the *live* document while
//! the user drags shapes around, because a round trip to Rust per mouse move
//! would be visibly laggy. Rust owns the base image and performs the final
//! render, because that is the only way an export looks identical on WebKitGTK,
//! WebView2 and WKWebView — and the only way it can exceed screen resolution.
//!
//! The base image reaches the canvas as a file on disk served over Tauri's
//! asset protocol rather than as a data URL. A 3420x2224 screenshot is roughly
//! eight megabytes of base64, and pushing that through IPC stalls the webview
//! for long enough to notice.

use std::path::PathBuf;
use std::sync::Mutex;

use image::RgbaImage;
use kestrel_editor::Document;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const EDITOR_LABEL: &str = "editor";

/// The capture most recently produced, kept so the editor can be opened after
/// the fact — from the tray, the post-capture card, or a workflow's
/// "open in editor" task.
#[derive(Default)]
pub struct LastCapture(pub Mutex<Option<RgbaImage>>);

impl LastCapture {
    pub fn set(&self, image: RgbaImage) {
        *self.0.lock().expect("last capture mutex poisoned") = Some(image);
    }

    pub fn get(&self) -> Option<RgbaImage> {
        self.0.lock().expect("last capture mutex poisoned").clone()
    }
}

/// The image an open editor window is working on.
#[derive(Default)]
pub struct EditorState(pub Mutex<Option<EditorSession>>);

pub struct EditorSession {
    pub base: RgbaImage,
    /// Where the base image was staged for the webview to load.
    pub staged: PathBuf,
}

impl EditorState {
    fn set(&self, session: EditorSession) {
        *self.0.lock().expect("editor mutex poisoned") = Some(session);
    }

    fn base(&self) -> Option<RgbaImage> {
        self.0
            .lock()
            .expect("editor mutex poisoned")
            .as_ref()
            .map(|s| s.base.clone())
    }

    fn clear(&self) -> Option<EditorSession> {
        self.0.lock().expect("editor mutex poisoned").take()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpened {
    /// Absolute path to the staged PNG, for `convertFileSrc`.
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error("there is no capture to edit yet")]
    NothingToEdit,
    #[error("no editor session is open")]
    NoSession,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image encoding failed: {0}")]
    Encode(#[from] image::ImageError),
    #[error("could not open the editor window: {0}")]
    Window(#[from] tauri::Error),
    #[error("the annotation document is not valid: {0}")]
    Document(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, EditorError>;

/// Stage `image` on disk and raise the editor window over it.
pub fn open(app: &AppHandle, image: RgbaImage) -> Result<EditorOpened> {
    let (width, height) = (image.width(), image.height());
    let staged = stage(app, &image)?;

    app.state::<EditorState>().set(EditorSession {
        base: image,
        staged: staged.clone(),
    });

    if let Some(existing) = app.get_webview_window(EDITOR_LABEL) {
        // Reuse the window; the frontend reloads the session on focus.
        let _ = existing.set_focus();
    } else {
        WebviewWindowBuilder::new(
            app,
            EDITOR_LABEL,
            WebviewUrl::App("index.html?view=editor".into()),
        )
        .title("Kestrel — düzenle")
        .inner_size(1100.0, 760.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .resizable(true)
        .build()?;
    }

    Ok(EditorOpened {
        path: staged.to_string_lossy().into_owned(),
        width,
        height,
    })
}

/// Open the editor on the most recent capture.
pub fn open_last(app: &AppHandle) -> Result<EditorOpened> {
    let image = app
        .state::<LastCapture>()
        .get()
        .ok_or(EditorError::NothingToEdit)?;
    open(app, image)
}

/// Details of the session the editor window should load.
pub fn session(app: &AppHandle) -> Result<EditorOpened> {
    let guard = app.state::<EditorState>();
    let guard = guard.0.lock().expect("editor mutex poisoned");
    let session = guard.as_ref().ok_or(EditorError::NoSession)?;

    Ok(EditorOpened {
        path: session.staged.to_string_lossy().into_owned(),
        width: session.base.width(),
        height: session.base.height(),
    })
}

/// Flatten the annotations onto the base image.
///
/// `document_json` comes straight from the canvas, so it is parsed rather than
/// trusted: a malformed document must fail loudly here instead of producing a
/// silently wrong export.
pub fn render(app: &AppHandle, document_json: &str) -> Result<RgbaImage> {
    let base = app
        .state::<EditorState>()
        .base()
        .ok_or(EditorError::NoSession)?;
    let document: Document = serde_json::from_str(document_json)?;
    Ok(kestrel_editor::render(&base, &document))
}

pub fn close(app: &AppHandle) {
    // Drop the staged file; it is scratch space, not the user's output.
    if let Some(session) = app.state::<EditorState>().clear() {
        let _ = std::fs::remove_file(session.staged);
    }
    if let Some(window) = app.get_webview_window(EDITOR_LABEL) {
        let _ = window.close();
    }
}

/// Write the base image somewhere the webview is allowed to read from.
fn stage(app: &AppHandle, image: &RgbaImage) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("editor");
    std::fs::create_dir_all(&dir)?;

    // A fixed name keeps the cache directory from growing without bound; the
    // previous session's file is no longer needed once a new one starts.
    let path = dir.join("session.png");
    image.save(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn last_capture_starts_empty_and_remembers_what_it_is_given() {
        let last = LastCapture::default();
        assert!(last.get().is_none());

        last.set(RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255])));

        let image = last.get().expect("should remember");
        assert_eq!(image.dimensions(), (4, 4));
    }

    #[test]
    fn a_later_capture_replaces_the_earlier_one() {
        let last = LastCapture::default();
        last.set(RgbaImage::new(2, 2));
        last.set(RgbaImage::new(8, 8));

        assert_eq!(last.get().unwrap().dimensions(), (8, 8));
    }

    #[test]
    fn clearing_an_editor_state_yields_the_session_once() {
        let state = EditorState::default();
        state.set(EditorSession {
            base: RgbaImage::new(2, 2),
            staged: PathBuf::from("/tmp/does-not-exist.png"),
        });

        assert!(state.clear().is_some());
        assert!(
            state.clear().is_none(),
            "clearing twice must not resurrect it"
        );
    }

    #[test]
    fn a_malformed_document_is_rejected_rather_than_rendered() {
        // The canvas is untrusted input; parsing must fail loudly.
        let parsed: std::result::Result<Document, _> = serde_json::from_str("{\"shapes\": 5}");
        assert!(parsed.is_err());
    }
}
