//! Kestrel's standalone tools.
//!
//! Everything here is pure: an image or some bytes in, a result out. That keeps
//! the tools testable without a screen, a network or a running app, and lets
//! the CLI expose them without going through Tauri.

pub mod analyze;
pub mod hash;
pub mod indexer;
pub mod metadata;
pub mod qr;

pub use analyze::{analyze, combine, split, thumbnail, Analysis, Direction};
pub use hash::{hash_bytes, hash_file, hash_file_all, Algorithm};
pub use indexer::{index, Options as IndexOptions};
pub use metadata::{read as read_metadata, strip as strip_metadata, Field as MetadataField};
pub use qr::{decode, encode, Decoded};
