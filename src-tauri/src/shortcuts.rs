//! Global shortcut registration.
//!
//! A shortcut another application already owns simply cannot be registered.
//! ShareX surfaces this in a dialog on startup; Kestrel keeps a live report so
//! the settings UI can show which bindings actually took effect and offer a
//! rebind, instead of leaving the user pressing a dead key combination.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::SettingsState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutReport {
    pub workflow_id: String,
    pub name: String,
    pub accelerator: String,
    pub registered: bool,
    /// Why registration failed, in the user's language.
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ShortcutState_(pub Mutex<Vec<ShortcutReport>>);

pub fn report(app: &AppHandle) -> Vec<ShortcutReport> {
    app.state::<ShortcutState_>()
        .0
        .lock()
        .expect("shortcut report mutex poisoned")
        .clone()
}

/// Drop every registration and rebuild it from current settings.
///
/// Rebuilding wholesale rather than diffing keeps this correct when a user
/// swaps two shortcuts: unregistering everything first means the intermediate
/// state can never collide with itself.
pub fn reregister(app: &AppHandle) {
    let manager = app.global_shortcut();
    if let Err(err) = manager.unregister_all() {
        tracing::warn!(%err, "could not clear existing shortcuts");
    }

    let workflows = app.state::<SettingsState>().snapshot().workflows;
    let mut reports = Vec::new();

    for workflow in workflows.iter().filter(|w| w.enabled) {
        let Some(accelerator) = workflow.shortcut.as_deref() else {
            continue;
        };

        let (registered, error) = match manager.register(accelerator) {
            Ok(()) => (true, None),
            Err(err) => {
                tracing::warn!(%accelerator, %err, "shortcut unavailable");
                (
                    false,
                    Some(format!(
                        "Bu kısayol kullanılamıyor, muhtemelen başka bir uygulama almış ({err})."
                    )),
                )
            }
        };

        reports.push(ShortcutReport {
            workflow_id: workflow.id.clone(),
            name: workflow.name.clone(),
            accelerator: accelerator.to_string(),
            registered,
            error,
        });
    }

    *app.state::<ShortcutState_>()
        .0
        .lock()
        .expect("shortcut report mutex poisoned") = reports.clone();

    let _ = app.emit(crate::EVENT_SHORTCUTS_CHANGED, reports);
}

/// The plugin instance, wired to dispatch the workflow bound to each key.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Fire on press only; otherwise every shortcut captures twice.
            if event.state() != ShortcutState::Pressed {
                return;
            }

            let accelerator = shortcut.into_string();
            let workflows = app.state::<SettingsState>().snapshot().workflows;
            let Some(workflow) = workflows
                .into_iter()
                .find(|w| w.enabled && w.shortcut.as_deref() == Some(accelerator.as_str()))
            else {
                tracing::warn!(%accelerator, "shortcut fired with no matching workflow");
                return;
            };

            crate::run_in_background(app, workflow.method);
        })
        .build()
}
