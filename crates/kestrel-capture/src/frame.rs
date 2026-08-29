//! Frozen display snapshots taken *before* the selection overlay appears.
//!
//! Region capture must never re-capture the screen after the overlay is up, or
//! the overlay's own dimming and toolbar land in the result. ShareX solves this
//! by freezing the screen first; Kestrel does the same. Selections are cropped
//! out of these buffers, so the overlay is invisible to the capture by
//! construction and the crop costs no additional platform round trip.

use image::RgbaImage;

use crate::{
    geometry::{Point, Region},
    Capture, CaptureError, DisplayInfo, Result,
};

/// One snapshot per display, kept for the lifetime of an overlay session.
#[derive(Default)]
pub struct FrozenFrames {
    frames: Vec<(DisplayInfo, RgbaImage)>,
}

impl FrozenFrames {
    pub fn new(frames: Vec<(DisplayInfo, RgbaImage)>) -> Self {
        Self { frames }
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn displays(&self) -> Vec<DisplayInfo> {
        self.frames.iter().map(|(info, _)| info.clone()).collect()
    }

    /// Crop a region given in the global logical coordinate space.
    ///
    /// The region is clipped to whichever display it overlaps most, so a
    /// selection dragged slightly past a screen edge still yields the visible
    /// part instead of failing.
    pub fn crop(&self, region: Region) -> Result<Capture> {
        if region.is_empty() {
            return Err(CaptureError::EmptyRegion);
        }

        let (info, image, overlap) = self
            .frames
            .iter()
            .filter_map(|(info, image)| {
                region
                    .intersection(&info.region)
                    .map(|overlap| (info, image, overlap))
            })
            .max_by_key(|(_, _, overlap)| overlap.area())
            .ok_or(CaptureError::RegionOffScreen { region })?;

        let local = overlap.relative_to(Point {
            x: info.region.x,
            y: info.region.y,
        });
        let physical = local.to_physical(info.scale_factor);

        // The frozen buffer is the authority on size: a display's reported
        // logical bounds times its scale factor does not always match the
        // pixels the platform actually handed us.
        let x = physical.x.max(0) as u32;
        let y = physical.y.max(0) as u32;
        let width = physical.width.min(image.width().saturating_sub(x));
        let height = physical.height.min(image.height().saturating_sub(y));

        if width == 0 || height == 0 {
            return Err(CaptureError::EmptyRegion);
        }

        let cropped = image::imageops::crop_imm(image, x, y, width, height).to_image();

        Ok(Capture {
            image: cropped,
            region: overlap,
            window_title: None,
            app_name: None,
        })
    }

    /// Crop the full extent of one display.
    pub fn display(&self, id: u32) -> Result<Capture> {
        let (info, image) = self
            .frames
            .iter()
            .find(|(info, _)| info.id == id)
            .ok_or(CaptureError::DisplayNotFound(id))?;

        Ok(Capture {
            image: image.clone(),
            region: info.region,
            window_title: None,
            app_name: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn display(id: u32, x: i32, y: i32, w: u32, h: u32, scale: f32) -> DisplayInfo {
        DisplayInfo {
            id,
            name: format!("display {id}"),
            region: Region::new(x, y, w, h),
            scale_factor: scale,
            is_primary: id == 1,
        }
    }

    /// A frame whose pixels encode their own coordinates, so a crop can be
    /// checked for being taken from exactly the right place.
    fn coded_frame(w: u32, h: u32, tag: u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, tag, 255])
        })
    }

    #[test]
    fn crop_maps_logical_selection_to_retina_pixels() {
        let info = display(1, 0, 0, 1710, 1112, 2.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(3420, 2224, 7))]);

        let capture = frames.crop(Region::new(100, 50, 200, 100)).unwrap();

        // 2x display: a 200x100 logical selection is 400x200 real pixels.
        assert_eq!(capture.width(), 400);
        assert_eq!(capture.height(), 200);
        // Top-left pixel must come from physical (200, 100).
        assert_eq!(capture.image.get_pixel(0, 0), &Rgba([200, 100, 7, 255]));
    }

    #[test]
    fn crop_on_a_non_retina_display_is_one_to_one() {
        let info = display(1, 0, 0, 1920, 1080, 1.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(1920, 1080, 3))]);

        let capture = frames.crop(Region::new(10, 20, 30, 40)).unwrap();

        assert_eq!((capture.width(), capture.height()), (30, 40));
        assert_eq!(capture.image.get_pixel(0, 0), &Rgba([10, 20, 3, 255]));
    }

    #[test]
    fn crop_picks_the_display_with_the_larger_overlap() {
        let left = display(1, 0, 0, 100, 100, 1.0);
        let right = display(2, 100, 0, 100, 100, 1.0);
        let frames = FrozenFrames::new(vec![
            (left, coded_frame(100, 100, 1)),
            (right, coded_frame(100, 100, 2)),
        ]);

        // 30 columns on display 1, 70 on display 2 — display 2 wins.
        let capture = frames.crop(Region::new(70, 10, 100, 20)).unwrap();
        assert_eq!(capture.image.get_pixel(0, 0)[2], 2);
        assert_eq!(capture.region, Region::new(100, 10, 70, 20));
    }

    #[test]
    fn crop_clips_a_selection_dragged_past_the_screen_edge() {
        let info = display(1, 0, 0, 100, 100, 1.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(100, 100, 1))]);

        let capture = frames.crop(Region::new(80, 80, 50, 50)).unwrap();

        assert_eq!((capture.width(), capture.height()), (20, 20));
    }

    #[test]
    fn crop_on_a_secondary_display_left_of_the_origin() {
        let primary = display(1, 0, 0, 100, 100, 1.0);
        let secondary = display(2, -100, 0, 100, 100, 2.0);
        let frames = FrozenFrames::new(vec![
            (primary, coded_frame(100, 100, 1)),
            (secondary, coded_frame(200, 200, 2)),
        ]);

        let capture = frames.crop(Region::new(-90, 10, 20, 20)).unwrap();

        assert_eq!(capture.image.get_pixel(0, 0), &Rgba([20, 20, 2, 255]));
        assert_eq!((capture.width(), capture.height()), (40, 40));
    }

    #[test]
    fn an_empty_selection_is_rejected() {
        let info = display(1, 0, 0, 100, 100, 1.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(100, 100, 1))]);

        assert!(matches!(
            frames.crop(Region::new(10, 10, 0, 40)),
            Err(CaptureError::EmptyRegion)
        ));
    }

    #[test]
    fn a_selection_entirely_off_screen_is_rejected() {
        let info = display(1, 0, 0, 100, 100, 1.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(100, 100, 1))]);

        assert!(matches!(
            frames.crop(Region::new(500, 500, 40, 40)),
            Err(CaptureError::RegionOffScreen { .. })
        ));
    }

    #[test]
    fn crop_never_reads_past_a_short_frame_buffer() {
        // The platform handed back fewer pixels than logical bounds x scale.
        let info = display(1, 0, 0, 1710, 1112, 2.0);
        let frames = FrozenFrames::new(vec![(info, coded_frame(3000, 2000, 9))]);

        let capture = frames.crop(Region::new(1400, 900, 300, 200)).unwrap();

        assert!(capture.width() > 0 && capture.height() > 0);
        assert!(2800 + capture.width() <= 3000);
    }
}
