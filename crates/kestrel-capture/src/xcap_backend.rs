//! `xcap`-backed implementation of [`CaptureBackend`].
//!
//! `xcap` gives us one API over ScreenCaptureKit, Windows.Graphics.Capture,
//! X11 and PipeWire. Its accessors are all fallible, so this module's job is
//! largely to collapse per-field errors into whole-object errors and to move
//! everything into Kestrel's single global coordinate space.

use image::RgbaImage;
use xcap::{Monitor, Window};

use crate::{
    geometry::{bounding_region, Point, Region},
    Capabilities, Capture, CaptureBackend, CaptureError, DisplayInfo, Result, WindowInfo,
};

pub struct XcapBackend;

impl XcapBackend {
    pub fn new() -> Self {
        Self
    }

    fn display_info(monitor: &Monitor) -> Result<DisplayInfo> {
        Ok(DisplayInfo {
            id: monitor.id()?,
            name: monitor
                .friendly_name()
                .or_else(|_| monitor.name())
                .unwrap_or_else(|_| "Display".to_string()),
            region: Region::new(
                monitor.x()?,
                monitor.y()?,
                monitor.width()?,
                monitor.height()?,
            ),
            scale_factor: monitor.scale_factor().unwrap_or(1.0),
            is_primary: monitor.is_primary().unwrap_or(false),
        })
    }

    fn window_info(window: &Window) -> Result<WindowInfo> {
        Ok(WindowInfo {
            id: window.id()?,
            title: window.title().unwrap_or_default(),
            app_name: window.app_name().unwrap_or_default(),
            region: Region::new(window.x()?, window.y()?, window.width()?, window.height()?),
            is_minimized: window.is_minimized().unwrap_or(false),
            z: window.z().unwrap_or(0),
            is_focused: window.is_focused().unwrap_or(false),
        })
    }

    /// Windows too small or too system-owned to be worth offering the user.
    /// Without this the picker fills up with 1px helper windows, menu bars and
    /// Kestrel's own overlays.
    fn is_pickable(window: &WindowInfo) -> bool {
        const MIN_EDGE: u32 = 40;

        if window.is_minimized || window.region.width < MIN_EDGE || window.region.height < MIN_EDGE
        {
            return false;
        }
        if window.app_name.eq_ignore_ascii_case("Kestrel") {
            return false;
        }
        // macOS reports the menu bar and desktop as Window Server windows.
        !window.app_name.eq_ignore_ascii_case("Window Server")
    }

    fn monitor_by_id(id: u32) -> Result<Monitor> {
        Monitor::all()?
            .into_iter()
            .find(|m| m.id().map(|i| i == id).unwrap_or(false))
            .ok_or(CaptureError::DisplayNotFound(id))
    }
}

