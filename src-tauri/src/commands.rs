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

/// Broadcast a finished capture so every window learns about it.
///
/// The overlay and the picker are closed immediately after they commit, so the
/// value returned to *them* is discarded. Without this event the main window
/// would never show a capture the user made from either surface.
fn finish_capture(app: &AppHandle, output: CaptureOutput) -> CaptureOutput {
    let _ = app.emit(crate::EVENT_CAPTURE_COMPLETE, output.clone());
    output
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
pub fn capture_fullscreen(settings: State<'_, SettingsState>) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_all_displays().map_err(err)?;
    capture_service::process(capture, &task_settings(&settings, None)).map_err(err)
}

#[tauri::command]
pub fn capture_display(
    app: AppHandle,
    id: u32,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_display(id).map_err(err)?;
    let output = capture_service::process(capture, &task_settings(&settings, None)).map_err(err)?;
    Ok(finish_capture(&app, output))
}

#[tauri::command]
pub fn capture_window(
    app: AppHandle,
    id: u32,
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    require_permission()?;
    let capture = backend().capture_window(id).map_err(err)?;
    let output = capture_service::process(capture, &task_settings(&settings, None)).map_err(err)?;
    Ok(finish_capture(&app, output))
}

/// The front-most window, without showing a picker — ShareX's "active window".
#[tauri::command]
pub fn capture_active_window(settings: State<'_, SettingsState>) -> Result<CaptureOutput, String> {
    require_permission()?;
    let backend = backend();
    let windows = backend.windows().map_err(err)?;
    let front = windows
        .iter()
        .find(|w| w.is_focused)
        .or_else(|| windows.first())
        .ok_or("Yakalanabilir pencere bulunamadı.")?;
    let capture = backend.capture_window(front.id).map_err(err)?;
    capture_service::process(capture, &task_settings(&settings, None)).map_err(err)
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
    settings: State<'_, SettingsState>,
) -> Result<CaptureOutput, String> {
    let capture = overlay::crop_selection(&app, region)?;
    let output = capture_service::process(capture, &task_settings(&settings, None)).map_err(err)?;
    Ok(finish_capture(&app, output))
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
            capture_service::process(capture, settings)
                .map(Some)
                .map_err(err)
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
            capture_service::process(capture, settings)
                .map(Some)
                .map_err(err)
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
            capture_service::process(capture, settings)
                .map(Some)
                .map_err(err)
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
