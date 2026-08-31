//! The region selection overlay and the window/display picker.
//!
//! Every window here is created through `run_on_main_thread`. macOS requires
//! window creation on the main thread, and Tauri's builder called from a worker
//! blocks until the event loop next wakes — which, if nothing else is
//! happening, is whenever the user next moves the mouse. That produced a
//! multi-minute delay between pressing the shortcut and the overlay appearing.
//!
//! The overlay is one borderless, transparent, always-on-top window per
//! display. It is deliberately *not* painted with a screenshot: the frozen
//! frames live in Rust (see `kestrel_capture::frame`) and selections are
//! cropped out of them, so nothing the overlay draws can ever end up in the
//! capture, and no multi-megabyte image has to cross the IPC boundary.

use std::sync::Mutex;

use kestrel_capture::{CaptureBackend, FrozenFrames, Region};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label prefix for overlay windows, one per display.
const OVERLAY_PREFIX: &str = "overlay-";
/// Label of the window/display picker.
pub const PICKER_LABEL: &str = "picker";

/// What the selection being made is for.
///
/// The two modes look almost the same and share every gesture, but they differ
/// in one decision that cannot be made later: a screenshot needs the screen
/// frozen at the moment the overlay opened, and a recording must not freeze it,
/// because the user is about to record whatever moves inside the rectangle they
/// are still drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Capture,
    /// `gif` chooses between an animated GIF and a video, exactly as the
    /// non-region recording methods do.
    Record { gif: bool },
}

#[derive(Default)]
pub struct OverlayState {
    /// The frozen screen, for a capture. `None` in record mode.
    frames: Mutex<Option<FrozenFrames>>,
    /// `Some` while an overlay session is open. This — not `frames` — is what
    /// says a session exists, because a recording session has no frames.
    mode: Mutex<Option<OverlayMode>>,
}

impl OverlayState {
    pub fn is_active(&self) -> bool {
        self.mode
            .lock()
            .expect("overlay mutex poisoned")
            .is_some()
    }

    /// What the open session is for, if one is open.
    pub fn mode(&self) -> Option<OverlayMode> {
        *self.mode.lock().expect("overlay mutex poisoned")
    }

    fn store(&self, mode: OverlayMode, frames: Option<FrozenFrames>) {
        *self.frames.lock().expect("overlay mutex poisoned") = frames;
        *self.mode.lock().expect("overlay mutex poisoned") = Some(mode);
    }

    fn take(&self) -> Option<FrozenFrames> {
        *self.mode.lock().expect("overlay mutex poisoned") = None;
        self.frames.lock().expect("overlay mutex poisoned").take()
    }
}

/// Freeze every display and raise a selection overlay on each one.
pub fn begin_region_selection(app: &AppHandle) -> Result<(), String> {
    begin(app, OverlayMode::Capture)
}

/// Raise the same overlay to choose the rectangle a recording will cover.
pub fn begin_region_recording(app: &AppHandle, gif: bool) -> Result<(), String> {
    begin(app, OverlayMode::Record { gif })
}

