//! Domain model. Deliberately mirrors ShareX's vocabulary so that a user
//! migrating from ShareX finds the same concepts under the same names.

use serde::{Deserialize, Serialize};

/// How a capture is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    Fullscreen,
    ActiveWindow,
    ActiveMonitor,
    WindowMenu,
    MonitorMenu,
    Region,
    RegionLight,
    RegionTransparent,
    LastRegion,
    CustomRegion,
    ScreenRecording,
    ScreenRecordingGif,
    ScrollingCapture,
    AutoCapture,
}

impl CaptureMethod {
    pub fn is_recording(self) -> bool {
        matches!(
            self,
            CaptureMethod::ScreenRecording | CaptureMethod::ScreenRecordingGif
        )
    }
}

/// Post-capture pipeline steps, in the order ShareX runs them.
/// The pipeline executes the enabled variants in this declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterCaptureTask {
    ShowQuickTaskMenu,
    ShowAfterCaptureWindow,
    BeautifyImage,
    AddImageEffects,
    OpenInEditor,
    CopyImageToClipboard,
    PinToScreen,
    PrintImage,
    SaveImageToFile,
    SaveImageToFileAs,
    SaveThumbnailImageToFile,
    PerformActions,
    CopyFileToClipboard,
    CopyFilePathToClipboard,
    CopyFolderPathToClipboard,
    ShowInFileManager,
    AnalyzeImage,
    ScanQrCode,
    RecognizeText,
    ShowBeforeUploadWindow,
    UploadImageToHost,
    DeleteFileLocally,
}

/// Post-upload pipeline steps, in ShareX's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterUploadTask {
    ShowAfterUploadWindow,
    ShortenUrl,
    ShareUrl,
    CopyUrlToClipboard,
    OpenUrl,
    ShowQrCode,
}

/// What a destination can accept. Mirrors ShareX's `DestinationType` flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationKinds {
    pub image: bool,
    pub text: bool,
    pub file: bool,
    pub url_shortener: bool,
    pub url_sharing: bool,
}

impl DestinationKinds {
    pub const IMAGE: Self = Self {
        image: true,
        text: false,
        file: false,
        url_shortener: false,
        url_sharing: false,
    };
}

/// Per-workflow settings. A workflow inherits the app defaults and overrides
/// only what it sets — the same model ShareX uses for task settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSettings {
    pub after_capture: Vec<AfterCaptureTask>,
    pub after_upload: Vec<AfterUploadTask>,
    /// ShareX-style filename pattern, e.g. `%y-%mo-%d_%h-%mi-%s`.
    pub filename_pattern: String,
    pub output_directory: Option<String>,
    pub image_format: ImageFormat,
    /// JPEG/WebP quality, 1..=100. Ignored for lossless formats.
    pub quality: u8,
    pub destination_image: Option<String>,
    pub destination_text: Option<String>,
    pub destination_file: Option<String>,
    pub destination_url_shortener: Option<String>,
}

impl Default for TaskSettings {
    fn default() -> Self {
        Self {
            after_capture: vec![
                AfterCaptureTask::CopyImageToClipboard,
                AfterCaptureTask::SaveImageToFile,
            ],
            after_upload: vec![AfterUploadTask::CopyUrlToClipboard],
            filename_pattern: "%y-%mo-%d_%h-%mi-%s".to_string(),
            output_directory: None,
            image_format: ImageFormat::Png,
            quality: 90,
            destination_image: None,
            destination_text: None,
            destination_file: None,
            destination_url_shortener: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Tiff,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
        }
    }

    pub fn is_lossy(self) -> bool {
        matches!(self, ImageFormat::Jpeg | ImageFormat::Webp)
    }
}

/// A named capture recipe bound to an optional global shortcut.
/// This is ShareX's single best idea and Kestrel's primary UI object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    /// Accelerator in Tauri syntax, e.g. `CmdOrCtrl+Shift+2`.
    pub shortcut: Option<String>,
    pub method: CaptureMethod,
    pub settings: TaskSettings,
    pub enabled: bool,
}

impl Workflow {
    pub fn new(id: impl Into<String>, name: impl Into<String>, method: CaptureMethod) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            shortcut: None,
            method,
            settings: TaskSettings::default(),
            enabled: true,
        }
    }

    pub fn with_shortcut(mut self, accelerator: impl Into<String>) -> Self {
        self.shortcut = Some(accelerator.into());
        self
    }
}

/// The default workflow set a fresh install ships with.
pub fn default_workflows() -> Vec<Workflow> {
    // Shortcut choice matters: macOS reserves Cmd+Shift+3/4/5/6 for its own
    // screenshot tools and consumes them before any application sees them, so
    // binding those would give the user dead keys. See `SYSTEM_RESERVED`.
    vec![
        Workflow::new("capture-region", "Bölge yakala", CaptureMethod::Region)
            .with_shortcut("CmdOrCtrl+Shift+2"),
        Workflow::new("capture-fullscreen", "Tüm ekran", CaptureMethod::Fullscreen)
            .with_shortcut("CmdOrCtrl+Shift+1"),
        // Opens the picker. ShareX calls this "window menu"; it is the one
        // people actually reach for.
        Workflow::new("capture-window", "Pencere seç", CaptureMethod::WindowMenu)
            .with_shortcut("CmdOrCtrl+Shift+7"),
        // No picker: grabs whatever is in front right now.
        Workflow::new(
            "capture-active-window",
            "Aktif pencere",
            CaptureMethod::ActiveWindow,
        )
        .with_shortcut("CmdOrCtrl+Shift+8"),
        Workflow::new("capture-monitor", "Ekran seç", CaptureMethod::MonitorMenu)
            .with_shortcut("CmdOrCtrl+Shift+9"),
        Workflow::new(
            "record-screen",
            "Ekran kaydı",
            CaptureMethod::ScreenRecording,
        )
        .with_shortcut("CmdOrCtrl+Shift+0"),
    ]
}

