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
    /// Record a dragged region rather than a whole display.
    RegionRecording,
    RegionRecordingGif,
    ScrollingCapture,
    AutoCapture,
}

impl CaptureMethod {
    pub fn is_recording(self) -> bool {
        matches!(
            self,
            CaptureMethod::ScreenRecording
                | CaptureMethod::ScreenRecordingGif
                | CaptureMethod::RegionRecording
                | CaptureMethod::RegionRecordingGif
        )
    }

    /// Whether the method asks the user to drag a region before it runs.
    ///
    /// Recording a region and screenshotting one share the same overlay, so the
    /// two families have to be distinguishable without listing the variants at
    /// every call site.
    pub fn needs_region_selection(self) -> bool {
        matches!(
            self,
            CaptureMethod::Region
                | CaptureMethod::RegionLight
                | CaptureMethod::RegionTransparent
                | CaptureMethod::RegionRecording
                | CaptureMethod::RegionRecordingGif
        )
    }

    /// Whether a recording method writes an animated GIF instead of a video.
    pub fn is_gif(self) -> bool {
        matches!(
            self,
            CaptureMethod::ScreenRecordingGif | CaptureMethod::RegionRecordingGif
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

impl AfterCaptureTask {
    /// Every task, in the order ShareX runs them.
    ///
    /// The order is the pipeline, not a menu ordering: "save to file" has to
    /// happen before "copy the file path", and "upload" before anything that
    /// touches the resulting URL.
    pub const ALL: [AfterCaptureTask; 22] = [
        AfterCaptureTask::ShowQuickTaskMenu,
        AfterCaptureTask::ShowAfterCaptureWindow,
        AfterCaptureTask::BeautifyImage,
        AfterCaptureTask::AddImageEffects,
        AfterCaptureTask::OpenInEditor,
        AfterCaptureTask::CopyImageToClipboard,
        AfterCaptureTask::PinToScreen,
        AfterCaptureTask::PrintImage,
        AfterCaptureTask::SaveImageToFile,
        AfterCaptureTask::SaveImageToFileAs,
        AfterCaptureTask::SaveThumbnailImageToFile,
        AfterCaptureTask::PerformActions,
        AfterCaptureTask::CopyFileToClipboard,
        AfterCaptureTask::CopyFilePathToClipboard,
        AfterCaptureTask::CopyFolderPathToClipboard,
        AfterCaptureTask::ShowInFileManager,
        AfterCaptureTask::AnalyzeImage,
        AfterCaptureTask::ScanQrCode,
        AfterCaptureTask::RecognizeText,
        AfterCaptureTask::ShowBeforeUploadWindow,
        AfterCaptureTask::UploadImageToHost,
        AfterCaptureTask::DeleteFileLocally,
    ];

    /// A stable identifier for settings files and IPC.
    pub fn id(self) -> &'static str {
        match self {
            AfterCaptureTask::ShowQuickTaskMenu => "show_quick_task_menu",
            AfterCaptureTask::ShowAfterCaptureWindow => "show_after_capture_window",
            AfterCaptureTask::BeautifyImage => "beautify_image",
            AfterCaptureTask::AddImageEffects => "add_image_effects",
            AfterCaptureTask::OpenInEditor => "open_in_editor",
            AfterCaptureTask::CopyImageToClipboard => "copy_image_to_clipboard",
            AfterCaptureTask::PinToScreen => "pin_to_screen",
            AfterCaptureTask::PrintImage => "print_image",
            AfterCaptureTask::SaveImageToFile => "save_image_to_file",
            AfterCaptureTask::SaveImageToFileAs => "save_image_to_file_as",
            AfterCaptureTask::SaveThumbnailImageToFile => "save_thumbnail_image_to_file",
            AfterCaptureTask::PerformActions => "perform_actions",
            AfterCaptureTask::CopyFileToClipboard => "copy_file_to_clipboard",
            AfterCaptureTask::CopyFilePathToClipboard => "copy_file_path_to_clipboard",
            AfterCaptureTask::CopyFolderPathToClipboard => "copy_folder_path_to_clipboard",
            AfterCaptureTask::ShowInFileManager => "show_in_file_manager",
            AfterCaptureTask::AnalyzeImage => "analyze_image",
            AfterCaptureTask::ScanQrCode => "scan_qr_code",
            AfterCaptureTask::RecognizeText => "recognize_text",
            AfterCaptureTask::ShowBeforeUploadWindow => "show_before_upload_window",
            AfterCaptureTask::UploadImageToHost => "upload_image_to_host",
            AfterCaptureTask::DeleteFileLocally => "delete_file_locally",
        }
    }

    /// Whether Kestrel actually performs this task yet.
    ///
    /// This is reported to the UI so an unimplemented task can be shown greyed
    /// out with a reason, rather than being selectable and then quietly doing
    /// nothing — which is the worst of the three options, because the user has
    /// no way to tell the difference between "did not run" and "ran and had no
    /// effect".
    ///
    /// Update this in the same commit that implements the task, never before.
    pub fn implemented(self) -> bool {
        matches!(
            self,
            AfterCaptureTask::OpenInEditor
                | AfterCaptureTask::CopyImageToClipboard
                | AfterCaptureTask::PinToScreen
                | AfterCaptureTask::SaveImageToFile
                | AfterCaptureTask::SaveThumbnailImageToFile
                | AfterCaptureTask::CopyFilePathToClipboard
                | AfterCaptureTask::CopyFolderPathToClipboard
                | AfterCaptureTask::ShowInFileManager
                | AfterCaptureTask::ScanQrCode
                | AfterCaptureTask::RecognizeText
                | AfterCaptureTask::UploadImageToHost
                | AfterCaptureTask::DeleteFileLocally
        )
    }

    /// Tasks that need a file on disk, and so are pointless without
    /// `SaveImageToFile` earlier in the chain.
    pub fn needs_saved_file(self) -> bool {
        matches!(
            self,
            AfterCaptureTask::SaveThumbnailImageToFile
                | AfterCaptureTask::CopyFileToClipboard
                | AfterCaptureTask::CopyFilePathToClipboard
                | AfterCaptureTask::CopyFolderPathToClipboard
                | AfterCaptureTask::ShowInFileManager
                | AfterCaptureTask::DeleteFileLocally
        )
    }
}

impl AfterUploadTask {
    pub const ALL: [AfterUploadTask; 6] = [
        AfterUploadTask::ShowAfterUploadWindow,
        AfterUploadTask::ShortenUrl,
        AfterUploadTask::ShareUrl,
        AfterUploadTask::CopyUrlToClipboard,
        AfterUploadTask::OpenUrl,
        AfterUploadTask::ShowQrCode,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AfterUploadTask::ShowAfterUploadWindow => "show_after_upload_window",
            AfterUploadTask::ShortenUrl => "shorten_url",
            AfterUploadTask::ShareUrl => "share_url",
            AfterUploadTask::CopyUrlToClipboard => "copy_url_to_clipboard",
            AfterUploadTask::OpenUrl => "open_url",
            AfterUploadTask::ShowQrCode => "show_qr_code",
        }
    }

    pub fn implemented(self) -> bool {
        matches!(
            self,
            AfterUploadTask::CopyUrlToClipboard | AfterUploadTask::OpenUrl
        )
    }
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
        // Recording a region is the recording people actually want most of the
        // time — a whole 5K display is an enormous file to share a dialog box.
        Workflow::new(
            "record-region",
            "Bölge kaydı",
            CaptureMethod::RegionRecording,
        )
        .with_shortcut("CmdOrCtrl+Alt+R"),
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
        "record-region" => &["CmdOrCtrl+Alt+R", "CmdOrCtrl+Alt+V", "CmdOrCtrl+Shift+V"],
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

    #[test]
    fn every_task_appears_in_all_exactly_once() {
        // ALL drives the settings UI. A variant missing from it is a task the
        // user can never switch on, and a duplicate is one that appears twice.
        let mut ids: Vec<&str> = AfterCaptureTask::ALL.iter().map(|t| t.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), count, "duplicate task in ALL");
        assert_eq!(count, 22, "ShareX has 22 after-capture tasks");
    }

