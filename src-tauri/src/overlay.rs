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

#[derive(Default)]
pub struct OverlayState(pub Mutex<Option<FrozenFrames>>);

impl OverlayState {
    pub fn is_active(&self) -> bool {
        self.0
            .lock()
            .expect("overlay mutex poisoned")
            .as_ref()
            .is_some_and(|frames| !frames.is_empty())
    }

    fn store(&self, frames: FrozenFrames) {
        *self.0.lock().expect("overlay mutex poisoned") = Some(frames);
    }

    fn take(&self) -> Option<FrozenFrames> {
        self.0.lock().expect("overlay mutex poisoned").take()
    }
}

/// Freeze every display and raise a selection overlay on each one.
pub fn begin_region_selection(app: &AppHandle) -> Result<(), String> {
    // Re-entrancy guard: hammering the shortcut must not stack overlays.
    if app.state::<OverlayState>().is_active() {
        focus_existing_overlays(app);
        return Ok(());
    }

    let backend = kestrel_capture::backend();
    let frames = backend.freeze().map_err(|e| e.to_string())?;
    let displays = frames.displays();

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
    let staged = stage_frames(app, &frames);
    app.state::<OverlayState>().store(frames);

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

            let url = WebviewUrl::App(
                format!(
                    "index.html?view=overlay&display={}&x={}&y={}&w={}&h={}&s={}&frame={frame}",
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
                    handle.state::<OverlayState>().0.lock().ok().map(|mut s| s.take());
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
    let guard = state.0.lock().expect("overlay mutex poisoned");
    let frames = guard.as_ref().ok_or("no selection is in progress")?;

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
