//! The annotation model.
//!
//! Every annotation is a value, never a pixel edit. That is what makes the
//! editor non-destructive: a saved `.kestrel` document can be reopened and
//! re-edited, undo is a plain list operation, and the same document can be
//! exported at any resolution. ShareX flattens annotations into the bitmap on
//! save; keeping them as data is a deliberate improvement.
//!
//! Coordinates are in the base image's own pixel space, as `f32` so that
//! freehand strokes and scaled re-exports stay smooth.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance_to(self, other: Point) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// An axis-aligned rectangle. Always stored normalised: `width` and `height`
/// are non-negative, so a shape dragged up and to the left is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Build from two arbitrary corners — the shape a click-drag produces.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// The smallest rectangle containing every given point.
    pub fn bounding(points: &[Point]) -> Option<Rect> {
        let first = points.first()?;
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut max_x = first.x;
        let mut max_y = first.y;

        for p in points.iter().skip(1) {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }
}

/// Straight RGBA, not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// The default annotation colour, matching the red people reach for first.
    pub const ACCENT: Self = Self::rgb(255, 69, 58);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn is_transparent(&self) -> bool {
        self.a == 0
    }

    pub fn to_rgba(self) -> image::Rgba<u8> {
        image::Rgba([self.r, self.g, self.b, self.a])
    }

    /// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`. The UI speaks CSS.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();

        match hex.len() {
            3 => {
                let nibble = |i: usize| {
                    u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17) // 0xF -> 0xFF
                };
                Some(Self::rgb(nibble(0)?, nibble(1)?, nibble(2)?))
            }
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

/// Stroke and fill settings shared by the outline shapes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            color: Color::ACCENT,
            width: 3.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowHead {
    None,
    End,
    Both,
}

/// One annotation. The variants map 1:1 onto ShareX's editor tools, so the
/// keyboard shortcuts and the tool palette carry over unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Rectangle {
        rect: Rect,
        stroke: Stroke,
        fill: Color,
        corner_radius: f32,
    },
    Ellipse {
        rect: Rect,
        stroke: Stroke,
        fill: Color,
    },
    Line {
        from: Point,
        to: Point,
        stroke: Stroke,
    },
    Arrow {
        from: Point,
        to: Point,
        stroke: Stroke,
        head: ArrowHead,
    },
    Freehand {
        points: Vec<Point>,
        stroke: Stroke,
    },
    Text {
        rect: Rect,
        content: String,
        color: Color,
        outline: Color,
        size: f32,
        bold: bool,
        italic: bool,
    },
    SpeechBalloon {
        rect: Rect,
        tail: Point,
        content: String,
        stroke: Stroke,
        fill: Color,
        text_color: Color,
        size: f32,
    },
    /// Auto-numbered callout. The number is assigned by the document so that
    /// deleting one renumbers the rest.
    Step {
        center: Point,
        radius: f32,
        number: u32,
        fill: Color,
        text_color: Color,
    },
    Highlight {
        rect: Rect,
        color: Color,
    },
    Blur {
        rect: Rect,
        radius: f32,
    },
    Pixelate {
        rect: Rect,
        block: u32,
    },
    /// Dims everything *outside* the rectangle.
    Spotlight {
        rect: Rect,
        dim: f32,
    },
    /// An image dropped or pasted onto the canvas.
    ///
    /// The pixels are carried as a base64 PNG inside the document rather than
    /// as a path. A document has to survive the file it came from being moved
    /// or deleted — a screenshot annotated with a logo that vanishes when the
    /// logo is tidied away is not a document, it is a promise.
    Image {
        rect: Rect,
        /// Base64-encoded PNG.
        data: String,
        /// 0.0 to 1.0.
        opacity: f32,
    },
}

