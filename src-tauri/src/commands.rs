//! The IPC surface. This is the only API the frontend sees.
//!
//! Every command returns `Result<_, String>` because Tauri needs a
//! serialisable error; the real error types stay in the Rust crates.

use kestrel_capture::{
    permissions, Capabilities, CaptureBackend, DisplayInfo, PermissionStatus, Region, WindowInfo,
};
use kestrel_core::{
    model::{default_workflows, CaptureMethod, TaskSettings, Workflow},
    name_pattern::{self, NameContext},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::capture_service::{self, CaptureOutput};
use crate::overlay;
use crate::settings::{AppSettings, SettingsState};

fn backend() -> impl CaptureBackend {
    kestrel_capture::backend()
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Explain a permission failure in the user's terms instead of surfacing an
/// empty result. On macOS a missing permission looks like success to the API.
fn require_permission() -> Result<(), String> {
    if permissions::status().is_usable() {
        return Ok(());
    }
    Err(
        "Ekran Kaydı izni verilmemiş. Kestrel ekranı göremiyor — Ayarlar bölümünden izni ver."
            .to_string(),
    )
}

/// Run the after-capture pipeline, remember the image, and tell every window.
///
/// Every capture path goes through here so they cannot drift apart. Two things
/// have to happen beyond the pipeline itself:
///
/// - The image is kept so the editor can be opened after the fact. The clone
///   costs one memcpy of the frame, which is far cheaper than re-capturing —
///   and re-capturing would grab a screen that has since changed.
/// - The result is broadcast, because the overlay and the picker are closed
///   the moment they commit, so the value returned to *them* goes nowhere.
fn finish_capture(
    app: &AppHandle,
    capture: kestrel_capture::Capture,
    settings: &TaskSettings,
) -> Result<CaptureOutput, String> {
    let (output, image) = capture_service::process(capture, settings).map_err(err)?;

    app.state::<crate::editor::LastCapture>().set(image.clone());

    // Record the capture before anything else can fail. History is a log of
    // what happened, so an entry that exists without a URL is correct; one that
    // never appears because a later step errored is not.
    let entry = crate::history::NewEntry {
        filename: output
            .path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}x{}", output.width, output.height)),
        path: output.path.clone(),
        width: output.width,
        height: output.height,
        window_title: output.window_title.clone(),
    };
    match app
        .state::<crate::history::History>()
        .insert(&entry, chrono::Utc::now().timestamp())
    {
        Ok(id) => {
            app.state::<crate::history::LastEntryId>().set(id);
            let _ = app.emit(crate::EVENT_HISTORY_CHANGED, ());
        }
        Err(err) => tracing::warn!(%err, "could not record the capture in history"),
    }

    let _ = app.emit(crate::EVENT_CAPTURE_COMPLETE, output.clone());

    // ShareX's "open in image editor" after-capture task. Failing to raise the
    // editor must not lose the capture, which is already saved by this point.
    if settings
        .after_capture
        .contains(&kestrel_core::model::AfterCaptureTask::ScanQrCode)
    {
        let found = kestrel_tools::decode(&image);
        if !found.is_empty() {
            tracing::info!(count = found.len(), "QR codes found in the capture");
            let _ = app.emit(crate::EVENT_QR_FOUND, found);
        }
    }

    if settings
        .after_capture
        .contains(&kestrel_core::model::AfterCaptureTask::PinToScreen)
    {
        if let Err(e) = crate::pin::pin(app, image.clone()) {
            tracing::error!(%e, "could not pin the capture");
        }
    }

    if settings
        .after_capture
        .contains(&kestrel_core::model::AfterCaptureTask::OpenInEditor)
    {
        if let Err(e) = crate::editor::open(app, image.clone()) {
            tracing::error!(%e, "could not open the editor");
        }
    }

    run_file_tasks(app, &output, settings);
    run_text_tasks(app, &image, settings);
    run_upload_task(app, &image, settings);

    Ok(output)
}

/// After-capture tasks that act on the file that was just written.
///
/// Every one is skipped in silence when nothing was saved — asking to copy the
/// file path when "save to file" is not in the chain is a configuration
/// mistake, not a runtime failure, and it is already visible in the task list.
fn run_file_tasks(app: &AppHandle, output: &CaptureOutput, settings: &TaskSettings) {
    use kestrel_core::model::AfterCaptureTask as Task;

    let Some(path) = output.path.as_deref().map(std::path::Path::new) else {
        return;
    };
    let enabled = |task: Task| settings.after_capture.contains(&task);

    if enabled(Task::SaveThumbnailImageToFile) {
        if let Err(err) = save_thumbnail(app, path) {
            tracing::warn!(%err, "could not write the thumbnail");
        }
    }

    // Both path tasks write text, so the later one wins if both are on. That
    // matches ShareX and there is nothing sensible to do with two values.
    if enabled(Task::CopyFilePathToClipboard) {
        copy_text(&path.to_string_lossy());
    }
    if enabled(Task::CopyFolderPathToClipboard) {
        if let Some(parent) = path.parent() {
            copy_text(&parent.to_string_lossy());
        }
    }

    if enabled(Task::ShowInFileManager) {
        reveal(path);
    }
}

fn save_thumbnail(app: &AppHandle, path: &std::path::Path) -> Result<(), String> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or("no capture")?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let destination = path.with_file_name(format!("{stem}-thumb.png"));

    kestrel_tools::thumbnail(&image, 480, 480)
        .save(&destination)
        .map_err(err)
}

/// Put text on the clipboard, logging rather than failing.
///
/// A clipboard error must not lose a capture that is already on disk.
fn copy_text(text: &str) {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => {}
        Err(err) => tracing::warn!(%err, "could not put text on the clipboard"),
    }
}

/// Select the file in the platform's file manager.
///
/// Selecting it is the point — opening the folder alone leaves the user hunting
/// for which of two hundred screenshots is the new one. Only Linux falls back
/// to opening the directory, because there is no portable way to ask an
/// arbitrary file manager to select a file.
fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("open")
        .args(["-R"])
        .arg(path)
        .spawn();

    #[cfg(target_os = "windows")]
    let command = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let command = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();

    if let Err(err) = command {
        tracing::warn!(%err, "could not show the file in the file manager");
    }
}

/// ShareX's "recognize text": read the capture and make it searchable.
///
/// Skipped with a log line when the OCR models are not installed. Downloading
/// twenty megabytes because a checkbox is ticked, without being asked, is not
/// something a capture should do.
fn run_text_tasks(app: &AppHandle, image: &image::RgbaImage, settings: &TaskSettings) {
    if !settings
        .after_capture
        .contains(&kestrel_core::model::AfterCaptureTask::RecognizeText)
    {
        return;
    }

    let app = app.clone();
    let image = image.clone();
    std::thread::spawn(move || match crate::ocr::read(&app, &image) {
        Ok(recognised) if recognised.text.is_empty() => {}
        Ok(recognised) => {
            if let Some(id) = app.state::<crate::history::LastEntryId>().get() {
                let history = app.state::<crate::history::History>();
                if let Err(err) = history.record_ocr(id, &recognised.text) {
                    tracing::warn!(%err, "could not attach the recognised text");
                }
            }
        }
        Err(err) => tracing::info!(%err, "skipping text recognition"),
    });
}

/// ShareX's "upload image to host", and the "delete file locally" that can only
/// follow it.
///
/// Runs in the background: an upload takes as long as the network takes, and
/// blocking the capture on it would make the shortcut feel broken.
fn run_upload_task(app: &AppHandle, image: &image::RgbaImage, settings: &TaskSettings) {
    use kestrel_core::model::AfterCaptureTask as Task;

    if !settings.after_capture.contains(&Task::UploadImageToHost) {
        return;
    }

    let app = app.clone();
    let image = image.clone();
    let delete_after = settings.after_capture.contains(&Task::DeleteFileLocally);
    let destination = settings.destination_image.clone();

    tauri::async_runtime::spawn(async move {
        match upload_last_capture(app.clone(), destination).await {
            Ok(uploaded) => {
                if delete_after {
                    // Only after a successful upload. Deleting on failure would
                    // destroy the only copy of the capture.
                    delete_local_copy(&app);
                }
                tracing::info!(url = %uploaded.url, "capture uploaded");
            }
            Err(err) => tracing::error!(%err, "could not upload the capture"),
        }
        drop(image);
    });
}

