//! Kestrel desktop shell.
//!
//! Deliberately thin: window/tray/shortcut wiring and an IPC surface. All
//! domain logic lives in the `kestrel-*` crates so the CLI and the test suite
//! can use it without Tauri.

mod capture_service;
mod commands;
mod editor;
mod history;
mod ocr;
mod overlay;
mod pin;
mod record;
mod rpc;
mod settings;
mod shortcuts;
mod uploads;
mod watch;
mod window_level;

use kestrel_core::CaptureMethod;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

/// Emitted when a capture finishes, consumed by the post-capture card.
pub const EVENT_CAPTURE_COMPLETE: &str = "kestrel://capture-complete";
/// Emitted when a capture fails, so the UI can surface the reason.
pub const EVENT_CAPTURE_FAILED: &str = "kestrel://capture-failed";
/// Emitted after shortcuts are re-registered, with which ones the OS accepted.
pub const EVENT_SHORTCUTS_CHANGED: &str = "kestrel://shortcuts-changed";
/// Emitted when an upload finishes, so any window can show the resulting URL.
pub const EVENT_UPLOAD_COMPLETE: &str = "kestrel://upload-complete";
/// Emitted whenever recording starts, stops or is paused.
pub const EVENT_RECORDING_CHANGED: &str = "kestrel://recording-changed";
/// Emitted when a capture turned out to contain QR codes.
pub const EVENT_QR_FOUND: &str = "kestrel://qr-found";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("KESTREL_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(shortcuts::plugin())
        .manage(settings::SettingsState::new())
        .manage(overlay::OverlayState::default())
        .manage(editor::EditorState::default())
        .manage(editor::LastCapture::default())
        .manage(history::History::open())
        .manage(history::LastEntryId::default())
        .manage(ocr::OcrState::default())
        .manage(watch::WatchState::default())
        .manage(record::RecordState::default())
        .manage(shortcuts::ShortcutRegistry::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_displays,
            commands::list_windows,
            commands::platform_capabilities,
            commands::permission_status,
            commands::request_screen_permission,
            commands::open_permission_settings,
            commands::capture_fullscreen,
            commands::capture_display,
            commands::capture_window,
            commands::capture_active_window,
            commands::window_thumbnail,
            commands::display_thumbnail,
            commands::list_window_previews,
            commands::list_display_previews,
            commands::begin_region_capture,
            commands::commit_region_capture,
            commands::cancel_region_capture,
            commands::overlay_sample,
            commands::open_window_picker,
            commands::close_window_picker,
            commands::get_settings,
            commands::list_workflows,
            commands::set_workflow_shortcut,
            commands::set_workflow_enabled,
            commands::reset_shortcuts,
            commands::list_tasks,
            commands::set_tasks,
            commands::shortcut_registration_report,
            commands::set_filename_pattern,
            commands::set_output_directory,
            commands::preview_filename,
            commands::run_workflow,
            commands::open_editor,
            commands::editor_session,
            commands::close_editor,
            commands::editor_export,
            commands::editor_set_effects,
            commands::import_sxie,
            commands::ocr_status,
            commands::ocr_install,
            commands::ocr_last_capture,
            commands::list_destinations,
            commands::import_uploader,
            commands::remove_uploader,
            commands::set_default_destination,
            commands::default_destination,
            commands::upload_last_capture,
            commands::upload_text,
            commands::history_list,
            commands::history_get,
            commands::history_remove,
            commands::history_clear,
            commands::history_count,
            commands::pin_last_capture,
            commands::close_pin,
            commands::ffmpeg_status,
            commands::recording_status,
            commands::start_recording,
            commands::stop_recording,
            commands::cancel_recording,
            commands::set_recording_paused,
            commands::audio_options,
            commands::set_audio,
            commands::scan_qr_code,
            commands::generate_qr_code,
            commands::hash_file,
            commands::compare_hash,
            commands::analyze_last_capture,
            commands::pick_color,
            commands::parse_color,
            commands::compare_images,
            commands::combine_images,
            commands::split_image,
            commands::convert_video,
            commands::video_thumbnail,
            commands::read_metadata,
            commands::strip_metadata,
            commands::index_directory,
            commands::watch_status,
            commands::set_watch,
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            shortcuts::reregister(app.handle());
            resume_watch(app.handle());
            rpc::serve(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running Kestrel")
        .run(|_app, event| {
            // Drop the endpoint file on the way out, so `kestrel capture`
            // reports "not running" instead of failing to reach a dead port.
            if matches!(event, tauri::RunEvent::Exit) {
                rpc::withdraw();
            }
        });
}

/// Restart the watch folder if it was on when the app last quit.
///
/// Without this the setting would persist but the behaviour would not, which
/// is the confusing half of both worlds: the toggle reads "on" and nothing
/// happens.
fn resume_watch(app: &AppHandle) {
    let watch = app.state::<settings::SettingsState>().snapshot().watch;
    if !watch.enabled {
        return;
    }

    let Some(directory) = watch.directory else {
        return;
    };
    if let Err(err) = watch::start(app, std::path::Path::new(&directory)) {
        tracing::warn!(%err, "could not resume the watch folder");
    }
}

/// Dispatch a capture off the UI thread and report the outcome by event.
///
/// Interactive methods (region overlay, picker) return `None` here — their
/// result arrives later, when the user commits a selection.
pub fn run_in_background(app: &AppHandle, method: CaptureMethod) {
    let app = app.clone();
    std::thread::spawn(move || match commands::dispatch_from_app(&app, method) {
        Ok(Some(output)) => {
            tracing::info!(?method, "capture complete");
            let _ = app.emit(EVENT_CAPTURE_COMPLETE, output);
        }
        Ok(None) => tracing::debug!(?method, "interactive capture started"),
        Err(err) => {
            tracing::error!(%err, ?method, "capture failed");
            let _ = app.emit(EVENT_CAPTURE_FAILED, err);
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItem::with_id(app, "capture-region", "Bölge yakala", true, None::<&str>)?;
    let fullscreen = MenuItem::with_id(app, "capture-fullscreen", "Tüm ekran", true, None::<&str>)?;
    let window_menu = MenuItem::with_id(app, "capture-window", "Pencere seç…", true, None::<&str>)?;
    let active_window = MenuItem::with_id(
        app,
        "capture-active-window",
        "Aktif pencere",
        true,
        None::<&str>,
    )?;
    let monitor_menu = MenuItem::with_id(app, "capture-monitor", "Ekran seç…", true, None::<&str>)?;
    let edit_last = MenuItem::with_id(
        app,
        "edit-last",
        "Son yakalamayı düzenle…",
        true,
        None::<&str>,
    )?;
    let record = MenuItem::with_id(
        app,
        "record",
        "Ekran kaydı başlat/durdur",
        true,
        None::<&str>,
    )?;
    let record_gif = MenuItem::with_id(
        app,
        "record-gif",
        "GIF kaydı başlat/durdur",
        true,
        None::<&str>,
    )?;
    let pin_last = MenuItem::with_id(app, "pin-last", "Ekrana sabitle", true, None::<&str>)?;
    let library = MenuItem::with_id(app, "open-library", "Kestrel'i aç…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Kestrel'den çık", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &region,
            &fullscreen,
            &window_menu,
            &active_window,
            &monitor_menu,
            &PredefinedMenuItem::separator(app)?,
            &record,
            &record_gif,
            &PredefinedMenuItem::separator(app)?,
            &edit_last,
            &pin_last,
            &library,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("kestrel-tray")
        .icon(
            app.default_window_icon().cloned().ok_or_else(|| {
                tauri::Error::AssetNotFound("default window icon is missing".into())
            })?,
        )
        .icon_as_template(true)
        .tooltip("Kestrel")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture-region" => run_in_background(app, CaptureMethod::Region),
            "capture-fullscreen" => run_in_background(app, CaptureMethod::Fullscreen),
            "capture-window" => run_in_background(app, CaptureMethod::WindowMenu),
            "capture-active-window" => run_in_background(app, CaptureMethod::ActiveWindow),
            "capture-monitor" => run_in_background(app, CaptureMethod::MonitorMenu),
            "record" => run_in_background(app, CaptureMethod::ScreenRecording),
            "record-gif" => run_in_background(app, CaptureMethod::ScreenRecordingGif),
            "edit-last" => open_editor(app),
            "pin-last" => pin_last_capture(app),
            "open-library" => show_main_window(app),
            "quit" => app.exit(0),
            other => tracing::warn!(id = other, "unhandled tray menu item"),
        })
        .build(app)?;

    Ok(())
}

/// Float the last capture above everything else.
fn pin_last_capture(app: &AppHandle) {
    if let Err(err) = pin::pin_last(app) {
        tracing::warn!(%err, "could not pin the last capture");
        let _ = app.emit(EVENT_CAPTURE_FAILED, err.to_string());
    }
}

/// Raise the editor on the last capture, reporting failure the same way a
/// failed capture is reported rather than doing nothing visible.
fn open_editor(app: &AppHandle) {
    if let Err(err) = editor::open_last(app) {
        tracing::warn!(%err, "could not open the editor");
        let _ = app.emit(EVENT_CAPTURE_FAILED, err.to_string());
    }
}

pub fn show_main_window(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
}
