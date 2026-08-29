//! The recording loop: platform frames in, encoded video out.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kestrel_capture::Region;
use xcap::Frame;

use crate::ffmpeg::{self, FfmpegError, RecordSettings};

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error(transparent)]
    Ffmpeg(#[from] FfmpegError),
    #[error("no display to record")]
    NoDisplay,
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("a recording is already running")]
    AlreadyRecording,
    #[error("no recording is running")]
    NotRecording,
}

pub type Result<T> = std::result::Result<T, RecordError>;

/// A recording in progress.
pub struct Recording {
    encoder: Child,
    /// Cleared to stop the pump thread.
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    pump: Option<std::thread::JoinHandle<()>>,
    recorder: xcap::VideoRecorder,
    started: Instant,
    /// Time spent paused, so the reported duration matches the video.
    paused_for: Arc<Mutex<Duration>>,
    pub output: PathBuf,
}

impl Recording {
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// How long the finished video will be.
    pub fn elapsed(&self) -> Duration {
        let paused = *self.paused_for.lock().expect("pause mutex poisoned");
        self.started.elapsed().saturating_sub(paused)
    }

    /// Stop recording and wait for the encoder to finish writing the file.
    pub fn finish(mut self) -> Result<PathBuf> {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.recorder.stop();

        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }

        // Closing stdin is what tells ffmpeg the stream ended; without it the
        // process waits forever and the file is never finalised.
        drop(self.encoder.stdin.take());

        let output = self.encoder.wait_with_output()?;
        if !output.status.success() {
            return Err(FfmpegError::Failed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            }
            .into());
        }
        Ok(self.output)
    }

    /// Abandon the recording and delete the partial file.
    pub fn cancel(mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.recorder.stop();
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
        drop(self.encoder.stdin.take());
        let _ = self.encoder.kill();
        let _ = self.encoder.wait();
        let _ = std::fs::remove_file(&self.output);
    }
}

/// Start recording a display, optionally cropped to a region.
pub fn start(
    display_id: u32,
    region: Option<Region>,
    settings: &RecordSettings,
    output: &Path,
) -> Result<Recording> {
    let monitor = xcap::Monitor::all()
        .map_err(|e| RecordError::Capture(e.to_string()))?
        .into_iter()
        .find(|m| m.id().map(|id| id == display_id).unwrap_or(false))
        .ok_or(RecordError::NoDisplay)?;

    let (recorder, frames) = monitor
        .video_recorder()
        .map_err(|e| RecordError::Capture(e.to_string()))?;

    // The frame size is not known until the first frame arrives, and ffmpeg
    // needs it up front, so wait for one before spawning the encoder.
    recorder
        .start()
        .map_err(|e| RecordError::Capture(e.to_string()))?;
    let first = frames
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| RecordError::Capture("no frame arrived within five seconds".into()))?;

    let crop = region.map(|region| Crop::new(region, first.width, first.height));
    let (width, height) = match &crop {
        Some(crop) => (crop.width, crop.height),
        None => (first.width, first.height),
    };

    let binary = ffmpeg::find().ok_or(FfmpegError::NotFound)?;
    let mut encoder = Command::new(binary)
        .args(ffmpeg::encode_args(width, height, settings, output))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(FfmpegError::Spawn)?;

    let stdin = encoder
        .stdin
        .take()
        .ok_or_else(|| FfmpegError::Spawn(std::io::Error::other("ffmpeg has no stdin")))?;

    let running = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(AtomicBool::new(false));
    let paused_for = Arc::new(Mutex::new(Duration::ZERO));

    let pump = std::thread::spawn({
        let running = running.clone();
        let paused = paused.clone();
        let paused_for = paused_for.clone();
        let fps = settings.fps.max(1);
        move || pump_frames(stdin, frames, first, crop, fps, running, paused, paused_for)
    });

    Ok(Recording {
        encoder,
        running,
        paused,
        pump: Some(pump),
        recorder,
        started: Instant::now(),
        paused_for,
        output: output.to_path_buf(),
    })
}

