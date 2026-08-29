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
use kestrel_editor::{Chain, Document};
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
    /// The capture as it arrived, never modified.
    ///
    /// The effect chain is always applied to this rather than to the previous
    /// result, so removing an effect restores exactly what was there before.
    /// Applying effects in place would make the chain one-way: you could add a
    /// blur but never get the sharp pixels back.
    pub original: RgbaImage,
    /// `original` with the current effect chain applied — what the canvas shows
    /// and what the export renders onto.
    pub base: RgbaImage,
    pub effects: Chain,
    /// Where the base image was staged for the webview to load.
    pub staged: PathBuf,
    /// Bumped whenever `staged` is rewritten, so the webview can bust its cache.
    pub revision: u32,
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
    /// Changes every time the staged file is rewritten.
    pub revision: u32,
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
    #[error("resize, rotate, flip, crop and border move the image, which would leave the existing annotations pointing at the wrong pixels — apply them before annotating")]
    GeometryWouldMoveAnnotations,
}

pub type Result<T> = std::result::Result<T, EditorError>;

/// Stage `image` on disk and raise the editor window over it.
pub fn open(app: &AppHandle, image: RgbaImage) -> Result<EditorOpened> {
    let (width, height) = (image.width(), image.height());
    let staged = stage(app, &image)?;

    app.state::<EditorState>().set(EditorSession {
        original: image.clone(),
        base: image,
        effects: Chain::default(),
        staged: staged.clone(),
        revision: 0,
    });

    // macOS creates windows on the main thread only, and Tauri's builder called
    // from a worker blocks until the event loop wakes. Queue it instead.
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(existing) = handle.get_webview_window(EDITOR_LABEL) {
            // Reuse the window; the frontend reloads the session on focus.
            let _ = existing.set_focus();
            return;
        }

        let built = WebviewWindowBuilder::new(
            &handle,
            EDITOR_LABEL,
            WebviewUrl::App("index.html?view=editor".into()),
        )
        .title("Kestrel — düzenle")
        .inner_size(1100.0, 760.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .resizable(true)
        .build();

        if let Err(err) = built {
            tracing::error!(%err, "could not open the editor window");
        }
    })?;

    Ok(EditorOpened {
        path: staged.to_string_lossy().into_owned(),
        width,
        height,
        revision: 0,
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
        revision: session.revision,
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

/// Replace the effect chain and restage the result.
///
/// The chain is applied to the untouched original every time, so removing an
/// effect genuinely undoes it.
///
/// `annotation_count` comes from the canvas because the webview owns the live
/// document. It matters: annotations are stored in base-image coordinates, so a
/// rotate or a crop would leave every existing arrow pointing at the wrong
/// pixels. Rather than silently misplace them, geometry-changing effects are
/// refused while annotations exist and the caller is told why.
pub fn apply_effects(
    app: &AppHandle,
    effects: Chain,
    annotation_count: usize,
) -> Result<EditorOpened> {
    if annotation_count > 0 && effects.changes_geometry() {
        return Err(EditorError::GeometryWouldMoveAnnotations);
    }

    let state = app.state::<EditorState>();
    let mut guard = state.0.lock().expect("editor mutex poisoned");
    let session = guard.as_mut().ok_or(EditorError::NoSession)?;

    let base = effects.apply(&session.original);
    let (width, height) = (base.width(), base.height());

    let staged = session.staged.clone();
    base.save(&staged)?;

    session.base = base;
    session.effects = effects;
    // The path never changes, so the webview would happily serve the previous
    // image from cache. The revision gives the frontend something to bust it
    // with.
    session.revision += 1;

    Ok(EditorOpened {
        path: staged.to_string_lossy().into_owned(),
        width,
        height,
        revision: session.revision,
    })
}

pub fn close(app: &AppHandle) {
    // Drop the staged file; it is scratch space, not the user's output.
    if let Some(session) = app.state::<EditorState>().clear() {
        let _ = std::fs::remove_file(session.staged);
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(EDITOR_LABEL) {
            let _ = window.close();
        }
    });
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
            original: RgbaImage::new(2, 2),
            base: RgbaImage::new(2, 2),
            effects: Chain::default(),
            staged: PathBuf::from("/tmp/does-not-exist.png"),
            revision: 0,
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
