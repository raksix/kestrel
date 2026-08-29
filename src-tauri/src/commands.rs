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
        Ok(id) => app.state::<crate::history::LastEntryId>().set(id),
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
        if let Err(e) = crate::editor::open(app, image) {
            tracing::error!(%e, "could not open the editor");
        }
    }

    Ok(output)
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
struct UiPrompter;

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
    }
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
        .map_err(err)
}

#[tauri::command]
pub fn history_clear(app: AppHandle) -> Result<(), String> {
    app.state::<crate::history::History>().clear().map_err(err)
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
    let defaults = settings.snapshot().defaults;
    start_recording_inner(&app, gif.unwrap_or(false), &defaults)
}

/// Shared by the command, the tray and the shortcut dispatch.
///
/// Recordings land beside screenshots and follow the same naming pattern, so
/// one folder and one convention cover everything Kestrel produces.
fn start_recording_inner(
    app: &AppHandle,
    gif: bool,
    settings: &TaskSettings,
) -> Result<crate::record::RecordingStatus, String> {
    require_permission()?;

    let record_settings = kestrel_record::RecordSettings {
        format: if gif {
            kestrel_record::OutputFormat::Gif
        } else {
            kestrel_record::OutputFormat::Video
        },
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

    let status = crate::record::start(
        &app.state::<crate::record::RecordState>(),
        None,
        None,
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
    let path = crate::record::stop(&app.state::<crate::record::RecordState>()).map_err(err)?;
    let path = path.to_string_lossy().into_owned();
    tracing::info!(%path, "recording finished");

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
                crate::record::stop(&app.state::<crate::record::RecordState>()).map_err(err)?;
                let _ = app.emit(
                    crate::EVENT_RECORDING_CHANGED,
                    app.state::<crate::record::RecordState>().status(),
                );
            } else {
                let gif = matches!(method, M::ScreenRecordingGif);
                start_recording_inner(app, gif, settings)?;
            }
            Ok(None)
        }
        M::ScrollingCapture => Err("Kaydırmalı yakalama henüz hazır değil (faz 6).".into()),
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
