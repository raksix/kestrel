//! Recording state for the running app.
//!
//! A recording handle cannot be shared between threads. On macOS it owns an
//! `AVCaptureSession`, which is neither `Send` nor `Sync`, and Tauri's managed
//! state requires both. So one thread owns the recording for its whole life and
//! everything else talks to it over a channel; the app reads progress from a
//! small shared snapshot rather than from the handle itself.
//!
//! Exactly one recording runs at a time. Two encoders would compete for CPU
//! badly enough to drop frames in both, and the tray has one indicator, so a
//! second recording would be invisible anyway.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::Local;
use kestrel_record::{ffmpeg, Recording};
use serde::Serialize;

/// How often the owning thread refreshes the elapsed time.
const TICK: Duration = Duration::from_millis(250);

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub active: bool,
    pub paused: bool,
    /// Seconds of video recorded so far, excluding pauses.
    pub elapsed: u64,
    pub output: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Platform-appropriate install command, when it is missing.
    pub install_hint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordAppError {
    #[error(transparent)]
    Record(#[from] kestrel_record::recorder::RecordError),
    #[error("a recording is already running")]
    AlreadyRecording,
    #[error("no recording is running")]
    NotRecording,
    #[error("no display to record")]
    NoDisplay,
    #[error("the recording thread stopped unexpectedly")]
    ThreadGone,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RecordAppError>;

enum Command {
    Pause(bool),
    Stop(SyncSender<std::result::Result<PathBuf, String>>),
    Cancel,
}

/// Handle to the thread that owns a running recording.
struct Controller {
    commands: Sender<Command>,
    status: Arc<Mutex<RecordingStatus>>,
    /// The recorded frame size, kept so the finished file can be described in
    /// the history without decoding it back off disk.
    size: (u32, u32),
}

#[derive(Default)]
pub struct RecordState(Mutex<Option<Controller>>);

impl RecordState {
    pub fn status(&self) -> RecordingStatus {
        self.0
            .lock()
            .expect("record mutex poisoned")
            .as_ref()
            .map(|c| c.status.lock().expect("status mutex poisoned").clone())
            .unwrap_or_default()
    }

    pub fn is_active(&self) -> bool {
        self.0.lock().expect("record mutex poisoned").is_some()
    }
}

/// Where ffmpeg is, and what to tell the user if it is not there.
pub fn ffmpeg_status() -> FfmpegStatus {
    match ffmpeg::find() {
        Some(path) => FfmpegStatus {
            version: ffmpeg::version(&path).ok(),
            path: Some(path.to_string_lossy().into_owned()),
            available: true,
            install_hint: None,
        },
        None => FfmpegStatus {
            available: false,
            path: None,
            version: None,
            install_hint: Some(install_hint().to_string()),
        },
    }
}

/// Naming the package manager is more useful than "install ffmpeg".
fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "brew install ffmpeg"
    } else if cfg!(target_os = "windows") {
        "winget install ffmpeg"
    } else {
        "sudo apt install ffmpeg"
    }
}

