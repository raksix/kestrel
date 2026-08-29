//! Raising the selection overlay above the Dock and the menu bar.
//!
//! The overlay is sized to the whole display, but on macOS that is not enough.
//! The Dock and the menu bar float at window levels above `always_on_top`
//! (which is `NSFloatingWindowLevel`, 3), so a full-screen overlay still has the
//! Dock sitting on top of it — the selection looks like it stops short of the
//! bottom of the screen, and clicking there hits the Dock instead of the
//! overlay.
//!
//! Hiding the Dock instead was the alternative, and it is worse: it needs
//! accessibility permission or a presentation-options change that outlives a
//! crash, and it moves other windows around while it animates. Raising our own
//! window changes nothing outside Kestrel and needs no permission.
//!
//! **This is the only `unsafe` in the desktop shell.** The crate denies unsafe
//! everywhere else; the allow is on this module alone so that the exception
//! stays visible in review. The two obligations are: the pointer comes straight
//! from Tauri's `ns_window()` and is only used while the window it came from is
//! alive, and the call happens on the main thread, which is where every window
//! operation in this app already runs.

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow};
    use tauri::WebviewWindow;

    /// Put `window` above the Dock and the menu bar.
    pub fn raise_above_shell(window: &WebviewWindow) {
        let Ok(pointer) = window.ns_window() else {
            tracing::warn!("no NSWindow for the overlay; it will sit under the Dock");
            return;
        };
        if pointer.is_null() {
            return;
        }

        // SAFETY: `ns_window()` hands back the `NSWindow` backing this
        // `WebviewWindow`, which outlives this call because `window` is
        // borrowed for it. `setLevel:` and `level` are plain property
        // accessors with no ownership implications.
        let applied = unsafe {
            let ns_window: &NSWindow = &*pointer.cast();
            ns_window.setLevel(NSScreenSaverWindowLevel);
            ns_window.level()
        };

        // Read it back rather than assuming. This started as a bug report of
        // "sometimes the Dock still covers it", and a setter that silently
        // does not stick is exactly the kind of thing that turns into a
        // report like that instead of a log line.
        if applied != NSScreenSaverWindowLevel {
            tracing::warn!(
                applied,
                wanted = NSScreenSaverWindowLevel,
                "the overlay window level did not stick; the Dock may cover it"
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use tauri::WebviewWindow;

    /// A no-op off macOS.
    ///
    /// Windows and the common Linux compositors put an always-on-top window
    /// above the taskbar or panel already, so there is nothing to raise.
    pub fn raise_above_shell(_window: &WebviewWindow) {}
}

pub use imp::raise_above_shell;