fn delete_local_copy(app: &AppHandle) {
    let Some(id) = app.state::<crate::history::LastEntryId>().get() else {
        return;
    };
    let history = app.state::<crate::history::History>();

    // The history entry keeps the URL, so the capture is not lost — the row
    // stays and only the local file goes.
    match history.get(id) {
        Ok(Some(entry)) => {
            if let Some(path) = entry.path {
                if let Err(err) = std::fs::remove_file(&path) {
                    tracing::warn!(%err, path, "could not delete the local copy");
                }
            }
        }
        Ok(None) => {}
        Err(err) => tracing::warn!(%err, "could not find the entry to delete"),
    }
}

// ── Discovery ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_displays() -> Result<Vec<DisplayInfo>, String> {
    backend().displays().map_err(err)
}

#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    require_permission()?;
    backend().windows().map_err(err)
}

#[tauri::command]
pub fn platform_capabilities() -> Capabilities {
    backend().capabilities()
}

// ── Permissions ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn permission_status() -> PermissionStatus {
    permissions::status()
}

/// Trigger the OS prompt. macOS only shows it once per app, so a `false`
/// answer means the user has to go to System Settings from here.
#[tauri::command]
pub fn request_screen_permission() -> PermissionStatus {
    permissions::request();
    permissions::status()
}

#[tauri::command]
pub fn open_permission_settings() {
    permissions::open_settings();
}

// ── Capture ─────────────────────────────────────────────────────────────

fn task_settings(settings: &State<'_, SettingsState>, workflow_id: Option<&str>) -> TaskSettings {
    let snapshot = settings.snapshot();
    workflow_id
        .and_then(|id| snapshot.workflow(id).map(|w| w.settings.clone()))
        .unwrap_or(snapshot.defaults)
}

#[tauri::command]
pub fn capture_fullscreen(
    app: AppHandle,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_all_displays().map_err(err)?;
    finish_capture(&app, capture, &task_settings(&settings, None))
}

#[tauri::command]
pub fn capture_display(
    app: AppHandle,
    id: u32,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_display(id).map_err(err)?;
    finish_capture(&app, capture, &task_settings(&settings, None))
}

#[tauri::command]
pub fn capture_window(
    app: AppHandle,
    id: u32,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_window(id).map_err(err)?;
    finish_capture(&app, capture, &task_settings(&settings, None))
}

/// The front-most window, without showing a picker — ShareX's "active window".
#[tauri::command]
pub fn capture_active_window(
    app: AppHandle,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let backend = backend();
    let windows = backend.windows().map_err(err)?;
    let front = windows
        .iter()
        .find(|w| w.is_focused)
        .or_else(|| windows.first())
        .ok_or("Yakalanabilir pencere bulunamadı.")?;
    let capture = backend.capture_window(front.id).map_err(err)?;
    finish_capture(&app, capture, &task_settings(&settings, None))
}

/// A small preview of a window, for the picker grid.
#[tauri::command]
pub fn window_thumbnail(id: u32) -> Result<String, String> {
    require_permission()?;
    let capture = backend().capture_window(id).map_err(err)?;
    capture_service::encode_preview(&capture.image).map_err(err)
}

#[tauri::command]
pub fn display_thumbnail(id: u32) -> Result<String, String> {
    require_permission()?;
    let capture = backend().capture_display(id).map_err(err)?;
    capture_service::encode_preview(&capture.image).map_err(err)
}

/// One entry in the picker grid, thumbnail included.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPreview {
    pub id: u32,
    pub title: String,
    pub subtitle: String,
    pub width: u32,
    pub height: u32,
    /// `None` when this target could not be rendered (a window that closed
    /// while we were scanning, for instance).
    pub preview: Option<String>,
}

/// Everything the picker needs, from a single screen grab.
///
/// Capturing each window separately meant one real screen capture per window —
/// on a busy desktop that is thirty of them, and the picker crawled. We freeze
/// once and crop, which is a single capture plus some memory copies.
///
/// The trade-off is that a thumbnail shows whatever was on top of the window at
/// that moment. That is fine for a preview; choosing the entry still performs a
/// proper isolated window capture.
#[tauri::command]
pub fn list_window_previews() -> Result<Vec<TargetPreview>, String> {
    require_permission()?;
    let backend = backend();
    let windows = backend.windows().map_err(err)?;
    let frames = backend.freeze().map_err(err)?;

    Ok(windows
        .into_iter()
        .map(|window| TargetPreview {
            id: window.id,
            title: if window.title.is_empty() {
                window.app_name.clone()
            } else {
                window.title.clone()
            },
            subtitle: window.app_name,
            width: window.region.width,
            height: window.region.height,
            preview: frames
                .crop(window.region)
                .ok()
                .and_then(|c| capture_service::encode_preview(&c.image).ok()),
        })
        .collect())
}

#[tauri::command]
pub fn list_display_previews() -> Result<Vec<TargetPreview>, String> {
    require_permission()?;
    let backend = backend();
    let displays = backend.displays().map_err(err)?;
    let frames = backend.freeze().map_err(err)?;

    Ok(displays
        .into_iter()
        .map(|display| TargetPreview {
            id: display.id,
            title: display.name,
            subtitle: if display.is_primary {
                "Birincil ekran".to_string()
            } else {
                "Ekran".to_string()
            },
            width: display.region.width,
            height: display.region.height,
            preview: frames
                .display(display.id)
                .ok()
                .and_then(|c| capture_service::encode_preview(&c.image).ok()),
        })
        .collect())
}

// ── Region selection ────────────────────────────────────────────────────

#[tauri::command]
pub fn begin_region_capture(app: AppHandle) -> Result<(), String> {
    require_permission()?;
    overlay::begin_region_selection(&app)
}

/// Commit a selection made in the overlay. The region arrives in global
/// logical coordinates and is cropped from the frames frozen before the
/// overlay appeared.
#[tauri::command]
pub fn commit_region_capture(
    app: AppHandle,
    region: Region,
    document: Option<String>,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    let mut capture = overlay::crop_selection(&app, region)?;

    // Anything drawn on the overlay is rendered here rather than in the
    // webview, for the same reason the editor's export is: this is the only
    // way the file matches across platforms. Shapes arrive in the coordinate
    // space of the *cropped* image, which is what the overlay drew on.
    if let Some(document) = document.filter(|d| !d.trim().is_empty()) {
        match serde_json::from_str::<kestrel_editor::Document>(&document) {
            Ok(parsed) => capture.image = kestrel_editor::render(&capture.image, &parsed),
            // Losing the annotations is bad; losing the capture is worse.
            Err(err) => tracing::error!(%err, "overlay annotations could not be parsed"),
        }
    }

    finish_capture(&app, capture, &task_settings(&settings, None))
}

#[tauri::command]
pub fn cancel_region_capture(app: AppHandle) {
    overlay::finish(&app);
}

// ── Region recording ────────────────────────────────────────────────────

/// Raise the selection overlay to choose what a recording will cover.
#[tauri::command]
pub fn begin_region_recording(app: AppHandle, gif: Option<bool>) -> Result<(), String> {
    require_permission()?;
    // Said before the overlay covers the screen rather than after the user has
    // framed a rectangle: ffmpeg is what writes the file, and finding out it is
    // missing at the end wastes the whole gesture.
    if !crate::record::ffmpeg_status().available {
        return Err(format!(
            "Ekran kaydı için ffmpeg gerekiyor. Kur ve tekrar dene — {}",
            crate::record::ffmpeg_status()
                .install_hint
                .unwrap_or_default()
        ));
    }
    overlay::begin_region_recording(&app, gif.unwrap_or(false))
}

