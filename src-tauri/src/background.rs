//! Living in the background: closing to the tray, hiding the Dock icon, and
//! starting with the session.
//!
//! A capture tool is used by shortcut far more than by window. It has to be
//! *running* to answer one, so closing the window must not quit — otherwise
//! every shortcut after the first close silently does nothing, which reads as
//! the shortcuts being broken rather than the app being closed.
//!
//! All three behaviours are settings rather than assumptions. Quietly refusing
//! to quit is the kind of thing that makes people hunt for a process to kill,
//! so the tray says what closing does and the setting can be turned off.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundSettings {
    /// Closing the window hides it instead of quitting.
    pub close_to_tray: bool,
    /// Hide the Dock icon and live in the menu bar only.
    ///
    /// macOS only. Off by default: a window that has no Dock icon and no
    /// visible window is a window you cannot get back to without knowing about
    /// the menu bar, and that is a bad thing to decide for someone.
    pub menu_bar_only: bool,
    /// Start when the user logs in.
    pub launch_at_login: bool,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            // On by default, because the alternative is shortcuts that stop
            // working the first time the window is closed.
            close_to_tray: true,
            menu_bar_only: false,
            launch_at_login: false,
        }
    }
}

/// Apply the Dock-icon policy.
///
/// Only does anything on macOS; Windows and Linux have no equivalent of an
/// accessory app, and the tray already behaves the way people expect there.
pub fn apply_activation_policy(app: &AppHandle, menu_bar_only: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if menu_bar_only {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        if let Err(err) = app.set_activation_policy(policy) {
            tracing::warn!(%err, "could not change the dock icon policy");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, menu_bar_only);
    }
}

/// Turn launch-at-login on or off.
///
/// Reports failure rather than swallowing it: this writes to the OS's login
/// items, and a silent failure would leave the setting showing "on" while
/// nothing happens at the next login.
pub fn apply_launch_at_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|err| err.to_string())
}

/// Whether the OS currently has us as a login item.
///
/// Read back rather than trusted from settings: the user can remove a login
/// item in System Settings, and a checkbox that disagrees with the system is
/// worse than no checkbox.
pub fn launches_at_login(app: &AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Hide the main window instead of letting it close.
///
/// Returns whether the close was intercepted, so the caller can decide what to
/// do with the event.
pub fn hide_instead_of_closing(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };

    match window.hide() {
        Ok(()) => true,
        Err(err) => {
            // If it cannot be hidden, let it close. A window that refuses both
            // is a window the user cannot get rid of.
            tracing::warn!(%err, "could not hide the main window; letting it close");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_to_the_tray_is_the_default() {
        // The alternative is a shortcut that works once and then silently does
        // nothing, because the app quit when its window closed.
        assert!(BackgroundSettings::default().close_to_tray);
    }

    #[test]
    fn hiding_the_dock_icon_and_autostart_are_both_opt_in() {
        // Neither is a decision to make on someone's behalf: one hides the app
        // from where they would look for it, the other adds it to their login.
        let defaults = BackgroundSettings::default();

        assert!(!defaults.menu_bar_only);
        assert!(!defaults.launch_at_login);
    }

    #[test]
    fn settings_survive_a_json_round_trip() {
        let settings = BackgroundSettings {
            close_to_tray: false,
            menu_bar_only: true,
            launch_at_login: true,
        };
        let json = serde_json::to_string(&settings).unwrap();

        assert_eq!(
            serde_json::from_str::<BackgroundSettings>(&json).unwrap(),
            settings
        );
    }

    #[test]
    fn an_older_settings_file_gets_the_defaults() {
        // Settings files predate this section, so it has to load from nothing.
        let settings: BackgroundSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, BackgroundSettings::default());
    }
}
