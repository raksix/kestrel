//! Global shortcut registration.
//!
//! Two things make this fiddlier than it looks:
//!
//! 1. A shortcut another application already owns simply cannot be registered.
//!    ShareX surfaces this in a startup dialog; Kestrel keeps a live report so
//!    the settings UI can show which bindings actually took effect and offer a
//!    rebind, instead of leaving the user pressing a dead key combination.
//!
//! 2. A `Shortcut` does not round-trip through a string. `"CmdOrCtrl+Shift+2"`
//!    parses fine but renders back as `"shift+super+Digit2"`, so matching a
//!    fired shortcut against stored accelerator *text* never succeeds. We keep
//!    the parsed values and compare those instead.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

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
    /// Set when the OS itself owns this combination. Registration still
    /// succeeds in that case, but the key press never reaches us — so this is
    /// the only way the user finds out why nothing happens.
    pub system_conflict: Option<String>,
}

#[derive(Default)]
pub struct ShortcutRegistry {
    reports: Mutex<Vec<ShortcutReport>>,
    /// Parsed shortcut → workflow id. The parsed value is the only reliable
    /// key; see the module docs.
    bindings: Mutex<Vec<(Shortcut, String)>>,
}

impl ShortcutRegistry {
    fn reports(&self) -> Vec<ShortcutReport> {
        self.reports.lock().expect("report mutex poisoned").clone()
    }

    fn workflow_for(&self, shortcut: &Shortcut) -> Option<String> {
        self.bindings
            .lock()
            .expect("binding mutex poisoned")
            .iter()
            .find(|(bound, _)| bound == shortcut)
            .map(|(_, id)| id.clone())
    }

    fn replace(&self, reports: Vec<ShortcutReport>, bindings: Vec<(Shortcut, String)>) {
        *self.reports.lock().expect("report mutex poisoned") = reports;
        *self.bindings.lock().expect("binding mutex poisoned") = bindings;
    }
}

pub fn report(app: &AppHandle) -> Vec<ShortcutReport> {
    app.state::<ShortcutRegistry>().reports()
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
    let mut bindings = Vec::new();

    for workflow in workflows.iter().filter(|w| w.enabled) {
        let Some(accelerator) = workflow.shortcut.as_deref() else {
            continue;
        };

        let parsed: Result<Shortcut, _> = accelerator.parse();
        let (registered, error, shortcut) = match parsed {
            Err(err) => {
                tracing::warn!(%accelerator, %err, "shortcut is not valid");
                (
                    false,
                    Some(format!("Bu kısayol çözümlenemedi: {err}")),
                    None,
                )
            }
            Ok(shortcut) => match manager.register(shortcut) {
                Ok(()) => (true, None, Some(shortcut)),
                Err(err) => {
                    tracing::warn!(%accelerator, %err, "shortcut unavailable");
                    (
                        false,
                        Some(format!(
                            "Kullanılamıyor, muhtemelen başka bir uygulama almış ({err})."
                        )),
                        None,
                    )
                }
            },
        };

        if let Some(shortcut) = shortcut {
            bindings.push((shortcut, workflow.id.clone()));
        }

        reports.push(ShortcutReport {
            workflow_id: workflow.id.clone(),
            name: workflow.name.clone(),
            accelerator: accelerator.to_string(),
            registered,
            error,
            system_conflict: kestrel_core::model::system_reserved(accelerator)
                .map(|owner| owner.to_string()),
        });
    }

    tracing::info!(
        registered = bindings.len(),
        total = reports.len(),
        "shortcuts rebuilt"
    );

    app.state::<ShortcutRegistry>()
        .replace(reports.clone(), bindings);
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

            let Some(workflow_id) = app.state::<ShortcutRegistry>().workflow_for(shortcut) else {
                tracing::warn!(?shortcut, "shortcut fired with no matching workflow");
                return;
            };

            let workflows = app.state::<SettingsState>().snapshot().workflows;
            let Some(workflow) = workflows.into_iter().find(|w| w.id == workflow_id) else {
                return;
            };

            tracing::debug!(workflow = %workflow.name, "shortcut fired");
            crate::run_in_background(app, workflow.method);
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this module's docs describe: a `Shortcut` does not survive a
    /// round trip through its own string form, so text comparison is unsound.
    #[test]
    fn accelerators_do_not_round_trip_as_strings() {
        let accelerator = "CmdOrCtrl+Shift+2";
        let shortcut: Shortcut = accelerator.parse().expect("should parse");

        assert_ne!(
            shortcut.into_string(),
            accelerator,
            "if this ever becomes equal, the value-based lookup is still correct"
        );
    }

    #[test]
    fn parsed_shortcuts_compare_by_value() {
        let a: Shortcut = "CmdOrCtrl+Shift+2".parse().unwrap();
        let b: Shortcut = "CmdOrCtrl+Shift+2".parse().unwrap();
        let c: Shortcut = "CmdOrCtrl+Shift+3".parse().unwrap();

        assert_eq!(a, b, "the same accelerator must match itself");
        assert_ne!(a, c);
    }

    /// A fallback that cannot be parsed is worse than no fallback: it looks
    /// like a safety net in the source and silently does nothing at runtime.
    #[test]
    fn every_fallback_shortcut_parses() {
        for workflow in kestrel_core::default_workflows() {
            for accelerator in kestrel_core::model::fallback_shortcuts(&workflow.id) {
                assert!(
                    accelerator.parse::<Shortcut>().is_ok(),
                    "fallback {accelerator} for {} does not parse",
                    workflow.id
                );
            }
        }
    }

    #[test]
    fn every_default_shortcut_parses() {
        for workflow in kestrel_core::default_workflows() {
            let Some(accelerator) = workflow.shortcut.as_deref() else {
                continue;
            };
            assert!(
                accelerator.parse::<Shortcut>().is_ok(),
                "default shortcut {accelerator} for {} must parse",
                workflow.id
            );
        }
    }
}