    #[test]
    fn all_is_in_the_order_the_pipeline_runs() {
        // The order is the pipeline, not a menu ordering: saving has to precede
        // copying the file path, and uploading has to precede deleting it.
        let position = |task: AfterCaptureTask| {
            AfterCaptureTask::ALL
                .iter()
                .position(|t| *t == task)
                .expect("in ALL")
        };

        assert!(
            position(AfterCaptureTask::SaveImageToFile)
                < position(AfterCaptureTask::CopyFilePathToClipboard)
        );
        assert!(
            position(AfterCaptureTask::SaveImageToFile)
                < position(AfterCaptureTask::ShowInFileManager)
        );
        assert!(
            position(AfterCaptureTask::UploadImageToHost)
                < position(AfterCaptureTask::DeleteFileLocally)
        );
    }

    #[test]
    fn every_upload_task_appears_in_all_exactly_once() {
        let mut ids: Vec<&str> = AfterUploadTask::ALL.iter().map(|t| t.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), count);
        assert_eq!(count, 6);
    }

    #[test]
    fn task_ids_match_their_serde_names() {
        // The id is what settings files and IPC carry. If it drifted from the
        // serde representation, a saved workflow would silently stop matching.
        for task in AfterCaptureTask::ALL {
            let serialised = serde_json::to_string(&task).expect("serialises");
            assert_eq!(serialised.trim_matches('"'), task.id(), "{task:?}");
        }
        for task in AfterUploadTask::ALL {
            let serialised = serde_json::to_string(&task).expect("serialises");
            assert_eq!(serialised.trim_matches('"'), task.id(), "{task:?}");
        }
    }

    #[test]
    fn the_default_tasks_are_ones_that_actually_work() {
        // Shipping a default that does nothing would make the app look broken
        // out of the box.
        let settings = TaskSettings::default();

        for task in &settings.after_capture {
            assert!(task.implemented(), "{task:?} is a default but does nothing");
        }
        for task in &settings.after_upload {
            assert!(task.implemented(), "{task:?} is a default but does nothing");
        }
    }

    #[test]
    fn tasks_that_need_a_file_are_the_ones_that_touch_a_path() {
        // Used by the UI to warn that these are pointless without "save to
        // file" earlier in the chain.
        assert!(AfterCaptureTask::CopyFilePathToClipboard.needs_saved_file());
        assert!(AfterCaptureTask::ShowInFileManager.needs_saved_file());
        assert!(AfterCaptureTask::DeleteFileLocally.needs_saved_file());

        assert!(!AfterCaptureTask::CopyImageToClipboard.needs_saved_file());
        assert!(!AfterCaptureTask::PinToScreen.needs_saved_file());
        assert!(!AfterCaptureTask::SaveImageToFile.needs_saved_file());
    }

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