/// How long to wait after closing the overlays before the first frame is taken.
///
/// Closing a window is queued on the main thread, so recording immediately
/// catches the overlay's own dim in the first frames of the video. Long enough
/// for the compositor to have dropped it, short enough that the user does not
/// read it as a stall.
const OVERLAY_TEARDOWN: std::time::Duration = std::time::Duration::from_millis(300);

/// Start recording the committed rectangle.
///
/// The region arrives in *global logical* coordinates, which is what the
/// overlay works in. The recorder crops raw frames, so it needs the rectangle
/// in the *physical pixels of one display* — those are different numbers on
/// every Retina panel, and getting it wrong records the wrong quarter of the
/// screen.
#[tauri::command]
pub fn commit_region_recording(
    app: AppHandle,
    region: Region,
    settings: State<'_, SettingsState>,
) -> Result<crate::record::RecordingStatus, String> {
    if region.width < 1 || region.height < 1 {
        overlay::finish(&app);
        return Err("Kaydedilecek bölge boş.".into());
    }

    let gif = overlay::finish_recording_selection(&app)?;
    let (display_id, local) = display_region(region)?;

    std::thread::sleep(OVERLAY_TEARDOWN);

    let snapshot = settings.snapshot();
    let started = start_recording_inner(
        &app,
        gif,
        &snapshot.defaults,
        &snapshot.audio,
        Some((display_id, local)),
    );

    // The overlay window this call came from is already closed, so its own
    // error handling has nowhere to draw. Failures are announced the same way a
    // failed capture is, which the main window already listens for.
    if let Err(message) = &started {
        let _ = app.emit(crate::EVENT_CAPTURE_FAILED, message.clone());
    }
    started
}

/// Which display a rectangle belongs to, and where it sits inside that
/// display's frames in physical pixels.
///
/// The display is chosen by the rectangle's centre rather than its origin: a
/// selection dragged from just outside the edge of a display still belongs to
/// the display most of it covers.
fn display_region(region: Region) -> Result<(u32, Region), String> {
    let displays = backend().displays().map_err(err)?;
    let centre_x = region.x + region.width as i32 / 2;
    let centre_y = region.y + region.height as i32 / 2;

    let target = displays
        .iter()
        .find(|d| {
            centre_x >= d.region.x
                && centre_x < d.region.x + d.region.width as i32
                && centre_y >= d.region.y
                && centre_y < d.region.y + d.region.height as i32
        })
        .or_else(|| displays.iter().find(|d| d.is_primary))
        .or_else(|| displays.first())
        .ok_or("Ekran bulunamadı.")?;

    let scale = if target.scale_factor > 0.0 {
        target.scale_factor
    } else {
        1.0
    };

    let local = Region::new(
        (((region.x - target.region.x) as f32) * scale).round() as i32,
        (((region.y - target.region.y) as f32) * scale).round() as i32,
        ((region.width as f32 * scale).round() as u32).max(1),
        ((region.height as f32 * scale).round() as u32).max(1),
    );
    Ok((target.id, local))
}

// ── Picker ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_window_picker(app: AppHandle, tab: Option<String>) -> Result<(), String> {
    require_permission()?;
    overlay::open_picker(&app, tab.as_deref().unwrap_or("windows"))
}

#[tauri::command]
pub fn close_window_picker(app: AppHandle) {
    overlay::close_picker(&app);
}

// ── Workflows and settings ──────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsState>) -> AppSettings {
    settings.snapshot()
}

#[tauri::command]
pub fn list_workflows(settings: State<'_, SettingsState>) -> Vec<Workflow> {
    settings.snapshot().workflows
}

/// Rebind a workflow's global shortcut. Passing `null` unbinds it.
/// Rejects a shortcut another workflow already owns, and re-registers with the
/// OS so the change takes effect immediately.
#[tauri::command]
pub fn set_workflow_shortcut(
    app: AppHandle,
    id: String,
    accelerator: Option<String>,
    settings: State<'_, SettingsState>,
) -> Result<Vec<Workflow>, String> {
    settings
        .update(|state| {
            if let Some(accelerator) = accelerator.as_deref() {
                if let Some(owner) = state.shortcut_conflict(accelerator, &id) {
                    return Err(crate::settings::SettingsError::ShortcutConflict {
                        accelerator: accelerator.to_string(),
                        owner: owner.name.clone(),
                    });
                }
            }

            let workflow = state
                .workflows
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| crate::settings::SettingsError::UnknownWorkflow(id.clone()))?;
            workflow.shortcut = accelerator.clone();
            Ok(())
        })
        .map_err(err)?;

    crate::shortcuts::reregister(&app);
    Ok(settings.snapshot().workflows)
}

#[tauri::command]
pub fn set_workflow_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
    settings: State<'_, SettingsState>,
) -> Result<Vec<Workflow>, String> {
    settings
        .update(|state| {
            let workflow = state
                .workflows
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| crate::settings::SettingsError::UnknownWorkflow(id.clone()))?;
            workflow.enabled = enabled;
            Ok(())
        })
        .map_err(err)?;

    crate::shortcuts::reregister(&app);
    Ok(settings.snapshot().workflows)
}

#[tauri::command]
pub fn reset_shortcuts(
    app: AppHandle,
    settings: State<'_, SettingsState>,
) -> Result<Vec<Workflow>, String> {
    settings
        .update(|state| {
            let defaults = default_workflows();
            for workflow in state.workflows.iter_mut() {
                if let Some(default) = defaults.iter().find(|d| d.id == workflow.id) {
                    workflow.shortcut = default.shortcut.clone();
                    workflow.enabled = true;
                }
            }
            Ok(())
        })
        .map_err(err)?;

    crate::shortcuts::reregister(&app);
    Ok(settings.snapshot().workflows)
}

/// Which shortcuts the OS actually accepted. A shortcut another app already
/// owns cannot be registered, and the user needs to see that.
#[tauri::command]
pub fn shortcut_registration_report(app: AppHandle) -> Vec<crate::shortcuts::ShortcutReport> {
    crate::shortcuts::report(&app)
}

#[tauri::command]
pub fn set_filename_pattern(
    pattern: String,
    settings: State<'_, SettingsState>,
) -> Result<AppSettings, String> {
    settings
        .update(|state| {
            state.defaults.filename_pattern = pattern.clone();
            Ok(())
        })
        .map_err(err)?;
    Ok(settings.snapshot())
}

#[tauri::command]
pub fn set_output_directory(
    directory: Option<String>,
    settings: State<'_, SettingsState>,
) -> Result<AppSettings, String> {
    settings
        .update(|state| {
            state.defaults.output_directory = directory.clone();
            Ok(())
        })
        .map_err(err)?;
    Ok(settings.snapshot())
}

/// Live preview for the workflow editor's filename field.
#[tauri::command]
pub fn preview_filename(pattern: String) -> String {
    let ctx = NameContext {
        window_title: Some("Kestrel".into()),
        app_name: Some("Kestrel".into()),
        width: Some(1920),
        height: Some(1080),
        ..Default::default()
    };
    name_pattern::expand_sanitized(&pattern, &ctx)
}

// ── Editor ──────────────────────────────────────────────────────────────

/// Open the annotation editor on the most recent capture.
#[tauri::command]
pub fn open_editor(app: AppHandle) -> Result<crate::editor::EditorOpened, String> {
    crate::editor::open_last(&app).map_err(err)
}

/// What the editor window should load. Called by the editor on mount, and
/// again when it regains focus in case a newer capture replaced the session.
#[tauri::command]
pub fn editor_session(app: AppHandle) -> Result<crate::editor::EditorOpened, String> {
    crate::editor::session(&app).map_err(err)
}

#[tauri::command]
pub fn close_editor(app: AppHandle) {
    crate::editor::close(&app);
}