/// Begin recording a display.
pub fn start(
    state: &RecordState,
    display_id: Option<u32>,
    region: Option<kestrel_capture::Region>,
    settings: &kestrel_record::RecordSettings,
    output_dir: &std::path::Path,
    filename_stem: &str,
) -> Result<RecordingStatus> {
    if state.0.lock().expect("record mutex poisoned").is_some() {
        return Err(RecordAppError::AlreadyRecording);
    }

    let display_id = match display_id {
        Some(id) => id,
        None => primary_display()?,
    };

    // The frame size, resolved here so the finished file can be described in
    // the history without decoding it back off disk. A region records its own
    // bounds; a whole display records the display's.
    let size = match region {
        Some(region) => (region.width, region.height),
        None => display_size(display_id).unwrap_or((0, 0)),
    };

    std::fs::create_dir_all(output_dir)?;
    let extension = match settings.format {
        kestrel_record::OutputFormat::Gif => "gif",
        kestrel_record::OutputFormat::Video => settings.codec.container(),
    };
    let output = unique_path(output_dir, filename_stem, extension);

    let status = Arc::new(Mutex::new(RecordingStatus {
        active: true,
        paused: false,
        elapsed: 0,
        output: Some(output.to_string_lossy().into_owned()),
    }));

    let (commands, receiver) = mpsc::channel();
    // Starting can fail, and the caller needs to know before it reports
    // success, so the thread reports back before entering its loop.
    let (ready, started) = mpsc::sync_channel(1);

    let settings = settings.clone();
    let thread_status = status.clone();
    let thread_output = output.clone();
    std::thread::spawn(move || {
        let recording = match kestrel_record::start(display_id, region, &settings, &thread_output) {
            Ok(recording) => {
                let _ = ready.send(Ok(()));
                recording
            }
            Err(err) => {
                let _ = ready.send(Err(err.to_string()));
                return;
            }
        };
        own_recording(recording, receiver, thread_status);
    });

    match started.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(message)) => {
            return Err(kestrel_record::recorder::RecordError::Capture(message).into())
        }
        Err(_) => return Err(RecordAppError::ThreadGone),
    }

    tracing::info!(path = %output.display(), "recording started");
    let snapshot = status.lock().expect("status mutex poisoned").clone();
    *state.0.lock().expect("record mutex poisoned") = Some(Controller {
        commands,
        status,
        size,
    });
    Ok(snapshot)
}

/// The loop that owns a recording until it is stopped.
fn own_recording(
    recording: Recording,
    commands: Receiver<Command>,
    status: Arc<Mutex<RecordingStatus>>,
) {
    loop {
        match commands.recv_timeout(TICK) {
            Ok(Command::Pause(paused)) => {
                recording.set_paused(paused);
                let mut status = status.lock().expect("status mutex poisoned");
                status.paused = paused;
            }
            Ok(Command::Stop(reply)) => {
                let _ = reply.send(recording.finish().map_err(|e| e.to_string()));
                return;
            }
            Ok(Command::Cancel) => {
                recording.cancel();
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut status = status.lock().expect("status mutex poisoned");
                status.elapsed = recording.elapsed().as_secs();
            }
            // Every sender is gone, so nothing can ever stop this recording.
            // Discarding the partial file is better than leaking the encoder.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                recording.cancel();
                return;
            }
        }
    }
}

/// The size of one display in *physical* pixels.
///
/// `DisplayInfo::region` is in logical points, and the recorder captures actual
/// pixels — so on a Retina panel the two differ by a factor of two. Recording
/// the logical size put a 3420x2224 video in the library labelled 1710x1112,
/// which is the sort of wrong that nobody notices until they rely on it.
///
/// `None` rather than an error: a missing size costs a "0 x 0" in the library,
/// which is not worth failing a recording over.
fn display_size(id: u32) -> Option<(u32, u32)> {
    use kestrel_capture::CaptureBackend;
    kestrel_capture::backend()
        .displays()
        .ok()?
        .into_iter()
        .find(|d| d.id == id)
        .map(|d| {
            let scale = if d.scale_factor > 0.0 {
                d.scale_factor
            } else {
                1.0
            };
            (
                (d.region.width as f32 * scale).round() as u32,
                (d.region.height as f32 * scale).round() as u32,
            )
        })
}

fn primary_display() -> Result<u32> {
    use kestrel_capture::CaptureBackend;
    let displays = kestrel_capture::backend()
        .displays()
        .map_err(|e| kestrel_record::recorder::RecordError::Capture(e.to_string()))?;
    displays
        .iter()
        .find(|d| d.is_primary)
        .or_else(|| displays.first())
        .map(|d| d.id)
        .ok_or(RecordAppError::NoDisplay)
}

/// Stop and finalise, returning the file that was written.
/// What a finished recording produced.
pub struct Finished {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub fn stop(state: &RecordState) -> Result<Finished> {
    let controller = state
        .0
        .lock()
        .expect("record mutex poisoned")
        .take()
        .ok_or(RecordAppError::NotRecording)?;

