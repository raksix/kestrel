//! Scrolling capture.
//!
//! Press once to start, scroll the window yourself, press again to finish. The
//! frames are joined by `kestrel_capture::stitch`, which works out how far the
//! content moved between them.
//!
//! # Why it does not scroll for you
//!
//! ShareX drives the scroll with `WM_SCROLL`, which exists only on Windows.
//! The portable equivalent is a synthetic scroll event, and on macOS posting
//! one requires the Accessibility permission — a second, separate, scary
//! permission prompt, for a feature that works without it if the user does the
//! scrolling. So this version asks for nothing, and automatic scrolling can be
//! added later for the platforms where it is free.
//!
//! Saying that out loud matters more than it sounds: "scrolling capture" that
//! silently needs an ungranted permission would look broken rather than
//! unimplemented.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::RgbaImage;
use kestrel_capture::{stitch, CaptureBackend, Region};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// How often the region is re-captured while scrolling.
///
/// Fast enough that a normal scroll gesture leaves overlapping frames — the
/// stitcher needs at least `MIN_OVERLAP` rows in common — and slow enough that
/// a minute of scrolling does not fill memory with hundreds of screenfuls.
const INTERVAL: Duration = Duration::from_millis(250);

/// The most frames kept.
///
/// At four a second this is two minutes of scrolling. Past that the user is
/// almost certainly not making one long screenshot, and an unbounded buffer of
/// full-region frames is how an app runs a machine out of memory.
const MAX_FRAMES: usize = 480;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollStatus {
    pub active: bool,
    /// Frames captured so far, so the UI can show it is alive.
    pub frames: usize,
    /// True once the buffer is full and older frames would be dropped.
    pub full: bool,
}

#[derive(Default)]
pub struct ScrollState(Mutex<Option<Running>>);

struct Running {
    region: Region,
    stop: Arc<AtomicBool>,
    frames: Arc<Mutex<Vec<RgbaImage>>>,
}

impl ScrollState {
    pub fn status(&self) -> ScrollStatus {
        let guard = self.0.lock().expect("scroll mutex poisoned");
        match guard.as_ref() {
            Some(running) => {
                let frames = running.frames.lock().expect("frames mutex poisoned");
                ScrollStatus {
                    active: true,
                    frames: frames.len(),
                    full: frames.len() >= MAX_FRAMES,
                }
            }
            None => ScrollStatus::default(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.0.lock().expect("scroll mutex poisoned").is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScrollError {
    #[error("a scrolling capture is already running")]
    AlreadyRunning,
    #[error("no scrolling capture is running")]
    NotRunning,
    #[error("nothing was captured")]
    NothingCaptured,
    #[error("the frames could not be joined")]
    NotStitchable,
}

pub type Result<T> = std::result::Result<T, ScrollError>;

/// Begin capturing `region` on a timer.
pub fn start(app: &AppHandle, region: Region) -> Result<ScrollStatus> {
    let state = app.state::<ScrollState>();
    if state.is_active() {
        return Err(ScrollError::AlreadyRunning);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let frames: Arc<Mutex<Vec<RgbaImage>>> = Arc::new(Mutex::new(Vec::new()));

    {
        let stop = stop.clone();
        let frames = frames.clone();
        std::thread::spawn(move || {
            let backend = kestrel_capture::backend();
            while !stop.load(Ordering::Relaxed) {
                match backend.capture_region(region) {
                    Ok(capture) => {
                        let mut frames = frames.lock().expect("frames mutex poisoned");
                        // Stop collecting rather than dropping the oldest: the
                        // beginning of a long page is the part you cannot
                        // recover by scrolling back.
                        if frames.len() < MAX_FRAMES {
                            frames.push(capture.image);
                        }
                    }
                    // One failed grab is not worth ending the capture — the
                    // window may have been briefly occluded.
                    Err(err) => tracing::debug!(%err, "a scrolling frame was missed"),
                }
                std::thread::sleep(INTERVAL);
            }
        });
    }

    *state.0.lock().expect("scroll mutex poisoned") = Some(Running {
        region,
        stop,
        frames,
    });

    tracing::info!(?region, "scrolling capture started");
    Ok(state.status())
}

/// What a finished scrolling capture produced.
pub struct Scrolled {
    pub image: RgbaImage,
    pub region: Region,
    /// True when the frames did not all overlap, so content is missing.
    pub had_gap: bool,
}

/// Stop capturing and join what was collected.
pub fn finish(app: &AppHandle) -> Result<Scrolled> {
    let running = app
        .state::<ScrollState>()
        .0
        .lock()
        .expect("scroll mutex poisoned")
        .take()
        .ok_or(ScrollError::NotRunning)?;

    running.stop.store(true, Ordering::Relaxed);

    let frames = std::mem::take(&mut *running.frames.lock().expect("frames mutex poisoned"));
    if frames.is_empty() {
        return Err(ScrollError::NothingCaptured);
    }

    let joined =
        stitch::stitch(&frames, stitch::Trim::default()).ok_or(ScrollError::NotStitchable)?;

    tracing::info!(
        captured = frames.len(),
        used = joined.frames_used,
        had_gap = joined.had_gap,
        height = joined.image.height(),
        "scrolling capture joined"
    );

    Ok(Scrolled {
        image: joined.image,
        region: running.region,
        had_gap: joined.had_gap,
    })
}

/// Abandon a scrolling capture without producing anything.
pub fn cancel(app: &AppHandle) {
    if let Some(running) = app
        .state::<ScrollState>()
        .0
        .lock()
        .expect("scroll mutex poisoned")
        .take()
    {
        running.stop.store(true, Ordering::Relaxed);
        tracing::info!("scrolling capture cancelled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_is_not_running() {
        let state = ScrollState::default();
        let status = state.status();

        assert!(!state.is_active());
        assert!(!status.active);
        assert_eq!(status.frames, 0);
    }

    #[test]
    fn the_frame_cap_is_a_bounded_amount_of_memory() {
        // Four frames a second for two minutes. The point of the number is that
        // it exists: an unbounded buffer of full-screen frames is how an app
        // runs a machine out of memory while looking like it is working.
        let seconds = MAX_FRAMES as f64 * INTERVAL.as_secs_f64();

        assert!(seconds >= 60.0, "should allow a long page");
        assert!(seconds <= 180.0, "but not an unbounded one");
    }

    #[test]
    fn the_interval_leaves_frames_that_can_overlap() {
        // Faster than a scroll gesture completes, or consecutive frames share
        // nothing and every join is a gap.
        assert!(INTERVAL <= Duration::from_millis(400));
    }
}
