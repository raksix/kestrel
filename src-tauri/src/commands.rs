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

        // Not built yet. Say so plainly rather than quietly doing something
        // else — a screenshot of the whole screen is not a screen recording.
        M::ScreenRecording | M::ScreenRecordingGif => {
            Err("Ekran kaydı henüz hazır değil (faz 4).".into())
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