fn begin(app: &AppHandle, mode: OverlayMode) -> Result<(), String> {
    // Re-entrancy guard: hammering the shortcut must not stack overlays.
    if app.state::<OverlayState>().is_active() {
        focus_existing_overlays(app);
        return Ok(());
    }

    let recording = matches!(mode, OverlayMode::Record { .. });

    // Record mode deliberately does not freeze. A frozen backdrop would show
    // the user a still of a screen they are about to record live, and worse,
    // hide the very motion they are trying to frame.
    let backend = kestrel_capture::backend();
    let displays = if recording {
        backend.displays().map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let frames = if recording {
        None
    } else {
        Some(backend.freeze().map_err(|e| e.to_string())?)
    };
    let displays = match &frames {
        Some(frames) => frames.displays(),
        None => displays,
    };

    // Stage each display's frozen frame where the webview can load it. The
    // overlay paints this rather than being a transparent hole onto the live
    // screen, for two reasons that both came from using it:
    //
    // - The dim then covers *everything*, including the Dock, which is what
    //   ShareX does and what people expect a capture overlay to look like.
    // - Blur and pixelate can only redact pixels they can see. On a transparent
    //   canvas they sampled nothing and appeared to do nothing, which read as
    //   "it only pixelates the shapes I drew".
    //
    // It goes to disk and is loaded over the asset protocol, not pushed through
    // IPC: a full-screen frame is megabytes, and that is the same reason the
    // editor stages its base image instead of sending a data URL.
    let staged = match &frames {
        Some(frames) => stage_frames(app, frames),
        None => std::collections::HashMap::new(),
    };
    app.state::<OverlayState>().store(mode, frames);

    // Window creation has to happen on the main thread; queueing it also
    // returns immediately, so the caller is never blocked on the event loop.
    let handle = app.clone();
    app.run_on_main_thread(move || {
        // Named `screen`, not `display`: `tracing`'s macros resolve a bare
        // `display` identifier to their own formatting helper.
        for screen in &displays {
            let label = format!("{OVERLAY_PREFIX}{}", screen.id);
            if handle.get_webview_window(&label).is_some() {
                continue;
            }

            let frame = staged
                .get(&screen.id)
                .map(|path| urlencoding_lite(&path.to_string_lossy()))
                .unwrap_or_default();

            let (overlay_mode, gif) = match mode {
                OverlayMode::Capture => ("capture", "0"),
                OverlayMode::Record { gif } => ("record", if gif { "1" } else { "0" }),
            };

            let url = WebviewUrl::App(
                format!(
                    "index.html?view=overlay&mode={overlay_mode}&gif={gif}\
                     &display={}&x={}&y={}&w={}&h={}&s={}&frame={frame}",
                    screen.id,
                    screen.region.x,
                    screen.region.y,
                    screen.region.width,
                    screen.region.height,
                    screen.scale_factor
                )
                .into(),
            );

            let built = WebviewWindowBuilder::new(&handle, &label, url)
                .title("Kestrel selection")
                .position(screen.region.x as f64, screen.region.y as f64)
                .inner_size(screen.region.width as f64, screen.region.height as f64)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                // Selection must still work if the user switches desktop/Space.
                .visible_on_all_workspaces(true)
                .resizable(false)
                .shadow(false)
                .visible(true)
                .focused(screen.is_primary)
                .build();

            match built {
                // The overlay covers the whole display, but on macOS the Dock
                // and the menu bar float above an always-on-top window. Without
                // this the selection looks like it stops short of the bottom of
                // the screen and clicks there hit the Dock.
                Ok(window) => crate::window_level::raise_above_shell(&window),
                Err(err) => {
                    tracing::error!(%err, screen_id = screen.id, "could not open the selection overlay");
                    close_overlays(&handle);
                    handle.state::<OverlayState>().take();
                    return;
                }
            }
        }
    })
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn focus_existing_overlays(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        for window in handle.webview_windows().values() {
            if window.label().starts_with(OVERLAY_PREFIX) {
                let _ = window.set_focus();
            }
        }
    });
}

/// Close every overlay window.
///
/// Closing is a main-thread operation too, and this runs from a command
/// handler, so it is queued rather than called directly.
/// Write each display's frozen frame to the cache directory.
///
/// Best effort: a display whose frame cannot be staged simply has no backdrop,
/// and the overlay falls back to being transparent over the live screen. That
/// is worse than the real thing but far better than refusing to open a
/// selection because a temp file could not be written.
fn stage_frames(
    app: &AppHandle,
    frames: &FrozenFrames,
) -> std::collections::HashMap<u32, std::path::PathBuf> {
    let mut staged = std::collections::HashMap::new();

    let Some(dir) = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("overlay"))
    else {
        return staged;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return staged;
    }

    for info in frames.displays() {
        // Cropping the whole display out of the frames is how we get one image
        // per screen without the frame module having to expose its internals.
        let Ok(capture) = frames.crop(info.region) else {
            continue;
        };
        // A per-display fixed name: the previous selection's file is dead the
        // moment a new one starts, so the cache cannot grow without bound.
        let path = dir.join(format!("display-{}.png", info.id));
        if capture.image.save(&path).is_ok() {
            staged.insert(info.id, path);
        }
    }

    staged
}

/// Percent-encode the few characters that would break a query string.
///
/// A full URL encoder would be the wrong tool: this value is a filesystem path
/// going into a query parameter we control at both ends, and the characters
/// that actually matter are the delimiters.
fn urlencoding_lite(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            '%' => "%25".to_string(),
            '+' => "%2B".to_string(),
            other => other.to_string(),
        })
        .collect()
}

pub fn close_overlays(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        for window in handle.webview_windows().values() {
            if window.label().starts_with(OVERLAY_PREFIX) {
                let _ = window.close();
            }
        }
    });
}

