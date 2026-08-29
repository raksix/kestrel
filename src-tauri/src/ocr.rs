//! OCR: model installation and the recogniser's lifetime.
//!
//! Two things live here that `kestrel-tools` deliberately does not do.
//!
//! The first is downloading. The models are about 20 MB and most people never
//! run OCR, so bundling them would put that in every download. They are fetched
//! on demand instead — which means the app must *say* it is about to use the
//! network rather than quietly reaching out.
//!
//! The second is keeping the engine alive. Loading the two networks takes long
//! enough to notice, so the first OCR pays for it and the rest do not.

use std::path::PathBuf;
use std::sync::Mutex;

use image::RgbaImage;
use kestrel_tools::ocr::{Engine, Models, Recognised};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Where the models are published.
///
/// These are the upstream `ocrs-models` releases — the same files the ocrs CLI
/// downloads. Kestrel does not mirror them, so what is fetched is what upstream
/// publishes.
const DETECTION_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const RECOGNITION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// A refusal to write a file that is obviously not a model.
///
/// A captive-portal login page or an S3 error document is a few kilobytes of
/// HTML; writing it under a `.rten` name would turn a network problem into a
/// confusing "corrupt model" error later.
const MIN_MODEL_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum OcrShellError {
    #[error("{0}")]
    Ocr(#[from] kestrel_tools::ocr::OcrError),
    #[error("could not reach the model download: {0}")]
    Network(String),
    #[error("the download returned {status} instead of a model")]
    BadStatus { status: u16 },
    #[error(
        "the downloaded file is only {got} bytes, which is not a model — check the connection"
    )]
    TooSmall { got: usize },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("there is no capture to read text from yet")]
    NothingToRead,
}

pub type Result<T> = std::result::Result<T, OcrShellError>;

/// The loaded engine, kept between calls.
#[derive(Default)]
pub struct OcrState(Mutex<Option<Engine>>);

impl OcrState {
    /// Drop the loaded engine, so the next call reloads from disk.
    ///
    /// Called after installing models: an engine loaded before the download is
    /// one that failed, and keeping it would make a successful install look
    /// like it had not happened.
    fn invalidate(&self) {
        *self.0.lock().expect("ocr mutex poisoned") = None;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub installed: bool,
    pub directory: String,
    /// Roughly what the download costs, so the UI can say so before starting.
    pub download_size_mb: u32,
}

fn model_directory(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("ocr-models")
}

fn models(app: &AppHandle) -> Models {
    Models::in_directory(model_directory(app))
}

pub fn status(app: &AppHandle) -> ModelStatus {
    let directory = model_directory(app);
    ModelStatus {
        installed: Models::in_directory(&directory).present(),
        directory: directory.to_string_lossy().into_owned(),
        download_size_mb: 20,
    }
}

/// Download whichever models are missing.
///
/// Only the missing ones: re-fetching a file that is already there wastes
/// someone's bandwidth for nothing.
pub async fn install(app: &AppHandle) -> Result<ModelStatus> {
    let models = models(app);
    if let Some(parent) = models.detection.parent() {
        std::fs::create_dir_all(parent)?;
    }

    for (url, path) in [
        (DETECTION_URL, &models.detection),
        (RECOGNITION_URL, &models.recognition),
    ] {
        if path.is_file() {
            continue;
        }
        let bytes = fetch(url).await?;

        // Write to a temporary name and rename into place, so an interrupted
        // download cannot leave a half file that `present()` reports as ready.
        let partial = path.with_extension("rten.part");
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, path)?;

        tracing::info!(path = %path.display(), bytes = bytes.len(), "installed OCR model");
    }

    app.state::<OcrState>().invalidate();
    Ok(status(app))
}

async fn fetch(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| OcrShellError::Network(e.to_string()))?;

    if !response.status().is_success() {
        return Err(OcrShellError::BadStatus {
            status: response.status().as_u16(),
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| OcrShellError::Network(e.to_string()))?;

    check_size(bytes.len())?;
    Ok(bytes.to_vec())
}

/// Refuse a response that is too small to be a model.
fn check_size(len: usize) -> Result<()> {
    if len < MIN_MODEL_BYTES {
        return Err(OcrShellError::TooSmall { got: len });
    }
    Ok(())
}

/// Read the text in an image, loading the engine on first use.
pub fn read(app: &AppHandle, image: &RgbaImage) -> Result<Recognised> {
    let state = app.state::<OcrState>();
    let mut guard = state.0.lock().expect("ocr mutex poisoned");

    if guard.is_none() {
        *guard = Some(Engine::load(&models(app))?);
    }

    let engine = guard.as_ref().expect("just loaded");
    Ok(engine.read(image)?)
}

/// Read the most recent capture and, if it is in the history, make the text
/// searchable.
pub fn read_last(app: &AppHandle) -> Result<Recognised> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or(OcrShellError::NothingToRead)?;

    let recognised = read(app, &image)?;

    // Attaching the text to the history entry is what makes a screenshot
    // findable later by what it said.
    //
    // A failure here is logged rather than returned: the user asked to read the
    // text, and they have it. Losing the search index is not worth turning a
    // successful read into an error.
    if !recognised.text.is_empty() {
        if let Some(id) = app.state::<crate::history::LastEntryId>().get() {
            let history = app.state::<crate::history::History>();
            if let Err(err) = history.record_ocr(id, &recognised.text) {
                tracing::warn!(%err, "could not attach the recognised text to the history");
            }
        }
    }

    Ok(recognised)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invalidated_state_reloads_next_time() {
        let state = OcrState::default();
        state.invalidate();
        assert!(state.0.lock().unwrap().is_none());
    }

    #[test]
    fn a_tiny_response_is_rejected_as_not_a_model() {
        // A captive portal login page or an S3 error document is a few
        // kilobytes of HTML. Writing that under a `.rten` name would turn a
        // network problem into a confusing "corrupt model" error later.
        let page = "<html><body>Sign in to continue</body></html>".len();

        assert!(matches!(
            check_size(page),
            Err(OcrShellError::TooSmall { got }) if got == page
        ));
        assert!(check_size(MIN_MODEL_BYTES).is_ok());
    }

    #[test]
    fn the_model_urls_are_https() {
        // These are fetched without asking again once the user has agreed, so
        // they must not be downgradeable.
        assert!(DETECTION_URL.starts_with("https://"));
        assert!(RECOGNITION_URL.starts_with("https://"));
    }
}