/// Set the editor's image effect chain and restage the result.
///
/// `annotationCount` lets Rust refuse an effect that would move the image out
/// from under existing annotations; see `editor::apply_effects`.
#[tauri::command]
pub fn editor_set_effects(
    app: AppHandle,
    effects: kestrel_editor::Chain,
    annotation_count: usize,
) -> Result<crate::editor::EditorOpened, String> {
    crate::editor::apply_effects(&app, effects, annotation_count).map_err(err)
}

/// Read a ShareX `.sxie` effect preset.
///
/// The result names any effect Kestrel could not map so the UI can say what was
/// left out — the format is not documented, and quietly importing a partial
/// preset would be worse than saying so.
#[tauri::command]
pub fn import_sxie(path: String) -> Result<SxiePreset, String> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let imported = kestrel_editor::import_sxie(&text).map_err(err)?;

    Ok(SxiePreset {
        name: imported.name,
        effects: imported.chain,
        unsupported: imported.unsupported,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SxiePreset {
    pub name: Option<String>,
    pub effects: kestrel_editor::Chain,
    pub unsupported: Vec<String>,
}

/// Flatten the annotations and run the after-capture pipeline over the result,
/// exactly as a fresh capture would — so the filename pattern, output folder
/// and clipboard behaviour stay consistent with everything else.
#[tauri::command]
pub fn editor_export(
    app: AppHandle,
    document: String,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    let image = crate::editor::render(&app, &document).map_err(err)?;
    let capture = kestrel_capture::Capture {
        region: kestrel_capture::Region::new(0, 0, image.width(), image.height()),
        image,
        window_title: None,
        app_name: None,
    };
    finish_capture(&app, capture, &task_settings(&settings, None))
}

// ── Destinations ────────────────────────────────────────────────────────

/// Prompts routed to the user. `{select:}`, `{inputbox:}` and `{outputbox:}`
/// are interactive by design, so an upload can need an answer mid-flight.
///
/// Until the prompt windows exist these behave as the unattended prompter
/// does — first option, supplied default — rather than blocking the upload on
/// UI that is not built yet.
pub struct UiPrompter;

/// The shared instance, so background paths (the watch folder) use exactly the
/// same answers an interactive upload would.
pub const UNATTENDED: UiPrompter = UiPrompter;

impl kestrel_upload::Prompter for UiPrompter {
    fn select(&self, options: &[String]) -> Option<String> {
        options.first().cloned()
    }
    fn input(&self, _title: Option<&str>, default: Option<&str>) -> Option<String> {
        default.map(str::to_string)
    }
    fn output(&self, title: Option<&str>, message: &str) {
        tracing::info!(title = title.unwrap_or("output"), %message, "uploader output");
    }
}

#[tauri::command]
pub fn list_destinations() -> Result<Vec<crate::uploads::Destination>, String> {
    crate::uploads::list().map_err(err)
}

/// Import a `.sxcu` file. Validated before it is stored, so a broken uploader
/// never reaches the destination list.
#[tauri::command]
pub fn import_uploader(path: String) -> Result<crate::uploads::Destination, String> {
    crate::uploads::import(std::path::Path::new(&path)).map_err(err)
}

#[tauri::command]
pub fn remove_uploader(id: String) -> Result<Vec<crate::uploads::Destination>, String> {
    crate::uploads::remove(&id).map_err(err)?;
    crate::uploads::list().map_err(err)
}

#[tauri::command]
pub fn set_default_destination(
    id: Option<String>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    settings
        .update(|state| {
            state.default_destination = id.clone().filter(|v| !v.is_empty());
            Ok(())
        })
        .map_err(err)
}

#[tauri::command]
pub fn default_destination(settings: State<'_, SettingsState>) -> Option<String> {
    settings.snapshot().default_destination
}

/// Upload the most recent capture.
///
/// The bytes are encoded here rather than read back from disk: a workflow that
/// does not save to a file still has something to upload, and re-reading would
/// pick up whatever the user has since edited.
#[tauri::command]
pub async fn upload_last_capture(
    app: AppHandle,
    destination: Option<String>,
) -> Result<crate::uploads::Uploaded, String> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or("Yüklenecek bir yakalama yok.")?;

    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(err)?;

    let settings = app.state::<SettingsState>().snapshot();
    let filename = {
        let ctx = NameContext {
            width: Some(image.width()),
            height: Some(image.height()),
            ..Default::default()
        };
        format!(
            "{}.png",
            name_pattern::expand_sanitized(&settings.defaults.filename_pattern, &ctx)
        )
    };

    let id = crate::uploads::resolve_destination(
        destination.or_else(|| settings.default_destination.clone()),
    )
    .map_err(err)?;

    let payload = kestrel_upload::Payload::File {
        bytes: bytes.into_inner(),
        filename,
        mime: "image/png".to_string(),
    };

    let uploaded = crate::uploads::upload(&id, payload, &UiPrompter)
        .await
        .map_err(err)?;

    record_upload_in_history(&app, &uploaded);
    let _ = app.emit(crate::EVENT_UPLOAD_COMPLETE, uploaded.clone());
    Ok(uploaded)
}

/// Attach an upload result to the capture it came from.
fn record_upload_in_history(app: &AppHandle, uploaded: &crate::uploads::Uploaded) {
    let Some(entry_id) = app.state::<crate::history::LastEntryId>().get() else {
        return;
    };
    if let Err(err) = app.state::<crate::history::History>().record_upload(
        entry_id,
        &uploaded.url,
        uploaded.thumbnail_url.as_deref(),
        uploaded.deletion_url.as_deref(),
        &uploaded.destination,
    ) {
        tracing::warn!(%err, "could not record the upload in history");
        return;
    }
    let _ = app.emit(crate::EVENT_HISTORY_CHANGED, ());
}

// ── History ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn history_list(
    app: AppHandle,
    query: Option<crate::history::Query>,
) -> Result<Vec<crate::history::Entry>, String> {
    app.state::<crate::history::History>()
        .list(&query.unwrap_or_default())
        .map_err(err)
}

/// Forget an entry. The file on disk is left alone — deleting a screenshot
/// because someone tidied a list would be a surprise, and an unrecoverable one.
#[tauri::command]
pub fn history_remove(app: AppHandle, id: i64) -> Result<(), String> {
    app.state::<crate::history::History>()
        .remove(id)
        .map_err(err)?;
    let _ = app.emit(crate::EVENT_HISTORY_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn history_clear(app: AppHandle) -> Result<(), String> {
    app.state::<crate::history::History>().clear().map_err(err)?;
    let _ = app.emit(crate::EVENT_HISTORY_CHANGED, ());
    Ok(())
}

/// A short, filesystem-safe name for a path.
///
/// Not a cryptographic hash and not meant to be: this only has to keep two
/// different videos from sharing one cache file, and a path with slashes,
/// spaces and non-ASCII in it cannot be a filename as it stands.
fn path_key(path: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// A picture to put on a library tile.
///
/// For a screenshot or a GIF this is the file itself — the library loads it
/// straight off disk over the asset protocol, which is why the grid can hold
/// hundreds of entries. A video has no such picture, so one frame is extracted
/// and cached; without this every recording in the library was a blank tile,
/// which is what "recordings are not saved" actually looked like.
///
/// The cache lives in the app's cache directory, not beside the recording: a
/// `-thumb.png` appearing next to the user's video is litter in their folder,
/// and would be picked up by the watch folder as a new capture.
#[tauri::command]
pub fn library_thumbnail(app: AppHandle, path: String) -> Result<String, String> {
    let source = std::path::PathBuf::from(&path);
    if !source.is_file() {
        return Err("Dosya bulunamadı.".into());
    }

    let extension = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    // Anything the webview can draw itself needs no help.
    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avif"
    ) {
        return Ok(path);
    }

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| "Önbellek klasörü yok.".to_string())?
        .join("thumbs");
    std::fs::create_dir_all(&dir).map_err(err)?;

    // Keyed by path *and* modification time, so a re-encoded file at the same
    // path does not keep showing the old frame.
    let modified = source
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cached = dir.join(format!("{:016x}-{modified}.png", path_key(&path)));
    if cached.is_file() {
        return Ok(cached.to_string_lossy().into_owned());
    }

    // A frame from the very start of a clip is often a fade-in or the desktop
    // mid-redraw, so this takes one a moment in. ffmpeg clamps a seek past the
    // end back to the last frame, so a shorter clip still yields a picture.
    kestrel_record::convert::run(
        &ffmpeg_binary()?,
        &kestrel_record::convert::thumbnail_args(&source, &cached, 0.5, 480),
    )
    .map_err(err)?;

    Ok(cached.to_string_lossy().into_owned())
}

/// One entry, for a detail view or to re-open a capture.
#[tauri::command]
pub fn history_get(app: AppHandle, id: i64) -> Result<Option<crate::history::Entry>, String> {
    app.state::<crate::history::History>().get(id).map_err(err)
}

#[tauri::command]
pub fn history_count(app: AppHandle) -> Result<i64, String> {
    app.state::<crate::history::History>().count().map_err(err)
}

#[tauri::command]
pub async fn upload_text(
    app: AppHandle,
    text: String,
    destination: Option<String>,
) -> Result<crate::uploads::Uploaded, String> {
    let configured = app.state::<SettingsState>().snapshot().default_destination;
    let id = crate::uploads::resolve_destination(destination.or(configured)).map_err(err)?;

    let uploaded = crate::uploads::upload(&id, kestrel_upload::Payload::Text(text), &UiPrompter)
        .await
        .map_err(err)?;

    let _ = app.emit(crate::EVENT_UPLOAD_COMPLETE, uploaded.clone());
    Ok(uploaded)
}

// ── Pin to screen ───────────────────────────────────────────────────────

#[tauri::command]
pub fn pin_last_capture(app: AppHandle) -> Result<crate::pin::Pinned, String> {
    crate::pin::pin_last(&app).map_err(err)
}

#[tauri::command]
pub fn close_pin(app: AppHandle, label: String) {
    crate::pin::close(&app, &label);
}

// ── Recording ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn ffmpeg_status() -> crate::record::FfmpegStatus {
    crate::record::ffmpeg_status()
}