/// End the session, returning the frames so a selection can be cropped.
pub fn finish(app: &AppHandle) -> Option<FrozenFrames> {
    close_overlays(app);
    app.state::<OverlayState>().take()
}

/// Crop a committed selection out of the frozen frames.
pub fn crop_selection(app: &AppHandle, region: Region) -> Result<kestrel_capture::Capture, String> {
    let frames = finish(app).ok_or("no selection is in progress")?;
    frames.crop(region).map_err(|e| e.to_string())
}

/// End a recording selection session, closing the overlays.
///
/// Separate from `crop_selection` because there is nothing to crop: the
/// rectangle is handed to the recorder, not to an image.
pub fn finish_recording_selection(app: &AppHandle) -> Result<bool, String> {
    let mode = app.state::<OverlayState>().mode();
    finish(app);
    match mode {
        Some(OverlayMode::Record { gif }) => Ok(gif),
        Some(OverlayMode::Capture) => Err("bu seçim bir ekran görüntüsü içindi".into()),
        None => Err("no selection is in progress".into()),
    }
}

/// A small crop of the frozen screen around a point, for the magnifier.
///
/// This is the one thing the overlay needs pixels for, and it takes only the
/// pixels it needs. The whole point of keeping the frames in Rust is that a
/// full-screen image never crosses the IPC boundary; a 33x33 patch is about a
/// kilobyte, which is a different thing entirely.
///
/// Returns the patch as a PNG data URL and the exact colour at the centre, so
/// the overlay does not have to read it back out of the image and get the
/// rounding wrong.
pub fn sample(app: &AppHandle, x: i32, y: i32, radius: u32) -> Result<Sample, String> {
    let state = app.state::<OverlayState>();
    let guard = state.frames.lock().expect("overlay mutex poisoned");
    // Only a capture session has frames; the magnifier reads the frozen screen,
    // so it has nothing to show while a recording region is being chosen.
    let frames = guard
        .as_ref()
        .ok_or("no frozen selection is in progress")?;

    // An odd width, so there is a single centre pixel to point the crosshair at.
    let radius = radius.clamp(1, 32) as i32;
    let side = (radius * 2 + 1) as u32;

    let capture = frames
        .crop(Region::new(x - radius, y - radius, side, side))
        .map_err(|e| e.to_string())?;

    // The crop is clamped to the display, so near an edge it comes back smaller
    // and the centre is no longer at `radius`. Ask the capture where the point
    // actually landed rather than assuming.
    let centre_x = (x - capture.region.x).clamp(0, capture.image.width() as i32 - 1) as u32;
    let centre_y = (y - capture.region.y).clamp(0, capture.image.height() as i32 - 1) as u32;
    let pixel = capture.image.get_pixel(centre_x, centre_y);

    Ok(Sample {
        image: crate::capture_service::encode_preview(&capture.image).map_err(|e| e.to_string())?,
        width: capture.image.width(),
        height: capture.image.height(),
        centre_x,
        centre_y,
        hex: format!("#{:02X}{:02X}{:02X}", pixel[0], pixel[1], pixel[2]),
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// The patch as a data URL.
    pub image: String,
    pub width: u32,
    pub height: u32,
    /// Where the sampled point sits inside the patch. Not always the middle:
    /// near a screen edge the crop is clipped.
    pub centre_x: u32,
    pub centre_y: u32,
    pub hex: String,
}

/// Open the window / display picker.
pub fn open_picker(app: &AppHandle, tab: &str) -> Result<(), String> {
    let tab = tab.to_string();
    let handle = app.clone();

    app.run_on_main_thread(move || {
        if let Some(existing) = handle.get_webview_window(PICKER_LABEL) {
            let _ = existing.set_focus();
            return;
        }

        let url = WebviewUrl::App(format!("index.html?view=picker&tab={tab}").into());
        let built = WebviewWindowBuilder::new(&handle, PICKER_LABEL, url)
            .title("Yakalanacak pencereyi seç")
            .inner_size(880.0, 560.0)
            .min_inner_size(520.0, 360.0)
            .center()
            .always_on_top(true)
            .resizable(true)
            .build();

        if let Err(err) = built {
            tracing::error!(%err, "could not open the picker");
        }
    })
    .map_err(|e| e.to_string())
}

pub fn close_picker(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(PICKER_LABEL) {
            let _ = window.close();
        }
    });
}
