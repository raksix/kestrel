//! Platform capture permissions.
//!
//! macOS gates all screen capture behind the Screen Recording TCC permission.
//! Without it the OS does not return an error — it silently returns a desktop
//! wallpaper image and hides every window from enumeration. That silent
//! degradation is why this module exists: we check up front and tell the user,
//! rather than letting them wonder why window capture "does nothing".

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    /// The platform has granted access.
    Granted,
    /// The platform requires permission and the user has not granted it.
    Denied,
    /// This platform does not gate screen capture behind a permission.
    NotRequired,
}

impl PermissionStatus {
    pub fn is_usable(self) -> bool {
        matches!(
            self,
            PermissionStatus::Granted | PermissionStatus::NotRequired
        )
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::PermissionStatus;

    // The only FFI in the codebase. CoreGraphics exposes no Rust binding for
    // these two, and every crate that wraps them is a thin shim over exactly
    // this.
    #[allow(unsafe_code)]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn status() -> PermissionStatus {
        // Safety: both calls take no arguments, return a plain bool, and are
        // safe to call from any thread.
        #[allow(unsafe_code)]
        let granted = unsafe { CGPreflightScreenCaptureAccess() };
        if granted {
            PermissionStatus::Granted
        } else {
            PermissionStatus::Denied
        }
    }

    /// Ask the system to show the permission prompt.
    ///
    /// macOS only shows the dialog once per app; afterwards this returns the
    /// stored answer and the user must go to System Settings. Callers should
    /// follow a `false` result with [`open_settings`].
    pub fn request() -> bool {
        #[allow(unsafe_code)]
        unsafe {
            CGRequestScreenCaptureAccess()
        }
    }

    pub fn open_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn();
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::PermissionStatus;

    pub fn status() -> PermissionStatus {
        // Windows does not gate capture. Wayland asks per session through the
        // portal at capture time, which is not something we can preflight.
        PermissionStatus::NotRequired
    }

    pub fn request() -> bool {
        true
    }

    pub fn open_settings() {}
}

pub use platform::{open_settings, request, status};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_stable_across_calls() {
        assert_eq!(status(), status());
    }

    #[test]
    fn non_macos_platforms_do_not_require_permission() {
        if !cfg!(target_os = "macos") {
            assert_eq!(status(), PermissionStatus::NotRequired);
            assert!(status().is_usable());
        }
    }
}