/// Alternates to try when a workflow's default shortcut cannot be registered.
///
/// Whether a combination is free depends on the machine — another application
/// may already hold it, and there is no way to know until registration fails.
/// Shipping a default that turns out to be dead is worse than quietly landing
/// on the next one and showing the user what it became.
pub fn fallback_shortcuts(workflow_id: &str) -> &'static [&'static str] {
    match workflow_id {
        "capture-region" => &["CmdOrCtrl+Shift+2", "CmdOrCtrl+Alt+2", "CmdOrCtrl+Shift+A"],
        "capture-fullscreen" => &["CmdOrCtrl+Shift+1", "CmdOrCtrl+Alt+1", "CmdOrCtrl+Shift+F"],
        "capture-window" => &["CmdOrCtrl+Shift+7", "CmdOrCtrl+Alt+W", "CmdOrCtrl+Shift+W"],
        "capture-active-window" => &["CmdOrCtrl+Shift+8", "CmdOrCtrl+Alt+8"],
        "capture-monitor" => &["CmdOrCtrl+Shift+9", "CmdOrCtrl+Alt+9"],
        "record-screen" => &["CmdOrCtrl+Shift+0", "CmdOrCtrl+Alt+0", "CmdOrCtrl+Shift+R"],
        _ => &[],
    }
}

/// Accelerators the operating system claims for itself.
///
/// These *register* successfully — Carbon happily hands out the binding — but
/// the OS consumes the key press first, so the app is never told. There is no
/// API to detect this, so the list is maintained by hand and surfaced as a
/// warning rather than silently leaving the user with a dead shortcut.
pub fn system_reserved(accelerator: &str) -> Option<&'static str> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    match accelerator {
        "CmdOrCtrl+Shift+3" => Some("macOS: tüm ekranın görüntüsünü al"),
        "CmdOrCtrl+Shift+4" => Some("macOS: seçilen bölgenin görüntüsünü al"),
        "CmdOrCtrl+Shift+5" => Some("macOS: ekran görüntüsü ve kayıt penceresi"),
        "CmdOrCtrl+Shift+6" => Some("macOS: Touch Bar görüntüsü"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_order_matches_declaration_order() {
        let mut tasks = vec![
            AfterCaptureTask::UploadImageToHost,
            AfterCaptureTask::OpenInEditor,
            AfterCaptureTask::CopyImageToClipboard,
        ];
        tasks.sort();
        assert_eq!(
            tasks,
            vec![
                AfterCaptureTask::OpenInEditor,
                AfterCaptureTask::CopyImageToClipboard,
                AfterCaptureTask::UploadImageToHost,
            ]
        );
    }

    #[test]
    fn every_workflow_has_fallback_shortcuts_starting_with_its_default() {
        for workflow in default_workflows() {
            let fallbacks = fallback_shortcuts(&workflow.id);
            assert!(
                !fallbacks.is_empty(),
                "{} has no alternates, so a taken shortcut leaves it dead",
                workflow.id
            );
            assert_eq!(
                Some(fallbacks[0]),
                workflow.shortcut.as_deref(),
                "{}'s first candidate must be the shipped default",
                workflow.id
            );
        }
    }

    #[test]
    fn no_fallback_shortcut_collides_with_the_operating_system() {
        // Falling back onto a combination macOS swallows would trade one dead
        // shortcut for another.
        for workflow in default_workflows() {
            for accelerator in fallback_shortcuts(&workflow.id) {
                assert_eq!(
                    system_reserved(accelerator),
                    None,
                    "{} may fall back to {accelerator}, which the OS owns",
                    workflow.id
                );
            }
        }
    }

    #[test]
    fn fallback_lists_do_not_overlap_between_workflows() {
        // Two workflows racing for the same alternate would make the outcome
        // depend on iteration order.
        let workflows = default_workflows();
        for (i, a) in workflows.iter().enumerate() {
            for b in workflows.iter().skip(i + 1) {
                let shared: Vec<&str> = fallback_shortcuts(&a.id)
                    .iter()
                    .filter(|x| fallback_shortcuts(&b.id).contains(x))
                    .copied()
                    .collect();
                assert!(
                    shared.is_empty(),
                    "{} and {} both list {shared:?}",
                    a.id,
                    b.id
                );
            }
        }
    }

    #[test]
    fn no_default_shortcut_collides_with_the_operating_system() {
        for workflow in default_workflows() {
            let Some(accelerator) = workflow.shortcut.as_deref() else {
                continue;
            };
            assert_eq!(
                system_reserved(accelerator),
                None,
                "{} is bound to {accelerator}, which the OS swallows",
                workflow.id
            );
        }
    }

    #[test]
    fn default_workflows_have_unique_ids_and_shortcuts() {
        let workflows = default_workflows();
        let mut ids: Vec<&str> = workflows.iter().map(|w| w.id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "workflow ids must be unique");

        let mut keys: Vec<&str> = workflows
            .iter()
            .filter_map(|w| w.shortcut.as_deref())
            .collect();
        keys.sort_unstable();
        let count = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), count, "shortcuts must not collide");
    }

    #[test]
    fn task_settings_round_trip_through_json() {
        let settings = TaskSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let back: TaskSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.filename_pattern, settings.filename_pattern);
        assert_eq!(back.after_capture, settings.after_capture);
    }
}