    let (reply, result) = mpsc::sync_channel(1);
    controller
        .commands
        .send(Command::Stop(reply))
        .map_err(|_| RecordAppError::ThreadGone)?;

    // Finalising waits for ffmpeg to flush and close the container, which for a
    // long clip is not instant.
    match result.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(path)) => Ok(Finished {
            path,
            width: controller.size.0,
            height: controller.size.1,
        }),
        Ok(Err(message)) => Err(kestrel_record::recorder::RecordError::Capture(message).into()),
        Err(_) => Err(RecordAppError::ThreadGone),
    }
}

/// Stop and throw the partial file away.
pub fn cancel(state: &RecordState) -> Result<()> {
    let controller = state
        .0
        .lock()
        .expect("record mutex poisoned")
        .take()
        .ok_or(RecordAppError::NotRecording)?;
    controller
        .commands
        .send(Command::Cancel)
        .map_err(|_| RecordAppError::ThreadGone)
}

pub fn set_paused(state: &RecordState, paused: bool) -> Result<RecordingStatus> {
    let guard = state.0.lock().expect("record mutex poisoned");
    let controller = guard.as_ref().ok_or(RecordAppError::NotRecording)?;
    controller
        .commands
        .send(Command::Pause(paused))
        .map_err(|_| RecordAppError::ThreadGone)?;

    let mut status = controller.status.lock().expect("status mutex poisoned");
    status.paused = paused;
    Ok(status.clone())
}

/// Never overwrite an existing recording.
fn unique_path(dir: &std::path::Path, stem: &str, extension: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{extension}"));
    if !candidate.exists() {
        return candidate;
    }
    for n in 2..10_000 {
        let candidate = dir.join(format!("{stem} ({n}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!(
        "{stem}-{}.{extension}",
        Local::now().timestamp_millis()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_state_reports_nothing_running() {
        let state = RecordState::default();
        let status = state.status();

        assert!(!status.active);
        assert!(!status.paused);
        assert_eq!(status.elapsed, 0);
        assert!(status.output.is_none());
        assert!(!state.is_active());
    }

    #[test]
    fn stopping_when_nothing_is_running_is_a_named_error() {
        let state = RecordState::default();
        assert!(matches!(stop(&state), Err(RecordAppError::NotRecording)));
        assert!(matches!(cancel(&state), Err(RecordAppError::NotRecording)));
        assert!(matches!(
            set_paused(&state, true),
            Err(RecordAppError::NotRecording)
        ));
    }

    #[test]
    fn the_install_hint_names_the_platform_package_manager() {
        // "Install ffmpeg" is not actionable; a command someone can paste is.
        let hint = install_hint();
        assert!(hint.contains("ffmpeg"));
        assert!(hint.split_whitespace().count() >= 2);
    }

    #[test]
    fn ffmpeg_status_is_self_consistent() {
        let status = ffmpeg_status();
        if status.available {
            assert!(status.path.is_some());
            assert!(status.install_hint.is_none());
        } else {
            assert!(status.path.is_none());
            assert!(status.install_hint.is_some(), "say how to fix it");
        }
    }

    #[test]
    fn recordings_never_overwrite_each_other() {
        let dir = std::env::temp_dir().join(format!("kestrel-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let first = unique_path(&dir, "clip", "mp4");
        assert_eq!(first.file_name().unwrap(), "clip.mp4");
        std::fs::write(&first, b"x").unwrap();

        let second = unique_path(&dir, "clip", "mp4");
        assert_eq!(second.file_name().unwrap(), "clip (2).mp4");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole reason for the owning thread: the state Tauri manages has to
    /// be `Send + Sync`, and a recording handle is not.
    #[test]
    fn the_managed_state_is_shareable_between_threads() {
        fn assert_shareable<T: Send + Sync + 'static>() {}
        assert_shareable::<RecordState>();
    }
}
