//! Cross-platform capture backend.
//!
//! Everything platform-specific hides behind this module. The rest of Kestrel
//! only ever sees [`DisplayInfo`], [`WindowInfo`], [`Region`] and [`Capture`].
//!
//! Backends today: `xcap` (X11, Wayland/PipeWire, Windows.Graphics.Capture,
//! ScreenCaptureKit). The trait exists so the macOS and Windows paths can be
//! swapped for hand-written ones later without touching callers.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

pub mod frame;
pub mod geometry;
pub mod permissions;
pub mod stitch;

pub use frame::FrozenFrames;
pub use geometry::{Point, Region};
pub use permissions::PermissionStatus;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no display matched id {0}")]
    DisplayNotFound(u32),
    #[error("no window matched id {0}")]
    WindowNotFound(u32),
    #[error("the selected region is empty")]
    EmptyRegion,
    #[error("region {region:?} does not overlap any display")]
    RegionOffScreen { region: Region },
    #[error("platform capture failed: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

impl From<xcap::XCapError> for CaptureError {
    fn from(value: xcap::XCapError) -> Self {
        CaptureError::Backend(value.to_string())
    }
}

/// A physical display, in the global logical coordinate space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub region: Region,
    /// Ratio of physical pixels to logical points. 2.0 on a Retina panel.
    pub scale_factor: f32,
    pub is_primary: bool,
}

/// A capturable top-level window, ordered front-to-back by the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub region: Region,
    pub is_minimized: bool,
    /// Stacking order; higher is closer to the front.
    pub z: i32,
    pub is_focused: bool,
}

/// The result of a capture, plus the provenance needed to name the file.
pub struct Capture {
    pub image: RgbaImage,
    pub region: Region,
    pub window_title: Option<String>,
    pub app_name: Option<String>,
}

impl Capture {
    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }
}

/// What the current platform actually supports. The UI reads this and disables
/// (with an explanation) anything unavailable — no silent failures.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub window_enumeration: bool,
    pub window_capture: bool,
    pub region_capture: bool,
    pub global_shortcuts: bool,
    pub scrolling_capture: bool,
    /// Whether the OS has actually let us capture yet. On macOS this is the
    /// difference between a working app and one that silently returns
    /// wallpaper.
    pub screen_permission: PermissionStatus,
}

pub trait CaptureBackend: Send + Sync {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn windows(&self) -> Result<Vec<WindowInfo>>;
    fn capture_display(&self, id: u32) -> Result<Capture>;
    fn capture_window(&self, id: u32) -> Result<Capture>;
    fn capture_region(&self, region: Region) -> Result<Capture>;
    fn capabilities(&self) -> Capabilities;

    /// Every display composited into one image, in global coordinates.
    fn capture_all_displays(&self) -> Result<Capture>;

    /// Snapshot every display at once, for the selection overlay to crop from.
    /// See [`frame`] for why region capture must not re-capture the screen.
    fn freeze(&self) -> Result<FrozenFrames>;
}

mod xcap_backend;
pub use xcap_backend::XcapBackend;

/// The backend for the current platform.
pub fn backend() -> impl CaptureBackend {
    XcapBackend::new()
}
