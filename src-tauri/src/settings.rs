//! Persisted application settings.
//!
//! Plain JSON on disk, in the platform's config directory, exactly like
//! ShareX — so a user can read it, edit it, diff it or keep it in a dotfiles
//! repo. Secrets never live here; those belong in the OS keychain.

use std::path::PathBuf;
use std::sync::Mutex;

use kestrel_core::model::{default_workflows, TaskSettings, Workflow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Bumped when the on-disk shape changes, so old files can be migrated
    /// rather than silently discarded.
    pub version: u32,
    pub workflows: Vec<Workflow>,
    pub defaults: TaskSettings,
    /// Destination used when a workflow does not name one. Lives here rather
    /// than in memory so the choice survives a restart.
    pub default_destination: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            workflows: default_workflows(),
            defaults: TaskSettings::default(),
            default_destination: None,
        }
    }
}

impl AppSettings {
    pub fn workflow(&self, id: &str) -> Option<&Workflow> {
        self.workflows.iter().find(|w| w.id == id)
    }

    /// Which other workflow already owns this accelerator, if any.
    pub fn shortcut_conflict(&self, accelerator: &str, ignoring: &str) -> Option<&Workflow> {
        self.workflows
            .iter()
            .find(|w| w.id != ignoring && w.shortcut.as_deref() == Some(accelerator))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("no config directory is available on this system")]
    NoConfigDir,
    #[error("no workflow with id {0}")]
    UnknownWorkflow(String),
    #[error("{accelerator} is already used by \"{owner}\"")]
    ShortcutConflict { accelerator: String, owner: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings file is not valid json: {0}")]
    Parse(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or(SettingsError::NoConfigDir)?
        .join("Kestrel"))
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("settings.json"))
}

/// Load settings, falling back to defaults when the file is missing.
///
/// A corrupt file is backed up rather than deleted — losing a user's whole
/// configuration to one bad character would be unforgivable.
pub fn load() -> AppSettings {
    let Ok(path) = settings_path() else {
        return AppSettings::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AppSettings::default();
    };

    match serde_json::from_str::<AppSettings>(&text) {
        Ok(mut settings) => {
            // Persist the result, or the same migration runs on every launch
            // and the file on disk never matches what the app is using.
            if migrate(&mut settings) {
                if let Err(err) = save(&settings) {
                    tracing::warn!(%err, "could not persist migrated settings");
                }
            }
            settings
        }
        Err(err) => {
            tracing::error!(%err, "settings file is corrupt, backing it up and starting fresh");
            let backup = path.with_extension("json.corrupt");
            let _ = std::fs::rename(&path, backup);
            AppSettings::default()
        }
    }
}

/// Bring an older settings file up to date.
///
/// The only migration so far: early builds shipped defaults bound to
/// Cmd+Shift+3/4/5, which macOS keeps for itself. Those shortcuts look fine in
/// the UI but never fire, so anyone who ran those builds is silently left with
/// dead keys. Rebind them to the current default rather than making the user
/// work out what went wrong.
/// Bring an older settings file up to date. Returns whether anything changed.
fn migrate(settings: &mut AppSettings) -> bool {
    let defaults = default_workflows();
    let mut changed = false;

    // Accelerators already spoken for, so a rebind cannot land on top of one.
    // Without this, moving Cmd+Shift+4 onto the current default for its
    // workflow collides with whichever workflow already holds that default —
    // and one of the two ends up with a shortcut that silently never fires.
    let mut taken: Vec<String> = settings
        .workflows
        .iter()
        .filter_map(|w| w.shortcut.clone())
        .filter(|a| kestrel_core::model::system_reserved(a).is_none())
        .collect();

    for workflow in settings.workflows.iter_mut() {
        let Some(accelerator) = workflow.shortcut.as_deref() else {
            continue;
        };
        let Some(owner) = kestrel_core::model::system_reserved(accelerator) else {
            continue;
        };

        let replacement = kestrel_core::model::fallback_shortcuts(&workflow.id)
            .iter()
            .copied()
            // Do not swap one reserved shortcut for another, and do not take
            // one another workflow is already using.
            .find(|a| {
                kestrel_core::model::system_reserved(a).is_none() && !taken.iter().any(|t| t == a)
            })
            .map(str::to_string);

        tracing::info!(
            workflow = %workflow.id,
            from = accelerator,
            to = replacement.as_deref().unwrap_or("(kaldırıldı)"),
            %owner,
            "rebinding a shortcut the operating system reserves"
        );
        if let Some(replacement) = &replacement {
            taken.push(replacement.clone());
        }
        workflow.shortcut = replacement;
        changed = true;
    }

    // Workflows added after the user's settings file was written.
    for default in defaults {
        if !settings.workflows.iter().any(|w| w.id == default.id) {
            settings.workflows.push(default);
            changed = true;
        }
    }
    changed
}

/// Write settings atomically so an interrupted save cannot truncate the file.
pub fn save(settings: &AppSettings) -> Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let text = serde_json::to_string_pretty(settings)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// Shared, mutable settings for the running app.
pub struct SettingsState(pub Mutex<AppSettings>);

impl SettingsState {
    pub fn new() -> Self {
        Self(Mutex::new(load()))
    }