#[tauri::command]
pub fn recording_status(app: AppHandle) -> crate::record::RecordingStatus {
    app.state::<crate::record::RecordState>().status()
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    gif: Option<bool>,
    settings: State<'_, SettingsState>,
) -> Result<crate::record::RecordingStatus, String> {
    let snapshot = settings.snapshot();
    start_recording_inner(
        &app,
        gif.unwrap_or(false),
        &snapshot.defaults,
        &snapshot.audio,
        None,
    )
}

/// Shared by the command, the tray and the shortcut dispatch.
///
/// Recordings land beside screenshots and follow the same naming pattern, so
/// one folder and one convention cover everything Kestrel produces.
///
/// `target` is the display and the rectangle inside it to record, in that
/// display's physical pixels; `None` records the primary display whole.
fn start_recording_inner(
    app: &AppHandle,
    gif: bool,
    settings: &TaskSettings,
    audio: &kestrel_record::AudioSettings,
    target: Option<(u32, Region)>,
) -> Result<crate::record::RecordingStatus, String> {
    require_permission()?;

    let record_settings = kestrel_record::RecordSettings {
        format: if gif {
            kestrel_record::OutputFormat::Gif
        } else {
            kestrel_record::OutputFormat::Video
        },
        audio: audio.clone(),
        ..Default::default()
    };

    let now = chrono::Local::now();
    let directory = match &settings.output_directory {
        Some(dir) => std::path::PathBuf::from(dir),
        None => dirs::picture_dir()
            .or_else(dirs::home_dir)
            .ok_or("Kayıt klasörü bulunamadı.")?
            .join("Kestrel"),
    }
    .join(now.format("%Y").to_string())
    .join(now.format("%m").to_string());

    let ctx = NameContext {
        datetime: now,
        ..Default::default()
    };
    let stem = {
        let expanded = name_pattern::expand_sanitized(&settings.filename_pattern, &ctx);
        if expanded.trim().is_empty() {
            now.format("%Y-%m-%d_%H-%M-%S").to_string()
        } else {
            expanded
        }
    };

    let (display_id, region) = match target {
        Some((id, region)) => (Some(id), Some(region)),
        None => (None, None),
    };

    let status = crate::record::start(
        &app.state::<crate::record::RecordState>(),
        display_id,
        region,
        &record_settings,
        &directory,
        &stem,
    )
    .map_err(err)?;

    let _ = app.emit(crate::EVENT_RECORDING_CHANGED, status.clone());
    Ok(status)
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> Result<String, String> {
    let finished = crate::record::stop(&app.state::<crate::record::RecordState>()).map_err(err)?;
    let path = finished.path.to_string_lossy().into_owned();
    tracing::info!(%path, "recording finished");

    // A recording is a capture: it goes in the history like one, or the library
    // shows every screenshot and none of the videos, which is what happened
    // until now. Failing to record it must not lose the file, which is already
    // on disk by this point.
    let entry = crate::history::NewEntry {
        filename: finished
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone()),
        path: Some(path.clone()),
        width: finished.width,
        height: finished.height,
        window_title: None,
    };
    match app
        .state::<crate::history::History>()
        .insert(&entry, chrono::Utc::now().timestamp())
    {
        Ok(id) => {
            app.state::<crate::history::LastEntryId>().set(id);
            // The library is a live view of this table, and it is open in a
            // window that did nothing to cause this. Without the event the
            // recording is in the history but invisible until the user retypes
            // a search — which reads as "recordings are not saved at all".
            let _ = app.emit(crate::EVENT_HISTORY_CHANGED, ());
        }
        Err(err) => tracing::warn!(%err, "could not record the recording in history"),
    }

    let _ = app.emit(
        crate::EVENT_RECORDING_CHANGED,
        app.state::<crate::record::RecordState>().status(),
    );
    Ok(path)
}

#[tauri::command]
pub fn cancel_recording(app: AppHandle) -> Result<(), String> {
    crate::record::cancel(&app.state::<crate::record::RecordState>()).map_err(err)?;
    let _ = app.emit(
        crate::EVENT_RECORDING_CHANGED,
        app.state::<crate::record::RecordState>().status(),
    );
    Ok(())
}

#[tauri::command]
pub fn set_recording_paused(
    app: AppHandle,
    paused: bool,
) -> Result<crate::record::RecordingStatus, String> {
    let status = crate::record::set_paused(&app.state::<crate::record::RecordState>(), paused)
        .map_err(err)?;
    let _ = app.emit(crate::EVENT_RECORDING_CHANGED, status.clone());
    Ok(status)
}

// ── Tools ───────────────────────────────────────────────────────────────

/// Read any QR codes out of the most recent capture.
///
/// An empty list is a normal answer — most screenshots have no QR code in them
/// — so this does not treat "found nothing" as a failure.
#[tauri::command]
pub fn scan_qr_code(app: AppHandle) -> Result<Vec<kestrel_tools::Decoded>, String> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or("Taranacak bir yakalama yok.")?;
    Ok(kestrel_tools::decode(&image))
}

/// Render text as a QR code, returned as a data URL for immediate display.
#[tauri::command]
pub fn generate_qr_code(text: String, module_size: Option<u32>) -> Result<String, String> {
    let image = kestrel_tools::encode(&text, module_size.unwrap_or(8)).map_err(err)?;
    capture_service::encode_preview(&image).map_err(err)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHashes {
    pub algorithm: String,
    pub digest: String,
}

#[tauri::command]
pub fn hash_file(path: String) -> Result<Vec<FileHashes>, String> {
    let results = kestrel_tools::hash_file_all(std::path::Path::new(&path)).map_err(err)?;
    Ok(results
        .into_iter()
        .map(|(algorithm, digest)| FileHashes {
            algorithm: algorithm.name().to_string(),
            digest,
        })
        .collect())
}

#[tauri::command]
pub fn compare_hash(expected: String, actual: String) -> bool {
    kestrel_tools::hash::matches(&expected, &actual)
}

#[tauri::command]
pub fn analyze_last_capture(app: AppHandle) -> Result<kestrel_tools::Analysis, String> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or("İncelenecek bir yakalama yok.")?;
    Ok(kestrel_tools::analyze(&image))
}

