//! Kestrel's annotation editor: model, history and rendering.
//!
//! The webview draws a live preview on a canvas, but the file Kestrel actually
//! writes is rendered here. Two reasons: webview colour management and
//! anti-aliasing differ across WebKitGTK, WebView2 and WKWebView, so a browser
//! render would make the same document export differently on each platform;
//! and the export must not be limited to what fits on screen.

pub mod document;
pub mod font;
pub mod frame;
pub mod render;
pub mod shape;

pub use document::Document;
pub use frame::{Background, Frame, Shadow};
pub use render::render;
pub use shape::{ArrowHead, Color, Point, Rect, Shape, Stroke};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