impl Shape {
    /// The axis-aligned box the shape occupies, used for hit testing and for
    /// deciding which pixels a re-render has to touch.
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Rectangle { rect, stroke, .. } | Shape::Ellipse { rect, stroke, .. } => {
                inflate(*rect, stroke.width / 2.0)
            }
            Shape::Line { from, to, stroke } => {
                inflate(Rect::from_corners(*from, *to), stroke.width / 2.0)
            }
            Shape::Arrow {
                from, to, stroke, ..
            } => inflate(
                Rect::from_corners(*from, *to),
                // The head extends well past the line's own width.
                stroke.width * 3.0,
            ),
            Shape::Freehand { points, stroke } => Rect::bounding(points)
                .map(|r| inflate(r, stroke.width / 2.0))
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            Shape::Text { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Spotlight { rect, .. }
            | Shape::Image { rect, .. } => *rect,
            Shape::SpeechBalloon { rect, tail, .. } => {
                let mut bounds = *rect;
                // The tail can point anywhere, including outside the bubble.
                bounds = bounds.translated(0.0, 0.0);
                Rect::from_corners(
                    Point::new(bounds.x.min(tail.x), bounds.y.min(tail.y)),
                    Point::new(bounds.right().max(tail.x), bounds.bottom().max(tail.y)),
                )
            }
            Shape::Step { center, radius, .. } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
        }
    }

    /// Whether a click at `p` should select this shape.
    ///
    /// Outline shapes are hit-tested by their bounding box rather than their
    /// stroke: demanding a click within three pixels of a thin line makes a
    /// shape practically unselectable.
    pub fn hit_test(&self, p: Point) -> bool {
        match self {
            Shape::Step { center, radius, .. } => center.distance_to(p) <= *radius,
            Shape::Freehand { points, stroke } => points
                .iter()
                .any(|point| point.distance_to(p) <= stroke.width.max(6.0)),
            _ => self.bounds().contains(p),
        }
    }

    /// Move the shape by a delta, in image pixels.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let move_point = |p: &mut Point| {
            p.x += dx;
            p.y += dy;
        };

        match self {
            Shape::Rectangle { rect, .. }
            | Shape::Ellipse { rect, .. }
            | Shape::Text { rect, .. }
            | Shape::Highlight { rect, .. }
            | Shape::Blur { rect, .. }
            | Shape::Pixelate { rect, .. }
            | Shape::Spotlight { rect, .. }
            | Shape::Image { rect, .. } => *rect = rect.translated(dx, dy),
            Shape::Line { from, to, .. } | Shape::Arrow { from, to, .. } => {
                move_point(from);
                move_point(to);
            }
            Shape::Freehand { points, .. } => points.iter_mut().for_each(move_point),
            Shape::SpeechBalloon { rect, tail, .. } => {
                *rect = rect.translated(dx, dy);
                move_point(tail);
            }
            Shape::Step { center, .. } => move_point(center),
        }
    }

    /// Whether this shape hides information. Redaction shapes are called out
    /// separately in the UI because getting them wrong leaks data.
    pub fn is_redaction(&self) -> bool {
        matches!(self, Shape::Blur { .. } | Shape::Pixelate { .. })
    }

    pub fn tool_name(&self) -> &'static str {
        match self {
            Shape::Rectangle { .. } => "rectangle",
            Shape::Ellipse { .. } => "ellipse",
            Shape::Line { .. } => "line",
            Shape::Arrow { .. } => "arrow",
            Shape::Freehand { .. } => "freehand",
            Shape::Text { .. } => "text",
            Shape::SpeechBalloon { .. } => "speech_balloon",
            Shape::Step { .. } => "step",
            Shape::Highlight { .. } => "highlight",
            Shape::Blur { .. } => "blur",
            Shape::Pixelate { .. } => "pixelate",
            Shape::Spotlight { .. } => "spotlight",
            Shape::Image { .. } => "image",
        }
    }
}

