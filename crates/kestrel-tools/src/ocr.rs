//! Reading text out of a capture, as ShareX's OCR.
//!
//! ShareX uses the Windows OCR API. There is no equivalent that exists on all
//! three platforms, so Kestrel carries its own recogniser — `ocrs`, which runs
//! two small neural networks locally. Nothing is sent anywhere.
//!
//! The models are not bundled. They are roughly 20 MB, they are not needed by
//! anyone who never runs OCR, and shipping them would put that weight in every
//! download. So this module works with model files it is handed and reports
//! plainly when they are missing — fetching them is the shell's job, and asking
//! first is the shell's job too.

use std::path::{Path, PathBuf};

use image::RgbaImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;
use serde::Serialize;

/// Where the two model files live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    pub detection: PathBuf,
    pub recognition: PathBuf,
}

impl Models {
    /// The layout Kestrel uses inside its data directory.
    pub fn in_directory(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            detection: dir.join("text-detection.rten"),
            recognition: dir.join("text-recognition.rten"),
        }
    }

    /// Whether both files are present.
    ///
    /// This is a check for existence, not for validity — a truncated download
    /// passes here and fails at load, which is where the error belongs.
    pub fn present(&self) -> bool {
        self.detection.is_file() && self.recognition.is_file()
    }

    /// The files that are not there yet, for a message that names them.
    pub fn missing(&self) -> Vec<&Path> {
        [self.detection.as_path(), self.recognition.as_path()]
            .into_iter()
            .filter(|path| !path.is_file())
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("the OCR models are not installed: {0}")]
    ModelsMissing(String),
    #[error("could not load the OCR model {path}: {source}")]
    ModelLoad {
        path: String,
        #[source]
        source: rten::ModelLoadError,
    },
    #[error("the image could not be read: {0}")]
    Image(String),
    #[error("text recognition failed: {0}")]
    Recognise(String),
}

pub type Result<T> = std::result::Result<T, OcrError>;

/// One recognised line, with where it was found.
///
/// The position is what makes the result useful for more than copying: it lets
/// the UI point at the text on the image, and it is what a "select the text
/// under here" gesture would need.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recognised {
    /// Every line joined by newlines, in reading order — what "copy text" puts
    /// on the clipboard.
    pub text: String,
    pub lines: Vec<Line>,
}

impl Recognised {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// A loaded recogniser.
///
/// Loading the models takes a noticeable moment and a fair amount of memory, so
/// this is worth keeping around rather than rebuilding per capture.
pub struct Engine {
    inner: OcrEngine,
}

// `OcrEngine` holds two loaded networks and does not implement Debug; there is
// nothing useful to print about them anyway.
impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engine(ocrs)")
    }
}

impl Engine {
    pub fn load(models: &Models) -> Result<Self> {
        let missing = models.missing();
        if !missing.is_empty() {
            return Err(OcrError::ModelsMissing(
                missing
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        let detection = load_model(&models.detection)?;
        let recognition = load_model(&models.recognition)?;

        let inner = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection),
            recognition_model: Some(recognition),
            ..Default::default()
        })
        .map_err(|e| OcrError::Recognise(e.to_string()))?;

        Ok(Self { inner })
    }

    /// Read the text in `image`.
    ///
    /// An image with no text is not an error — it returns an empty result, the
    /// same as ShareX showing an empty OCR window.
    pub fn read(&self, image: &RgbaImage) -> Result<Recognised> {
        let source = ImageSource::from_bytes(image.as_raw(), image.dimensions())
            .map_err(|e| OcrError::Image(e.to_string()))?;
        let input = self
            .inner
            .prepare_input(source)
            .map_err(|e| OcrError::Image(e.to_string()))?;

        let words = self
            .inner
            .detect_words(&input)
            .map_err(|e| OcrError::Recognise(e.to_string()))?;
        let line_rects = self.inner.find_text_lines(&input, &words);
        let recognised = self
            .inner
            .recognize_text(&input, &line_rects)
            .map_err(|e| OcrError::Recognise(e.to_string()))?;

        let lines: Vec<Line> = recognised
            .into_iter()
            .flatten()
            .filter_map(|line| {
                let text = line.to_string();
                // The recogniser emits blank lines where it found a shape it
                // could not read. They carry nothing, and pasting them would
                // put stray newlines in the middle of the copied text.
                if text.trim().is_empty() {
                    return None;
                }
                let rect = line.bounding_rect();
                Some(Line {
                    text,
                    x: rect.left(),
                    y: rect.top(),
                    width: rect.width().max(0) as u32,
                    height: rect.height().max(0) as u32,
                })
            })
            .collect();

        Ok(Recognised {
            text: lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            lines,
        })
    }
}

fn load_model(path: &Path) -> Result<Model> {
    Model::load_file(path).map_err(|source| OcrError::ModelLoad {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        // Per-test names: a shared directory means parallel tests delete each
        // other's fixtures.
        let dir = std::env::temp_dir().join(format!("kestrel-ocr-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn models_are_looked_for_under_predictable_names() {
        let models = Models::in_directory("/models");

        assert!(models.detection.ends_with("text-detection.rten"));
        assert!(models.recognition.ends_with("text-recognition.rten"));
    }

    #[test]
    fn an_empty_directory_has_both_models_missing() {
        let dir = temp_dir("empty");
        let models = Models::in_directory(&dir);

        assert!(!models.present());
        assert_eq!(models.missing().len(), 2);
    }

    #[test]
    fn a_half_finished_install_is_reported_as_incomplete() {
        // A partial download is the likely failure, and treating it as "ready"
        // would surface as a confusing load error later.
        let dir = temp_dir("half");
        let models = Models::in_directory(&dir);
        std::fs::write(&models.detection, b"not a real model").unwrap();

        assert!(!models.present());
        assert_eq!(models.missing(), [models.recognition.as_path()]);
    }

    #[test]
    fn loading_without_the_models_names_what_is_missing() {
        let dir = temp_dir("names");
        let models = Models::in_directory(&dir);

        let error = Engine::load(&models).unwrap_err();
        let message = error.to_string();

        assert!(matches!(error, OcrError::ModelsMissing(_)));
        assert!(
            message.contains("text-detection.rten"),
            "the message should name the file: {message}"
        );
    }

    #[test]
    fn a_corrupt_model_fails_at_load_rather_than_being_treated_as_absent() {
        // Existence and validity are different questions; conflating them would
        // tell someone to download files they already have.
        let dir = temp_dir("corrupt");
        let models = Models::in_directory(&dir);
        std::fs::write(&models.detection, b"garbage").unwrap();
        std::fs::write(&models.recognition, b"garbage").unwrap();

        assert!(models.present());
        assert!(matches!(
            Engine::load(&models).unwrap_err(),
            OcrError::ModelLoad { .. }
        ));
    }

    #[test]
    fn an_empty_result_serialises_as_empty_rather_than_null() {
        let json = serde_json::to_string(&Recognised::default()).unwrap();

        assert!(Recognised::default().is_empty());
        assert_eq!(json, r#"{"text":"","lines":[]}"#);
    }
}
