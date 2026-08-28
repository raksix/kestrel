//! Coordinate maths for a multi-display, mixed-DPI world.
//!
//! Every logical→physical conversion in Kestrel goes through this module.
//! Scattering `* scale_factor` across the codebase is the single biggest
//! source of off-by-a-pixel capture bugs, so it lives in exactly one place.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in the global logical coordinate space.
/// The origin is the top-left of the primary display; secondary displays may
/// sit at negative coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Build a region from two arbitrary corners — the shape a click-drag
    /// produces. Handles dragging up and/or left.
    pub fn from_corners(a: Point, b: Point) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self {
            x,
            y,
            width: (a.x - b.x).unsigned_abs(),
            height: (a.y - b.y).unsigned_abs(),
        }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersects(&self, other: &Region) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The overlapping area, or `None` when the rectangles are disjoint.
    pub fn intersection(&self, other: &Region) -> Option<Region> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(Region::new(x, y, (right - x) as u32, (bottom - y) as u32))
    }

    /// The smallest rectangle containing both.
    pub fn union(&self, other: &Region) -> Region {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Region::new(x, y, (right - x) as u32, (bottom - y) as u32)
    }

    /// Translate into a coordinate space whose origin is `origin`.
    pub fn relative_to(&self, origin: Point) -> Region {
        Region::new(
            self.x - origin.x,
            self.y - origin.y,
            self.width,
            self.height,
        )
    }

    /// Convert logical points to physical pixels for a display of the given
    /// scale factor. Rounds outward so a selection never loses an edge pixel.
    pub fn to_physical(&self, scale_factor: f32) -> Region {
        if (scale_factor - 1.0).abs() < f32::EPSILON {
            return *self;
        }
        let left = (self.x as f32 * scale_factor).floor();
        let top = (self.y as f32 * scale_factor).floor();
        let right = (self.right() as f32 * scale_factor).ceil();
        let bottom = (self.bottom() as f32 * scale_factor).ceil();
        Region::new(
            left as i32,
            top as i32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    }

    /// Grow the rectangle by `n` on every side, clamped at the coordinate space
    /// edge so the result never has negative extent.
    pub fn inflate(&self, n: i32) -> Region {
        let width = (self.width as i64 + 2 * n as i64).max(0) as u32;
        let height = (self.height as i64 + 2 * n as i64).max(0) as u32;
        Region::new(self.x - n, self.y - n, width, height)
    }
}

/// The smallest rectangle covering every display — the size of the virtual
/// desktop, which may start at negative coordinates.
pub fn bounding_region(regions: &[Region]) -> Option<Region> {
    regions
        .iter()
        .copied()
        .reduce(|acc, region| acc.union(&region))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn p(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn from_corners_handles_every_drag_direction() {
        let expected = Region::new(10, 10, 90, 40);
        assert_eq!(Region::from_corners(p(10, 10), p(100, 50)), expected);
        assert_eq!(Region::from_corners(p(100, 50), p(10, 10)), expected);
        assert_eq!(Region::from_corners(p(100, 10), p(10, 50)), expected);
        assert_eq!(Region::from_corners(p(10, 50), p(100, 10)), expected);
    }

    #[test]
    fn a_zero_size_drag_is_empty() {
        assert!(Region::from_corners(p(5, 5), p(5, 5)).is_empty());
        assert!(Region::from_corners(p(5, 5), p(5, 90)).is_empty());
    }

    #[test]
    fn intersection_of_disjoint_regions_is_none() {
        let a = Region::new(0, 0, 100, 100);
        let b = Region::new(200, 200, 50, 50);
        assert_eq!(a.intersection(&b), None);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn touching_edges_do_not_count_as_overlap() {
        let a = Region::new(0, 0, 100, 100);
        let b = Region::new(100, 0, 100, 100);
        assert_eq!(a.intersection(&b), None);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn intersection_clips_to_the_shared_area() {
        let a = Region::new(0, 0, 100, 100);
        let b = Region::new(50, 50, 100, 100);
        assert_eq!(a.intersection(&b), Some(Region::new(50, 50, 50, 50)));
    }

    #[test]
    fn union_spans_displays_left_of_the_origin() {
        // A secondary display placed to the left sits at negative x.
        let primary = Region::new(0, 0, 1920, 1080);
        let secondary = Region::new(-1440, 0, 1440, 900);
        assert_eq!(primary.union(&secondary), Region::new(-1440, 0, 3360, 1080));
    }

    #[test]
    fn bounding_region_covers_a_mixed_layout() {
        let displays = [
            Region::new(0, 0, 1920, 1080),
            Region::new(-1440, -200, 1440, 900),
            Region::new(1920, 300, 2560, 1440),
        ];
        assert_eq!(
            bounding_region(&displays),
            Some(Region::new(-1440, -200, 5920, 1940))
        );
        assert_eq!(bounding_region(&[]), None);
    }

    #[test]
    fn physical_conversion_rounds_outward_on_retina() {
        let region = Region::new(10, 10, 100, 50);
        assert_eq!(region.to_physical(2.0), Region::new(20, 20, 200, 100));
        // A fractional scale must never shrink the selection.
        let odd = Region::new(11, 11, 33, 33);
        let physical = odd.to_physical(1.5);
        assert!(physical.width >= 49 && physical.height >= 49);
    }

    #[test]
    fn physical_conversion_is_identity_at_scale_one() {
        let region = Region::new(-5, 7, 100, 50);
        assert_eq!(region.to_physical(1.0), region);
    }

    #[test]
    fn relative_to_moves_into_display_local_space() {
        let selection = Region::new(1930, 310, 200, 100);
        let display_origin = p(1920, 300);
        assert_eq!(
            selection.relative_to(display_origin),
            Region::new(10, 10, 200, 100)
        );
    }

    #[test]
    fn inflate_never_produces_negative_extent() {
        let tiny = Region::new(10, 10, 4, 4);
        assert_eq!(tiny.inflate(-10), Region::new(20, 20, 0, 0));
    }

    #[test]
    fn contains_excludes_the_far_edges() {
        let region = Region::new(0, 0, 10, 10);
        assert!(region.contains(p(0, 0)));
        assert!(region.contains(p(9, 9)));
        assert!(!region.contains(p(10, 10)));
    }
}
