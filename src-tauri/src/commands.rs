//! The IPC surface. This is the only API the frontend sees.
//!
//! Every command returns `Result<_, String>` because Tauri needs a
//! serialisable error; the real error types stay in the Rust crates.

use kestrel_capture::{Capabilities, CaptureBackend, DisplayInfo, Region, WindowInfo};
use kestrel_core::{
    model::{default_workflows, TaskSettings, Workflow},
    name_pattern::{self, NameContext},
    CaptureMethod,
};

use crate::capture_service::{self, CaptureOutput};

fn backend() -> impl CaptureBackend {
    kestrel_capture::backend()
}

fn to_string_err<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

#[tauri::command]
pub fn list_displays() -> Result<Vec<DisplayInfo>, String> {
    backend().displays().map_err(to_string_err)
}

#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    backend().windows().map_err(to_string_err)
}

/// What this platform can actually do. The UI disables anything unsupported
/// and explains why rather than failing at the point of use.
#[tauri::command]
pub fn platform_capabilities() -> Capabilities {
    backend().capabilities()
}

#[tauri::command]
pub fn capture(method: CaptureMethod) -> Result<CaptureOutput, String> {
    let backend = backend();
    let capture = capture_service::capture_for_method(&backend, method).map_err(to_string_err)?;
    capture_service::process(capture, &TaskSettings::default()).map_err(to_string_err)
}

/// Called by the overlay once the user has committed a selection.
#[tauri::command]
pub fn capture_region(region: Region) -> Result<CaptureOutput, String> {
    let backend = backend();
    let capture = backend.capture_region(region).map_err(to_string_err)?;
    capture_service::process(capture, &TaskSettings::default()).map_err(to_string_err)
}

/// Live preview for the workflow editor's filename field.
/// Powers the feature ShareX lacks: seeing what `%y-%mo-%d` actually produces.
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

#[tauri::command]
pub fn list_workflows() -> Vec<Workflow> {
    default_workflows()
}