impl Default for XcapBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for XcapBackend {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Monitor::all()?.iter().map(Self::display_info).collect()
    }

    fn windows(&self) -> Result<Vec<WindowInfo>> {
        // A window whose fields cannot be read is skipped rather than failing
        // the whole enumeration — one bad window should not hide the rest.
        let mut windows: Vec<WindowInfo> = Window::all()?
            .iter()
            .filter_map(|w| Self::window_info(w).ok())
            .filter(Self::is_pickable)
            .collect();

        // Front-most first: that is the order a picker should show them in,
        // and it makes "active window" mean the top of this list. Descending,
        // hence the negation rather than a plain sort.
        windows.sort_by_key(|window| std::cmp::Reverse(window.z));
        Ok(windows)
    }

    fn capture_display(&self, id: u32) -> Result<Capture> {
        let monitor = Self::monitor_by_id(id)?;
        let info = Self::display_info(&monitor)?;
        Ok(Capture {
            image: monitor.capture_image()?,
            region: info.region,
            window_title: None,
            app_name: None,
        })
    }

    fn capture_window(&self, id: u32) -> Result<Capture> {
        let window = Window::all()?
            .into_iter()
            .find(|w| w.id().map(|i| i == id).unwrap_or(false))
            .ok_or(CaptureError::WindowNotFound(id))?;
        let info = Self::window_info(&window)?;
        Ok(Capture {
            image: window.capture_image()?,
            region: info.region,
            window_title: Some(info.title),
            app_name: Some(info.app_name),
        })
    }

    fn capture_region(&self, region: Region) -> Result<Capture> {
        if region.is_empty() {
            return Err(CaptureError::EmptyRegion);
        }

        let monitors = Monitor::all()?;
        let mut best: Option<(Monitor, DisplayInfo, Region)> = None;

        // Pick the display the selection overlaps most. A selection spanning
        // two displays is captured from the dominant one, then filled in by
        // `capture_all_displays` callers when true spanning is needed.
        for monitor in monitors {
            let Ok(info) = Self::display_info(&monitor) else {
                continue;
            };
            let Some(overlap) = region.intersection(&info.region) else {
                continue;
            };
            let is_better = best
                .as_ref()
                .map(|(_, _, current)| overlap.area() > current.area())
                .unwrap_or(true);
            if is_better {
                best = Some((monitor, info, overlap));
            }
        }

        let Some((monitor, info, overlap)) = best else {
            return Err(CaptureError::RegionOffScreen { region });
        };

        let local = overlap.relative_to(Point {
            x: info.region.x,
            y: info.region.y,
        });

        let image = monitor.capture_region(
            local.x.max(0) as u32,
            local.y.max(0) as u32,
            local.width,
            local.height,
        )?;

        Ok(Capture {
            image,
            region: overlap,
            window_title: None,
            app_name: None,
        })
    }

    fn capture_all_displays(&self) -> Result<Capture> {
        let monitors = Monitor::all()?;

        // One display is the common case; compositing it into a fresh canvas
        // would double the work and the peak memory for no benefit.
        if let [monitor] = monitors.as_slice() {
            let info = Self::display_info(monitor)?;
            return Ok(Capture {
                image: monitor.capture_image()?,
                region: info.region,
                window_title: None,
                app_name: None,
            });
        }

        let infos: Vec<DisplayInfo> = monitors
            .iter()
            .filter_map(|m| Self::display_info(m).ok())
            .collect();

        let regions: Vec<Region> = infos.iter().map(|d| d.region).collect();
        let bounds = bounding_region(&regions)
            .ok_or_else(|| CaptureError::Backend("no displays reported by the platform".into()))?;

        // Composite in physical pixels so a Retina panel next to a 1x panel
        // does not lose detail. The canvas uses the largest scale in play.
        let scale = infos.iter().map(|d| d.scale_factor).fold(1.0f32, f32::max);
        let canvas_bounds = bounds.to_physical(scale);
        let mut canvas = RgbaImage::new(canvas_bounds.width, canvas_bounds.height);

        for (monitor, info) in monitors.iter().zip(infos.iter()) {
            let Ok(shot) = monitor.capture_image() else {
                continue;
            };
            let placement = info.region.to_physical(scale);
            let offset_x = placement.x - canvas_bounds.x;
            let offset_y = placement.y - canvas_bounds.y;
            overlay(&mut canvas, &shot, offset_x, offset_y);
        }

        Ok(Capture {
            image: canvas,
            region: bounds,
            window_title: None,
            app_name: None,
        })
    }

    fn freeze(&self) -> Result<crate::FrozenFrames> {
        let monitors = Monitor::all()?;
        let mut frames = Vec::with_capacity(monitors.len());

        for monitor in monitors {
            let Ok(info) = Self::display_info(&monitor) else {
                continue;
            };
            match monitor.capture_image() {
                Ok(image) => frames.push((info, image)),
                Err(err) => tracing::warn!(display = info.id, %err, "could not freeze display"),
            }
        }

        if frames.is_empty() {
            return Err(CaptureError::Backend(
                "no display could be captured — screen recording permission may be missing".into(),
            ));
        }
        Ok(crate::FrozenFrames::new(frames))
    }

    fn capabilities(&self) -> Capabilities {
        let permission = crate::permissions::status();
        // Without the OS permission nothing below is truly available, even
        // though the APIs exist — report it honestly rather than pretending.
        let usable = permission.is_usable();
        Capabilities {
            window_enumeration: usable && !is_wayland(),
            window_capture: usable && !is_wayland(),
            region_capture: usable,
            global_shortcuts: !is_wayland(),
            // Available everywhere, because Kestrel does not drive the scroll
            // — the user does, and the frames are joined afterwards. This used
            // to say Windows only, from when the plan was ShareX's WM_SCROLL.
            scrolling_capture: usable,
            screen_permission: permission,
        }
    }
}

/// Wayland deliberately withholds window enumeration and global key grabs.
/// We detect it so the UI can explain what is unavailable and why.
fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or_else(|_| std::env::var("WAYLAND_DISPLAY").is_ok())
}

/// Copy `src` onto `dst` at the given offset, clipping at the canvas edges.
///
/// Delegates to `imageops::replace`, which copies row-wise and handles
/// out-of-bounds placement. The previous hand-rolled per-pixel loop cost
/// seconds on a multi-megapixel frame in a debug build.
fn overlay(dst: &mut RgbaImage, src: &RgbaImage, offset_x: i32, offset_y: i32) {
    image::imageops::replace(dst, src, offset_x as i64, offset_y as i64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn overlay_clips_instead_of_panicking_out_of_bounds() {
        let mut canvas = RgbaImage::new(4, 4);
        let mut tile = RgbaImage::new(4, 4);
        for pixel in tile.pixels_mut() {
            *pixel = Rgba([255, 0, 0, 255]);
        }

        // Placed so that only the bottom-right quadrant lands on the canvas.
        overlay(&mut canvas, &tile, -2, -2);

        assert_eq!(*canvas.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
        assert_eq!(*canvas.get_pixel(1, 1), Rgba([255, 0, 0, 255]));
        assert_eq!(*canvas.get_pixel(3, 3), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn overlay_fully_off_canvas_is_a_no_op() {
        let mut canvas = RgbaImage::new(2, 2);
        let mut tile = RgbaImage::new(2, 2);
        for pixel in tile.pixels_mut() {
            *pixel = Rgba([9, 9, 9, 255]);
        }
        overlay(&mut canvas, &tile, 100, 100);
        assert!(canvas.pixels().all(|p| *p == Rgba([0, 0, 0, 0])));
    }
}
