//! Watch folder, as ShareX's.
//!
//! A directory that uploads whatever lands in it. Useful for anything that
//! writes files outside Kestrel — a game's screenshot key, a recorder, a
//! scanner.
//!
//! Polling rather than filesystem events, deliberately. The event APIs differ
//! on every platform, they fire mid-write about as often as after it, and the
//! interesting case here is a file appearing every few minutes, not every few
//! milliseconds. A poll every couple of seconds is cheaper to get right than
//! three platform backends plus a debounce.
//!
//! The hard part is not noticing the file — it is knowing when the *writer* has
//! finished with it. Uploading a half-written PNG produces a corrupt link that
//! looks like a Kestrel bug, so a file must hold the same size across two polls
//! before it is touched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// How often the directory is listed.
const POLL: Duration = Duration::from_secs(2);

/// Extensions worth uploading.
///
/// An allow-list rather than a deny-list: a watch folder is often somewhere
/// with other things in it, and uploading a stray `.DS_Store` or a partial
/// `.crdownload` would be worse than missing a format.
const EXTENSIONS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "mp4", "webm", "mkv", "txt",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchSettings {
    pub enabled: bool,
    pub directory: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub running: bool,
    pub directory: Option<String>,
    /// Files seen so far, so the UI can say the watcher is alive.
    pub handled: u64,
}

#[derive(Default)]
pub struct WatchState {
    inner: Mutex<Option<Running>>,
}

struct Running {
    directory: PathBuf,
    stop: Arc<AtomicBool>,
    handled: Arc<Mutex<u64>>,
}

impl WatchState {
    pub fn status(&self) -> WatchStatus {
        let guard = self.inner.lock().expect("watch mutex poisoned");
        match guard.as_ref() {
            Some(running) => WatchStatus {
                running: true,
                directory: Some(running.directory.to_string_lossy().into_owned()),
                handled: *running.handled.lock().expect("counter poisoned"),
            },
            None => WatchStatus {
                running: false,
                directory: None,
                handled: 0,
            },
        }
    }

    /// Stop the watcher, if one is running.
    ///
    /// The thread notices on its next tick rather than being killed, so it
    /// cannot be interrupted midway through an upload.
    pub fn stop(&self) {
        if let Some(running) = self.inner.lock().expect("watch mutex poisoned").take() {
            running.stop.store(true, Ordering::Relaxed);
            tracing::info!(directory = %running.directory.display(), "stopped watching");
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("{0} is not a directory")]
    NotADirectory(String),
    #[error("no directory has been chosen to watch")]
    NoDirectory,
}

pub type Result<T> = std::result::Result<T, WatchError>;

/// Start watching `directory`, replacing any existing watcher.
pub fn start(app: &AppHandle, directory: &Path) -> Result<WatchStatus> {
    if !directory.is_dir() {
        return Err(WatchError::NotADirectory(directory.display().to_string()));
    }

    let state = app.state::<WatchState>();
    state.stop();

    let stop = Arc::new(AtomicBool::new(false));
    let handled = Arc::new(Mutex::new(0u64));

    {
        let app = app.clone();
        let directory = directory.to_path_buf();
        let stop = stop.clone();
        let handled = handled.clone();
        std::thread::spawn(move || watch_loop(app, directory, stop, handled));
    }

    *state.inner.lock().expect("watch mutex poisoned") = Some(Running {
        directory: directory.to_path_buf(),
        stop,
        handled,
    });

    tracing::info!(directory = %directory.display(), "watching for new files");
    Ok(state.status())
}

fn watch_loop(app: AppHandle, directory: PathBuf, stop: Arc<AtomicBool>, handled: Arc<Mutex<u64>>) {
    // Everything already in the folder when watching starts is left alone.
    // Turning the watcher on should not upload a year of old screenshots.
    let mut known: HashMap<PathBuf, u64> = snapshot(&directory).into_iter().collect();

    // Sizes seen on the previous tick, for the settle check.
    let mut pending: HashMap<PathBuf, u64> = HashMap::new();

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        if stop.load(Ordering::Relaxed) {
            break;
        }

        for (path, size) in snapshot(&directory) {
            if known.contains_key(&path) {
                continue;
            }

            match pending.get(&path) {
                // Still growing: the writer has not finished. Uploading now
                // would produce a corrupt link that looks like a Kestrel bug.
                Some(previous) if *previous != size => {
                    pending.insert(path, size);
                }
                Some(_) => {
                    pending.remove(&path);
                    known.insert(path.clone(), size);
                    *handled.lock().expect("counter poisoned") += 1;
                    upload(&app, path);
                }
                None => {
                    pending.insert(path, size);
                }
            }
        }
    }
}

/// The uploadable files in `directory`, with their sizes.
///
/// Not recursive: a watch folder with a deep tree under it is far more likely
/// to be someone's Pictures directory than an intentional configuration.
fn snapshot(directory: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || !uploadable(&path) {
                return None;
            }
            Some((path, entry.metadata().ok()?.len()))
        })
        .collect()
}

fn uploadable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            EXTENSIONS.contains(&extension.as_str())
        })
        .unwrap_or(false)
}

fn upload(app: &AppHandle, path: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::uploads::upload_path(&app, &path, &crate::commands::UNATTENDED).await {
            Ok(uploaded) => tracing::info!(
                file = %path.display(),
                url = %uploaded.url,
                "uploaded a watched file"
            ),
            Err(err) => tracing::error!(%err, file = %path.display(), "watch upload failed"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kestrel-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_known_extensions_are_uploadable() {
        // An allow-list, because a watch folder usually has other things in it
        // and a stray .DS_Store must not become a public link.
        assert!(uploadable(Path::new("/a/shot.png")));
        assert!(uploadable(Path::new("/a/clip.MP4")));
        assert!(!uploadable(Path::new("/a/.DS_Store")));
        assert!(!uploadable(Path::new("/a/half.crdownload")));
        assert!(!uploadable(Path::new("/a/noextension")));
    }

    #[test]
    fn a_snapshot_lists_files_with_their_sizes() {
        let dir = dir("snapshot");
        std::fs::write(dir.join("a.png"), b"12345").unwrap();
        std::fs::write(dir.join("ignored.tmp"), b"12345").unwrap();

        let found = snapshot(&dir);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, 5);
    }

    #[test]
    fn a_snapshot_of_a_missing_directory_is_empty_rather_than_a_panic() {
        // The folder can be renamed or unmounted while the watcher runs.
        assert!(snapshot(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn subdirectories_are_not_descended_into() {
        // A watch folder with a deep tree under it is far more likely to be
        // someone's Pictures directory than an intentional configuration.
        let dir = dir("nested");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/deep.png"), b"x").unwrap();

        assert!(snapshot(&dir).is_empty());
    }

    #[test]
    fn a_fresh_state_reports_that_nothing_is_running() {
        let state = WatchState::default();
        let status = state.status();

        assert!(!status.running);
        assert_eq!(status.directory, None);
    }

    #[test]
    fn stopping_an_idle_watcher_is_harmless() {
        // The UI calls stop unconditionally when the toggle goes off.
        let state = WatchState::default();
        state.stop();
        state.stop();

        assert!(!state.status().running);
    }

    #[test]
    fn watching_is_off_until_it_is_switched_on() {
        // A watcher that started itself would upload files without being asked.
        let settings = WatchSettings::default();

        assert!(!settings.enabled);
        assert_eq!(settings.directory, None);
    }
}
