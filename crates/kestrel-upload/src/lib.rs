//! Uploading, and ShareX-compatible custom uploaders.
//!
//! The custom uploader engine matters more than any individual service
//! integration: ShareX's `.sxcu` files describe an HTTP endpoint completely, and
//! hundreds of them already exist. Supporting the format exactly means all of
//! them work here without modification.

pub mod client;
pub mod sxcu;
pub mod syntax;

pub use client::{execute, Payload, RawResponse};
pub use sxcu::{CustomUploader, PreparedRequest, UploadResult};
pub use syntax::{expand, Context, NoPrompts, Prompter};