/// Feed ffmpeg exactly `fps` frames per second.
///
/// Frames only arrive when something on screen changes, so a still screen would
/// otherwise produce a video far shorter than the time that passed. Repeating
/// the last frame on a fixed clock keeps the recording's length honest, which
/// matters as soon as there is audio to stay in sync with.
#[allow(clippy::too_many_arguments)]
fn pump_frames(
    mut stdin: std::process::ChildStdin,
    frames: Receiver<Frame>,
    first: Frame,
    crop: Option<Crop>,
    fps: u32,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    paused_for: Arc<Mutex<Duration>>,
) {
    let interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut latest = crop.as_ref().map_or(first.raw.clone(), |c| c.apply(&first));
    let mut next_tick = Instant::now();
    let mut pause_started: Option<Instant> = None;

    while running.load(Ordering::Relaxed) {
        // Drain everything queued and keep only the newest: falling behind
        // should drop frames, not accumulate a growing delay.
        while let Ok(frame) = frames.try_recv() {
            latest = crop.as_ref().map_or(frame.raw.clone(), |c| c.apply(&frame));
        }

        if paused.load(Ordering::Relaxed) {
            pause_started.get_or_insert_with(Instant::now);
            std::thread::sleep(interval);
            next_tick = Instant::now();
            continue;
        }

        if let Some(started) = pause_started.take() {
            *paused_for.lock().expect("pause mutex poisoned") += started.elapsed();
        }

        if stdin.write_all(&latest).is_err() {
            // ffmpeg exited — nothing useful is achieved by continuing.
            break;
        }

        next_tick += interval;
        match next_tick.checked_duration_since(Instant::now()) {
            Some(wait) => std::thread::sleep(wait),
            // Behind schedule: catch up rather than compounding the lag.
            None => next_tick = Instant::now(),
        }
    }

    let _ = stdin.flush();
}

/// A crop applied to every frame, in physical pixels.
#[derive(Debug, Clone, Copy)]
struct Crop {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Crop {
    /// Clamp a region to the frame it will be applied to.
    ///
    /// A region reaching past the display edge must yield a smaller crop, not
    /// an out-of-bounds read on every frame for the length of the recording.
    fn new(region: Region, frame_width: u32, frame_height: u32) -> Self {
        let x = region.x.max(0) as u32;
        let y = region.y.max(0) as u32;
        Self {
            x: x.min(frame_width.saturating_sub(1)),
            y: y.min(frame_height.saturating_sub(1)),
            width: region.width.min(frame_width.saturating_sub(x)).max(1),
            height: region.height.min(frame_height.saturating_sub(y)).max(1),
        }
    }

    fn apply(&self, frame: &Frame) -> Vec<u8> {
        let stride = frame.width as usize * 4;
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 4);

        for row in 0..self.height {
            let start = (self.y + row) as usize * stride + self.x as usize * 4;
            let end = start + self.width as usize * 4;
            match frame.raw.get(start..end) {
                Some(slice) => out.extend_from_slice(slice),
                // A short frame would otherwise panic mid-recording; padding
                // keeps the stream the size ffmpeg was promised.
                None => out.resize(out.len() + self.width as usize * 4, 0),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame whose pixels encode their own coordinates.
    fn coded(width: u32, height: u32) -> Frame {
        let mut raw = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 0, 255]);
            }
        }
        Frame::new(width, height, raw)
    }

    #[test]
    fn a_crop_takes_pixels_from_the_right_place() {
        let frame = coded(100, 80);
        let crop = Crop::new(Region::new(10, 20, 30, 40), 100, 80);
        let out = crop.apply(&frame);

        assert_eq!(out.len(), 30 * 40 * 4);
        // First pixel of the crop is the frame's (10, 20).
        assert_eq!(&out[0..4], &[10, 20, 0, 255]);
    }

    #[test]
    fn a_crop_reaching_past_the_edge_is_clamped() {
        // Otherwise every frame for the length of the recording reads out of
        // bounds.
        let crop = Crop::new(Region::new(90, 70, 50, 50), 100, 80);
        assert_eq!((crop.width, crop.height), (10, 10));

        let out = crop.apply(&coded(100, 80));
        assert_eq!(out.len(), 10 * 10 * 4);
    }

    #[test]
    fn a_crop_with_a_negative_origin_starts_at_zero() {
        let crop = Crop::new(Region::new(-20, -5, 40, 40), 100, 80);
        assert_eq!((crop.x, crop.y), (0, 0));
    }

    #[test]
    fn a_crop_is_never_empty() {
        // A zero-sized crop would make ffmpeg reject the stream outright.
        let crop = Crop::new(Region::new(0, 0, 0, 0), 100, 80);
        assert!(crop.width >= 1 && crop.height >= 1);
    }

    #[test]
    fn a_short_frame_is_padded_rather_than_panicking() {
        // A truncated frame mid-recording must not take the app down, and the
        // byte count still has to match what ffmpeg was told to expect.
        let truncated = Frame::new(100, 80, vec![0u8; 100 * 40 * 4]);
        let crop = Crop::new(Region::new(0, 0, 100, 80), 100, 80);

        let out = crop.apply(&truncated);
        assert_eq!(out.len(), 100 * 80 * 4);
    }

    #[test]
    fn cropping_the_whole_frame_reproduces_it() {
        let frame = coded(16, 16);
        let crop = Crop::new(Region::new(0, 0, 16, 16), 16, 16);
        assert_eq!(crop.apply(&frame), frame.raw);
    }
}