/// Metadata on a file, sensitive fields first.
#[tauri::command]
pub fn read_metadata(path: String) -> Result<Vec<kestrel_tools::MetadataField>, String> {
    kestrel_tools::read_metadata(std::path::Path::new(&path)).map_err(err)
}

/// Write a copy with no metadata.
///
/// Never overwrites the original: a privacy tool that destroys the only copy of
/// a photo while trying to protect it is worse than no tool.
#[tauri::command]
pub fn strip_metadata(path: String) -> Result<String, String> {
    let source = std::path::Path::new(&path);
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    let extension = source
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".into());

    let destination = source
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(format!("{stem}-temiz.{extension}"));

    kestrel_tools::strip_metadata(source, &destination).map_err(err)?;
    Ok(destination.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn index_directory(
    path: String,
    options: Option<kestrel_tools::IndexOptions>,
) -> Result<String, String> {
    let options = options.unwrap_or_default();
    let tree = kestrel_tools::index(std::path::Path::new(&path), &options).map_err(err)?;
    kestrel_tools::indexer::render(&tree, &options).map_err(err)
}

// ── Dispatch ────────────────────────────────────────────────────────────

/// Run a workflow by id. Interactive methods open their own UI; direct ones
/// capture immediately and return the result.
#[tauri::command]
pub fn run_workflow(
    app: AppHandle,
    id: String,
    settings: State<'_, SettingsState>,
) -> Result<Option<CaptureOutput>, String> {
    let snapshot = settings.snapshot();
    let workflow = snapshot
        .workflow(&id)
        .ok_or_else(|| format!("no workflow with id {id}"))?;
    dispatch(&app, workflow.method, &workflow.settings)
}

/// Shared by the tray, the shortcuts and the UI so all three behave alike.
pub fn dispatch(
    app: &AppHandle,
    method: CaptureMethod,
    settings: &TaskSettings,
) -> Result<Option<CaptureOutput>, String> {
    use CaptureMethod as M;
    require_permission()?;

    match method {
        // Interactive: these raise their own UI and finish asynchronously.
        M::Region | M::RegionLight | M::RegionTransparent => {
            overlay::begin_region_selection(app)?;
            Ok(None)
        }
        M::WindowMenu => {
            overlay::open_picker(app, "windows")?;
            Ok(None)
        }
        M::MonitorMenu => {
            overlay::open_picker(app, "displays")?;
            Ok(None)
        }

        // Direct.
        M::Fullscreen => {
            let capture = backend().capture_all_displays().map_err(err)?;
            finish_capture(app, capture, settings).map(Some)
        }
        M::ActiveMonitor => {
            let backend = backend();
            let displays = backend.displays().map_err(err)?;
            let target = displays
                .iter()
                .find(|d| d.is_primary)
                .or_else(|| displays.first())
                .ok_or("Ekran bulunamadı.")?;
            let capture = backend.capture_display(target.id).map_err(err)?;
            finish_capture(app, capture, settings).map(Some)
        }
        M::ActiveWindow => {
            let backend = backend();
            let windows = backend.windows().map_err(err)?;
            let front = windows
                .iter()
                .find(|w| w.is_focused)
                .or_else(|| windows.first())
                .ok_or("Yakalanabilir pencere bulunamadı.")?;
            let capture = backend.capture_window(front.id).map_err(err)?;
            finish_capture(app, capture, settings).map(Some)
        }

        // The shortcut toggles: pressing it again is how a recording started
        // by a shortcut gets stopped, since there is no window to click.
        M::ScreenRecording | M::ScreenRecordingGif => {
            if app.state::<crate::record::RecordState>().is_active() {
                // The same call the UI makes, so the tray and the shortcut
                // cannot end up recording to a different place — or, as they
                // did, failing to record it in the history at all.
                stop_recording(app.clone())?;
            } else {
                let gif = method.is_gif();
                let audio = app.state::<SettingsState>().snapshot().audio;
                start_recording_inner(app, gif, settings, &audio, None)?;
            }
            Ok(None)
        }
        // The region equivalent, and a toggle for the same reason: the shortcut
        // that started it is the only thing at hand to stop it.
        M::RegionRecording | M::RegionRecordingGif => {
            if app.state::<crate::record::RecordState>().is_active() {
                stop_recording(app.clone())?;
            } else {
                begin_region_recording(app.clone(), Some(method.is_gif()))?;
            }
            Ok(None)
        }
        // Toggles, like recording: press once to start, scroll the window
        // yourself, press again to join what was captured. There is no window
        // to click while the target app has the focus, so the shortcut has to
        // be both halves.
        M::ScrollingCapture => {
            if app.state::<crate::scrolling::ScrollState>().is_active() {
                let scrolled = crate::scrolling::finish(app).map_err(err)?;
                if scrolled.had_gap {
                    // Said out loud, because the picture looks complete.
                    tracing::warn!("the scrolling capture has gaps: scroll more slowly");
                }
                let capture = kestrel_capture::Capture {
                    region: kestrel_capture::Region::new(
                        scrolled.region.x,
                        scrolled.region.y,
                        scrolled.image.width(),
                        scrolled.image.height(),
                    ),
                    image: scrolled.image,
                    window_title: None,
                    app_name: None,
                };
                finish_capture(app, capture, settings).map(Some)
            } else {
                require_permission()?;
                // The front-most window is the thing being scrolled; asking for
                // a region first would mean the target loses focus, and a
                // window that is not focused does not scroll.
                let backend = backend();
                let windows = backend.windows().map_err(err)?;
                let target = windows
                    .iter()
                    .find(|w| w.is_focused)
                    .or_else(|| windows.first())
                    .ok_or("Kaydırılacak pencere bulunamadı.")?;

                crate::scrolling::start(app, target.region).map_err(err)?;
                Ok(None)
            }
        }
        M::LastRegion => Err("Son bölge henüz hazır değil.".into()),
        M::CustomRegion => Err("Özel bölge henüz hazır değil.".into()),
        M::AutoCapture => Err("Otomatik yakalama henüz hazır değil.".into()),
    }
}

/// Used by the tray and shortcut handlers, which have no `State` access.
pub fn dispatch_from_app(
    app: &AppHandle,
    method: CaptureMethod,
) -> Result<Option<CaptureOutput>, String> {
    let settings = app.state::<SettingsState>().snapshot().defaults;
    dispatch(app, method, &settings)
}

// ── OCR ─────────────────────────────────────────────────────────────────

/// Whether the OCR models are installed, and what installing them would cost.
///
/// The UI asks this before offering OCR so it can say "this downloads 20 MB"
/// rather than stalling on a silent fetch.
#[tauri::command]
pub fn ocr_status(app: AppHandle) -> crate::ocr::ModelStatus {
    crate::ocr::status(&app)
}

/// Download the OCR models. Only called once the user has agreed to it.
#[tauri::command]
pub async fn ocr_install(app: AppHandle) -> Result<crate::ocr::ModelStatus, String> {
    crate::ocr::install(&app).await.map_err(err)
}

/// Read the text in the most recent capture.
///
/// Recognition runs locally; nothing leaves the machine.
#[tauri::command]
pub fn ocr_last_capture(app: AppHandle) -> Result<kestrel_tools::ocr::Recognised, String> {
    crate::ocr::read_last(&app).map_err(err)
}

// ── Colour picker and image comparer ────────────────────────────────────

/// Read the colour at a point in the last capture, in every notation at once.
///
/// `radius` averages a square around the point. On anti-aliased text the single
/// pixel under the cursor is not the colour anyone means.
#[tauri::command]
pub fn pick_color(
    app: AppHandle,
    x: u32,
    y: u32,
    radius: Option<u32>,
) -> Result<kestrel_tools::Swatch, String> {
    let image = app
        .state::<crate::editor::LastCapture>()
        .get()
        .ok_or("Renk seçmek için önce bir yakalama gerek.")?;

    match radius.unwrap_or(0) {
        0 => kestrel_tools::pick_color(&image, x, y),
        radius => kestrel_tools::pick_average(&image, x, y, radius),
    }
    .ok_or_else(|| "Seçilen nokta görselin dışında.".to_string())
}

/// Convert a colour the user typed, so the panel works without a capture.
#[tauri::command]
pub fn parse_color(text: String) -> Result<kestrel_tools::Swatch, String> {
    kestrel_tools::Rgb::from_hex(&text)
        .map(|rgb| rgb.swatch())
        .ok_or_else(|| format!("`{text}` bir renk değeri değil."))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageComparison {
    #[serde(flatten)]
    pub summary: kestrel_tools::Comparison,
    /// The diff picture, as a data URL for the preview.
    pub preview: String,
}

/// Compare two image files and describe what differs.
#[tauri::command]
pub fn compare_images(
    first: String,
    second: String,
    tolerance: Option<u8>,
) -> Result<ImageComparison, String> {
    let tolerance = tolerance.unwrap_or(kestrel_tools::compare::DEFAULT_TOLERANCE);
    let a = image::open(&first).map_err(err)?.to_rgba8();
    let b = image::open(&second).map_err(err)?.to_rgba8();

    let summary = kestrel_tools::compare(&a, &b, tolerance);
    let preview = capture_service::encode_preview(&kestrel_tools::diff_image(&a, &b, tolerance))
        .map_err(err)?;

    Ok(ImageComparison { summary, preview })
}

// ── Video tools ─────────────────────────────────────────────────────────

fn ffmpeg_binary() -> Result<std::path::PathBuf, String> {
    kestrel_record::ffmpeg::find()
        .ok_or_else(|| kestrel_record::ffmpeg::FfmpegError::NotFound.to_string())
}

/// Convert a video, writing the result beside the source.
///
/// The source is never the destination: ffmpeg runs with `-y`, so reading and
/// writing the same path would leave the user with a truncated original.
#[tauri::command]
pub fn convert_video(
    path: String,
    settings: kestrel_record::ConvertSettings,
) -> Result<String, String> {
    let output = kestrel_record::convert(&ffmpeg_binary()?, std::path::Path::new(&path), &settings)
        .map_err(err)?;
    Ok(output.to_string_lossy().into_owned())
}

/// Grab a single frame from a video as a PNG.
#[tauri::command]
pub fn video_thumbnail(
    path: String,
    at_seconds: f32,
    width: Option<u32>,
) -> Result<String, String> {
    let output = kestrel_record::thumbnail(
        &ffmpeg_binary()?,
        std::path::Path::new(&path),
        at_seconds,
        width.unwrap_or(480),
    )
    .map_err(err)?;
    Ok(output.to_string_lossy().into_owned())
}

// ── Workflow task chains ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: &'static str,
    /// Whether Kestrel actually performs this task yet.
    ///
    /// Sent to the UI so an unimplemented task can be greyed out rather than
    /// being selectable and then quietly doing nothing.
    pub implemented: bool,
    /// Whether the task is pointless without "save to file" earlier in the
    /// chain, so the UI can say so instead of leaving the user guessing.
    pub needs_saved_file: bool,
}

/// Every after-capture and after-upload task, in pipeline order.
#[tauri::command]
pub fn list_tasks() -> (Vec<TaskInfo>, Vec<TaskInfo>) {
    use kestrel_core::model::{AfterCaptureTask, AfterUploadTask};

    (
        AfterCaptureTask::ALL
            .iter()
            .map(|task| TaskInfo {
                id: task.id(),
                implemented: task.implemented(),
                needs_saved_file: task.needs_saved_file(),
            })
            .collect(),
        AfterUploadTask::ALL
            .iter()
            .map(|task| TaskInfo {
                id: task.id(),
                implemented: task.implemented(),
                needs_saved_file: false,
            })
            .collect(),
    )
}

/// Replace a workflow's task chain, or the defaults when `id` is absent.
///
/// The chain is stored in pipeline order regardless of the order it arrives in.
/// The order is the pipeline — saving before copying the path, uploading before
/// deleting the file — so honouring an arbitrary order would let the UI build a
/// workflow that cannot work.
#[tauri::command]
pub fn set_tasks(
    id: Option<String>,
    after_capture: Vec<kestrel_core::model::AfterCaptureTask>,
    after_upload: Vec<kestrel_core::model::AfterUploadTask>,
    settings: State<'_, SettingsState>,
) -> Result<AppSettings, String> {
    let mut after_capture = after_capture;
    let mut after_upload = after_upload;
    after_capture.sort_unstable();
    after_capture.dedup();
    after_upload.sort_unstable();
    after_upload.dedup();

    settings
        .update(|state| {
            let target = match &id {
                Some(id) => {
                    &mut state
                        .workflows
                        .iter_mut()
                        .find(|w| w.id == *id)
                        .ok_or_else(|| crate::settings::SettingsError::UnknownWorkflow(id.clone()))?
                        .settings
                }
                None => &mut state.defaults,
            };
            target.after_capture = after_capture.clone();
            target.after_upload = after_upload.clone();
            Ok(())
        })
        .map_err(err)?;

    Ok(settings.snapshot())
}

// ── Image combiner and splitter ─────────────────────────────────────────

/// Stack images into one, as ShareX's image combiner.
///
/// The result is written beside the first input. Different sizes are aligned to
/// the start with transparent gaps rather than stretched — a stretched
/// screenshot is unreadable, which defeats the point of combining them.
#[tauri::command]
pub fn combine_images(
    paths: Vec<String>,
    vertical: bool,
    spacing: Option<u32>,
) -> Result<String, String> {
    if paths.len() < 2 {
        return Err("Birleştirmek için en az iki görsel gerek.".to_string());
    }

    let images = paths
        .iter()
        .map(|path| {
            image::open(path)
                .map(|image| image.to_rgba8())
                .map_err(|e| format!("{path}: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let direction = if vertical {
        kestrel_tools::Direction::Vertical
    } else {
        kestrel_tools::Direction::Horizontal
    };
    let combined = kestrel_tools::combine(&images, direction, spacing.unwrap_or(0))
        .ok_or("Birleştirilecek görsel yok.")?;

    let first = std::path::Path::new(&paths[0]);
    let destination = first.with_file_name(format!(
        "{}-combined.png",
        first.file_stem().unwrap_or_default().to_string_lossy()
    ));
    combined.save(&destination).map_err(err)?;

    Ok(destination.to_string_lossy().into_owned())
}

/// Cut an image into a grid, as ShareX's image splitter.
#[tauri::command]
pub fn split_image(path: String, columns: u32, rows: u32) -> Result<Vec<String>, String> {
    let image = image::open(&path).map_err(err)?.to_rgba8();
    let source = std::path::Path::new(&path);
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();

    kestrel_tools::split(&image, columns, rows)
        .into_iter()
        .enumerate()
        .map(|(index, tile)| {
            let destination = source.with_file_name(format!("{stem}-{}.png", index + 1));
            tile.save(&destination).map_err(err)?;
            Ok(destination.to_string_lossy().into_owned())
        })
        .collect()
}

// ── Watch folder ────────────────────────────────────────────────────────

#[tauri::command]
pub fn watch_status(app: AppHandle) -> crate::watch::WatchStatus {
    app.state::<crate::watch::WatchState>().status()
}

/// Start or stop watching a folder, remembering the choice.
///
/// The setting is written before the watcher starts, so a directory that turns
/// out to be unwatchable still leaves the app in a state the user can see and
/// correct rather than one that silently forgets what they picked.
#[tauri::command]
pub fn set_watch(
    app: AppHandle,
    enabled: bool,
    directory: Option<String>,
    settings: State<'_, SettingsState>,
) -> Result<crate::watch::WatchStatus, String> {
    settings
        .update(|state| {
            state.watch.enabled = enabled;
            state.watch.directory = directory.clone();
            Ok(())
        })
        .map_err(err)?;

    let state = app.state::<crate::watch::WatchState>();
    if !enabled {
        state.stop();
        return Ok(state.status());
    }

    let directory = directory.ok_or_else(|| crate::watch::WatchError::NoDirectory.to_string())?;
    crate::watch::start(&app, std::path::Path::new(&directory)).map_err(err)
}

// ── Recording audio ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioOptions {
    pub devices: Vec<kestrel_record::AudioDevice>,
    pub selected: Option<String>,
    pub bitrate_kbps: u32,
    /// Set on platforms where recording the system's own output needs extra
    /// software, with an explanation to show the user.
    pub system_audio_note: Option<&'static str>,
}

/// The audio inputs ffmpeg can see, and what is currently chosen.
#[tauri::command]
pub fn audio_options(settings: State<'_, SettingsState>) -> Result<AudioOptions, String> {
    let ffmpeg = kestrel_record::ffmpeg::find()
        .ok_or_else(|| kestrel_record::ffmpeg::FfmpegError::NotFound.to_string())?;
    let chosen = settings.snapshot().audio;

    Ok(AudioOptions {
        devices: kestrel_record::audio::devices(&ffmpeg),
        selected: chosen.device.clone(),
        bitrate_kbps: chosen.bitrate(),
        system_audio_note: kestrel_record::audio::system_audio_note(),
    })
}

/// Choose the audio input for recordings, or `None` for silence.
#[tauri::command]
pub fn set_audio(
    device: Option<String>,
    bitrate_kbps: Option<u32>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    settings
        .update(|state| {
            state.audio.device = device.clone().filter(|d| !d.trim().is_empty());
            if let Some(bitrate) = bitrate_kbps {
                state.audio.bitrate_kbps = bitrate;
            }
            Ok(())
        })
        .map_err(err)
}

/// A magnified patch of the frozen screen under the overlay's cursor.
///
/// Called on pointer move while the magnifier is showing, so it stays small and
/// synchronous: a 33x33 patch is about a kilobyte.
#[tauri::command]
pub fn overlay_sample(
    app: AppHandle,
    x: i32,
    y: i32,
    radius: Option<u32>,
) -> Result<crate::overlay::Sample, String> {
    crate::overlay::sample(&app, x, y, radius.unwrap_or(12))
}

/// The image on the clipboard, as a PNG data URL.
///
/// Read through Rust rather than the webview's `paste` event. The overlay is a
/// borderless transparent always-on-top window; whether it has the focus a DOM
/// paste needs is up to the platform, and on macOS it frequently does not — so
/// Cmd+V did nothing at all. Asking the OS directly works regardless of focus,
/// and it is the same clipboard the rest of the app writes to.
///
/// `None` when the clipboard holds no image. That is not an error: pasting text
/// onto a screenshot is a reasonable thing to try and doing nothing is the
/// right answer.
#[tauri::command]
pub fn clipboard_image() -> Result<Option<String>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(err)?;

    let image = match clipboard.get_image() {
        Ok(image) => image,
        // arboard reports "no image" as an error, so a missing image and a
        // broken clipboard look the same here. Treating both as "nothing to
        // paste" is right: neither is something the user can act on.
        Err(_) => return Ok(None),
    };

    encode_clipboard_image(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .map(Some)
}

/// Turn raw RGBA from the clipboard into a PNG data URL.
///
/// Split out from the command because this is the part that can be quietly
/// wrong — a byte count that does not match the reported size produces either
/// a panic or a skewed image, and neither is something the clipboard tells you
/// about.
fn encode_clipboard_image(width: u32, height: u32, bytes: Vec<u8>) -> Result<String, String> {
    let buffer = image::RgbaImage::from_raw(width, height, bytes).ok_or_else(|| {
        format!("the clipboard reported {width}x{height} but sent a different number of bytes")
    })?;

    let mut png = std::io::Cursor::new(Vec::new());
    buffer
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(err)?;

    use base64::Engine as _;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clipboard_image_becomes_a_png_data_url() {
        let pixels = vec![255u8; 4 * 4 * 4];
        let url = encode_clipboard_image(4, 4, pixels).expect("encodes");

        assert!(url.starts_with("data:image/png;base64,"), "{url}");

        // Decode it back rather than trusting the prefix: a truncated or
        // mis-encoded payload still starts with the right characters.
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(url.split_once(',').unwrap().1)
            .expect("valid base64");
        let decoded = image::load_from_memory(&bytes).expect("a real png");

        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 4);
    }

    #[test]
    fn a_byte_count_that_does_not_match_the_size_is_an_error_not_a_panic() {
        // The clipboard is an OS API handing over a length and a buffer; if
        // they disagree, saying so beats a panic or a skewed image.
        let result = encode_clipboard_image(100, 100, vec![0u8; 16]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("100x100"));
    }

    #[test]
    fn a_zero_sized_clipboard_image_is_refused() {
        assert!(encode_clipboard_image(0, 10, Vec::new()).is_err());
    }
}

// ── Scrolling capture ───────────────────────────────────────────────────

#[tauri::command]
pub fn scrolling_status(app: AppHandle) -> crate::scrolling::ScrollStatus {
    app.state::<crate::scrolling::ScrollState>().status()
}

/// Start or finish a scrolling capture, whichever applies.
///
/// One command rather than two, because the caller is always toggling: a
/// scrolling capture has no meaningful "start again while running".
#[tauri::command]
pub fn toggle_scrolling_capture(app: AppHandle) -> Result<Option<CaptureOutput>, String> {
    dispatch_from_app(&app, CaptureMethod::ScrollingCapture)
}

#[tauri::command]
pub fn cancel_scrolling_capture(app: AppHandle) {
    crate::scrolling::cancel(&app);
}

// ── Background behaviour ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundStatus {
    pub close_to_tray: bool,
    pub menu_bar_only: bool,
    /// Read back from the OS rather than from settings, so the checkbox cannot
    /// disagree with the system's own login items.
    pub launch_at_login: bool,
    /// False where hiding the dock icon has no meaning, so the UI can leave the
    /// control out instead of offering one that does nothing.
    pub supports_menu_bar_only: bool,
}

#[tauri::command]
pub fn background_status(app: AppHandle, settings: State<'_, SettingsState>) -> BackgroundStatus {
    let background = settings.snapshot().background;
    BackgroundStatus {
        close_to_tray: background.close_to_tray,
        menu_bar_only: background.menu_bar_only,
        launch_at_login: crate::background::launches_at_login(&app),
        supports_menu_bar_only: cfg!(target_os = "macos"),
    }
}

/// Change one of the background behaviours.
///
/// Each is applied before it is persisted, so a setting that the OS refuses —
/// adding a login item, most likely — is reported instead of being written down
/// as though it had worked.
#[tauri::command]
pub fn set_background(
    app: AppHandle,
    close_to_tray: Option<bool>,
    menu_bar_only: Option<bool>,
    launch_at_login: Option<bool>,
    settings: State<'_, SettingsState>,
) -> Result<BackgroundStatus, String> {
    if let Some(enabled) = launch_at_login {
        crate::background::apply_launch_at_login(&app, enabled)?;
    }
    if let Some(enabled) = menu_bar_only {
        crate::background::apply_activation_policy(&app, enabled);
    }

    settings
        .update(|state| {
            if let Some(value) = close_to_tray {
                state.background.close_to_tray = value;
            }
            if let Some(value) = menu_bar_only {
                state.background.menu_bar_only = value;
            }
            if let Some(value) = launch_at_login {
                state.background.launch_at_login = value;
            }
            Ok(())
        })
        .map_err(err)?;

    Ok(background_status(app, settings))
}

/// Quit for real, from the UI.
///
/// Needed precisely because closing the window no longer does: without an
/// explicit way out, the only one left is the tray, and someone who has hidden
/// the dock icon and closed the window has to go looking for it.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