    pub fn snapshot(&self) -> AppSettings {
        self.0.lock().expect("settings mutex poisoned").clone()
    }

    /// Mutate and persist in one step, so an in-memory change can never drift
    /// from what is on disk.
    pub fn update<F, T>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut AppSettings) -> Result<T>,
    {
        let mut guard = self.0.lock().expect("settings mutex poisoned");
        let outcome = mutate(&mut guard)?;
        save(&guard)?;
        Ok(outcome)
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_ship_with_workflows_and_shortcuts() {
        let settings = AppSettings::default();
        assert!(!settings.workflows.is_empty());
        assert!(settings.workflows.iter().all(|w| w.shortcut.is_some()));
        assert!(
            settings.workflows.iter().all(|w| w.enabled),
            "every default workflow is enabled"
        );
    }

    #[test]
    fn conflict_detection_ignores_the_workflow_being_edited() {
        let settings = AppSettings::default();
        let first = settings.workflows[0].clone();
        let accelerator = first.shortcut.clone().unwrap();

        // Rebinding a workflow to the shortcut it already owns is not a clash.
        assert!(settings
            .shortcut_conflict(&accelerator, &first.id)
            .is_none());

        // Another workflow taking it is.
        let other = &settings.workflows[1];
        let clash = settings.shortcut_conflict(&accelerator, &other.id);
        assert_eq!(clash.map(|w| w.id.as_str()), Some(first.id.as_str()));
    }

    /// `shortcuts::reregister` only binds enabled workflows, so disabling one
    /// must remove it from that set.
    #[test]
    fn disabling_a_workflow_removes_it_from_the_bindable_set() {
        let bindable = |s: &AppSettings| {
            s.workflows
                .iter()
                .filter(|w| w.enabled && w.shortcut.is_some())
                .count()
        };

        let mut settings = AppSettings::default();
        let before = bindable(&settings);
        settings.workflows[0].enabled = false;
        assert_eq!(bindable(&settings), before - 1);
    }

    fn shortcuts_of(settings: &AppSettings) -> Vec<String> {
        settings
            .workflows
            .iter()
            .filter_map(|w| w.shortcut.clone())
            .collect()
    }

    #[test]
    fn migration_never_leaves_two_workflows_on_one_shortcut() {
        // The failure this guards against: a settings file where one workflow
        // already holds the accelerator another is about to be moved onto. The
        // loser registers second, fails, and its shortcut silently never fires.
        let mut settings = AppSettings::default();
        settings.workflows[0].shortcut = Some("CmdOrCtrl+Shift+3".into());
        settings.workflows[1].shortcut = Some("CmdOrCtrl+Shift+4".into());

        migrate(&mut settings);

        let mut bound = shortcuts_of(&settings);
        let count = bound.len();
        bound.sort();
        bound.dedup();
        assert_eq!(bound.len(), count, "shortcuts must stay unique: {bound:?}");
    }

    #[test]
    fn migration_reports_whether_it_changed_anything() {
        let mut untouched = AppSettings::default();
        assert!(
            !migrate(&mut untouched),
            "current defaults need no migration"
        );

        let mut stale = AppSettings::default();
        stale.workflows[0].shortcut = Some("CmdOrCtrl+Shift+4".into());
        assert!(migrate(&mut stale));
    }

    #[test]
    fn migration_rebinds_shortcuts_the_os_reserves() {
        let mut settings = AppSettings::default();
        // What the first release shipped, and what macOS swallows.
        settings.workflows[0].shortcut = Some("CmdOrCtrl+Shift+4".into());

        migrate(&mut settings);

        let bound = settings.workflows[0].shortcut.as_deref();
        assert_ne!(bound, Some("CmdOrCtrl+Shift+4"));
        if let Some(bound) = bound {
            assert_eq!(
                kestrel_core::model::system_reserved(bound),
                None,
                "must not swap one reserved shortcut for another"
            );
        }
    }

    #[test]
    fn migration_leaves_a_usable_shortcut_alone() {
        let mut settings = AppSettings::default();
        settings.workflows[0].shortcut = Some("CmdOrCtrl+Alt+K".into());

        migrate(&mut settings);

        assert_eq!(
            settings.workflows[0].shortcut.as_deref(),
            Some("CmdOrCtrl+Alt+K"),
            "a shortcut the user chose must survive"
        );
    }

    #[test]
    fn migration_adds_workflows_introduced_later() {
        let mut settings = AppSettings::default();
        let removed = settings.workflows.pop().expect("has workflows");

        migrate(&mut settings);

        assert!(settings.workflows.iter().any(|w| w.id == removed.id));
    }

    #[test]
    fn settings_survive_a_json_round_trip() {
        let mut settings = AppSettings::default();
        settings.workflows[0].shortcut = Some("CmdOrCtrl+Alt+9".into());

        let text = serde_json::to_string(&settings).unwrap();
        let back: AppSettings = serde_json::from_str(&text).unwrap();

        assert_eq!(back.version, settings.version);
        assert_eq!(
            back.workflows[0].shortcut.as_deref(),
            Some("CmdOrCtrl+Alt+9")
        );
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // A settings file written by an older build must still load.
        let back: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!back.workflows.is_empty());
    }
}
