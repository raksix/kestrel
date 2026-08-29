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

            let url = WebviewUrl::App(
                format!(
                    "index.html?view=overlay&display={}&x={}&y={}&w={}&h={}&s={}",
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

            if let Err(err) = built {
                tracing::error!(%err, screen_id = screen.id, "could not open the selection overlay");
                close_overlays(&handle);
                handle.state::<OverlayState>().0.lock().ok().map(|mut s| s.take());
                return;
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
