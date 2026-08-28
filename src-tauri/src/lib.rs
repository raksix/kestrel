//! Kestrel desktop shell.
//!
//! Deliberately thin: window/tray/shortcut wiring and an IPC surface. All
//! domain logic lives in the `kestrel-*` crates so the CLI and the test suite
//! can use it without Tauri.

mod capture_service;
mod commands;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

/// Event emitted when a capture finishes, consumed by the post-capture card.
const EVENT_CAPTURE_COMPLETE: &str = "kestrel://capture-complete";
/// Event emitted when a capture fails, so the UI can surface the reason.
const EVENT_CAPTURE_FAILED: &str = "kestrel://capture-failed";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("KESTREL_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(global_shortcut_plugin());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            commands::list_displays,
            commands::list_windows,
            commands::platform_capabilities,
            commands::capture,
            commands::capture_region,
            commands::preview_filename,
            commands::list_workflows,
        ])
        .setup(|app| {
            build_tray(app.handle())?;

            #[cfg(desktop)]
            register_default_shortcuts(app.handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Kestrel");
}

/// Run a capture on a worker thread and report the outcome to the frontend.
/// Capture can take tens of milliseconds; never block the UI thread with it.
fn run_capture(app: &AppHandle, method: kestrel_core::CaptureMethod) {
    let app = app.clone();
    std::thread::spawn(move || match commands::capture(method) {
        Ok(output) => {
            tracing::info!(?method, "capture complete");
            let _ = app.emit(EVENT_CAPTURE_COMPLETE, output);
        }
        Err(err) => {
            tracing::error!(%err, ?method, "capture failed");
            let _ = app.emit(EVENT_CAPTURE_FAILED, err);
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItem::with_id(app, "capture-region", "Bölge yakala", true, None::<&str>)?;
    let fullscreen = MenuItem::with_id(app, "capture-fullscreen", "Tüm ekran", true, None::<&str>)?;
    let window = MenuItem::with_id(app, "capture-window", "Pencere", true, None::<&str>)?;
    let library = MenuItem::with_id(app, "open-library", "Kütüphane…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Kestrel'den çık", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &region,
            &fullscreen,
            &window,
            &PredefinedMenuItem::separator(app)?,
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
        .on_menu_event(|app, event| {
            use kestrel_core::CaptureMethod as M;
            match event.id().as_ref() {
                "capture-region" => run_capture(app, M::Region),
                "capture-fullscreen" => run_capture(app, M::Fullscreen),
                "capture-window" => run_capture(app, M::ActiveWindow),
                "open-library" => show_main_window(app),
                "quit" => app.exit(0),
                other => tracing::warn!(id = other, "unhandled tray menu item"),
            }
        })
        .build(app)?;

    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Fire on press only; without this every shortcut captures twice.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let accelerator = shortcut.into_string();
            let Some(workflow) = kestrel_core::default_workflows()
                .into_iter()
                .find(|w| w.shortcut.as_deref() == Some(accelerator.as_str()))
            else {
                tracing::warn!(%accelerator, "shortcut fired with no matching workflow");
                return;
            };
            run_capture(app, workflow.method);
        })
        .build()
}

#[cfg(desktop)]
fn register_default_shortcuts(app: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    for workflow in kestrel_core::default_workflows() {
        let Some(accelerator) = workflow.shortcut.as_deref() else {
            continue;
        };
        // A shortcut already owned by another app must not take the whole
        // startup down — log it so the UI can offer a rebind later.
        match app.global_shortcut().register(accelerator) {
            Ok(()) => {
                tracing::info!(%accelerator, workflow = %workflow.name, "shortcut registered")
            }
            Err(err) => {
                tracing::warn!(%accelerator, %err, "shortcut unavailable, likely taken by another app")
            }
        }
    }
}
