//! Screen recording.
//!
//! Frames come from the same platform layer the screenshot path uses, and are
//! piped to ffmpeg as raw RGBA. Letting ffmpeg grab the screen itself would
//! mean three separate input backends — `avfoundation`, `gdigrab`, `x11grab` —
//! each with its own device discovery and region syntax, and region recording
//! would have to be rebuilt rather than reused. ShareX bundles ffmpeg for the
//! same reasons.

pub mod convert;
pub mod ffmpeg;
pub mod recorder;

pub use convert::{convert, thumbnail, ConvertSettings, Target};
pub use ffmpeg::{OutputFormat, RecordSettings, VideoCodec};
pub use recorder::{start, Recording};