fn inflate(rect: Rect, by: f32) -> Rect {
    Rect::new(
        rect.x - by,
        rect.y - by,
        rect.width + by * 2.0,
        rect.height + by * 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> Stroke {
        Stroke {
            color: Color::ACCENT,
            width: 4.0,
        }
    }

    #[test]
    fn rect_from_corners_normalises_every_drag_direction() {
        let expected = Rect::new(10.0, 10.0, 90.0, 40.0);
        for (a, b) in [
            (Point::new(10.0, 10.0), Point::new(100.0, 50.0)),
            (Point::new(100.0, 50.0), Point::new(10.0, 10.0)),
            (Point::new(100.0, 10.0), Point::new(10.0, 50.0)),
            (Point::new(10.0, 50.0), Point::new(100.0, 10.0)),
        ] {
            assert_eq!(Rect::from_corners(a, b), expected);
        }
    }

    #[test]
    fn bounding_covers_every_point() {
        let points = [
            Point::new(5.0, 20.0),
            Point::new(-3.0, 8.0),
            Point::new(11.0, -2.0),
        ];
        let bounds = Rect::bounding(&points).unwrap();
        assert!(points.iter().all(|p| bounds.contains(*p)));
        assert_eq!(bounds, Rect::new(-3.0, -2.0, 14.0, 22.0));
        assert_eq!(Rect::bounding(&[]), None);
    }

    #[test]
    fn colors_round_trip_through_hex() {
        for hex in ["#ff453a", "#00000080", "#ffffff"] {
            let color = Color::from_hex(hex).expect("should parse");
            assert_eq!(color.to_hex(), hex);
        }
        assert_eq!(Color::from_hex("#f00"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::from_hex("nonsense"), None);
        assert_eq!(Color::from_hex("#12345"), None);
    }

    #[test]
    fn stroke_width_widens_the_bounds() {
        let shape = Shape::Rectangle {
            rect: Rect::new(10.0, 10.0, 100.0, 50.0),
            stroke: stroke(),
            fill: Color::TRANSPARENT,
            corner_radius: 0.0,
        };
        let bounds = shape.bounds();
        // A 4px stroke straddles the edge, so 2px each side.
        assert_eq!(bounds, Rect::new(8.0, 8.0, 104.0, 54.0));
    }

    #[test]
    fn arrow_bounds_leave_room_for_the_head() {
        let shape = Shape::Arrow {
            from: Point::new(0.0, 0.0),
            to: Point::new(50.0, 0.0),
            stroke: stroke(),
            head: ArrowHead::End,
        };
        let bounds = shape.bounds();
        assert!(
            bounds.height > stroke().width,
            "the head is wider than the line and must fit inside the bounds"
        );
    }

    #[test]
    fn translate_moves_every_kind_of_shape() {
        let mut shapes = [
            Shape::Rectangle {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                stroke: stroke(),
                fill: Color::TRANSPARENT,
                corner_radius: 0.0,
            },
            Shape::Line {
                from: Point::new(0.0, 0.0),
                to: Point::new(10.0, 10.0),
                stroke: stroke(),
            },
            Shape::Freehand {
                points: vec![Point::new(0.0, 0.0), Point::new(5.0, 5.0)],
                stroke: stroke(),
            },
            Shape::Step {
                center: Point::new(0.0, 0.0),
                radius: 12.0,
                number: 1,
                fill: Color::ACCENT,
                text_color: Color::WHITE,
            },
        ];

        for shape in shapes.iter_mut() {
            let before = shape.bounds();
            shape.translate(7.0, -3.0);
            let after = shape.bounds();
            assert!(
                (after.x - before.x - 7.0).abs() < 0.001
                    && (after.y - before.y + 3.0).abs() < 0.001,
                "{} did not translate",
                shape.tool_name()
            );
        }
    }

    #[test]
    fn freehand_hit_test_follows_the_stroke_not_the_box() {
        let shape = Shape::Freehand {
            points: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
            stroke: Stroke {
                color: Color::ACCENT,
                width: 2.0,
            },
        };
        // On the line.
        assert!(shape.hit_test(Point::new(100.0, 2.0)));
        // Inside the bounding box but far from any drawn point.
        assert!(!shape.hit_test(Point::new(50.0, 60.0)));
    }

    #[test]
    fn step_hit_test_is_circular() {
        let shape = Shape::Step {
            center: Point::new(50.0, 50.0),
            radius: 10.0,
            number: 3,
            fill: Color::ACCENT,
            text_color: Color::WHITE,
        };
        assert!(shape.hit_test(Point::new(55.0, 50.0)));
        // Inside the bounding square, outside the circle.
        assert!(!shape.hit_test(Point::new(59.0, 59.0)));
    }

    #[test]
    fn redaction_shapes_are_flagged() {
        let blur = Shape::Blur {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radius: 8.0,
        };
        let highlight = Shape::Highlight {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgba(255, 255, 0, 90),
        };
        assert!(blur.is_redaction());
        assert!(!highlight.is_redaction());
    }

    #[test]
    fn shapes_survive_a_json_round_trip() {
        let shapes = vec![
            Shape::Arrow {
                from: Point::new(1.0, 2.0),
                to: Point::new(3.0, 4.0),
                stroke: stroke(),
                head: ArrowHead::Both,
            },
            Shape::Text {
                rect: Rect::new(0.0, 0.0, 80.0, 20.0),
                content: "merhaba".into(),
                color: Color::WHITE,
                outline: Color::BLACK,
                size: 18.0,
                bold: true,
                italic: false,
            },
        ];

        let json = serde_json::to_string(&shapes).unwrap();
        let back: Vec<Shape> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, shapes);
        // The tag makes the format self-describing and forward-compatible.
        assert!(json.contains("\"kind\":\"arrow\""));
    }
}
