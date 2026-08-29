//! `kestrel` — command-line access to Kestrel's tools.
//!
//! This exists because the logic lives in crates that never import a UI. Every
//! subcommand here calls exactly the same code the app calls, so the two cannot
//! drift apart and neither is the "real" implementation.
//!
//! **Scope.** These are the tools that need nothing but a file. The subcommands
//! that drive a running app — taking a screenshot, running a workflow, opening
//! the editor — are not here yet, because they need an IPC channel to the
//! running instance that does not exist. Shipping them as stubs that fail at
//! run time would be worse than their absence, so `kestrel --help` does not
//! advertise what it cannot do.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod tools;

#[derive(Parser)]
#[command(
    name = "kestrel",
    version,
    about = "Kestrel's tools, without the app",
    long_about = None
)]
struct Cli {
    /// Print results as JSON instead of text.
    ///
    /// Text is for reading; JSON is for piping into something else.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Checksums for a file: MD5, SHA-1, SHA-256 and SHA-512 in one pass.
    Hash {
        path: PathBuf,
        /// Compare against this value; exits non-zero when it does not match.
        #[arg(long)]
        expect: Option<String>,
    },

    /// Metadata in a file, with anything identifying flagged.
    Metadata {
        path: PathBuf,
        /// Write a copy with the metadata removed, leaving the original alone.
        #[arg(long)]
        strip: bool,
    },

    /// Read the QR codes in an image.
    Qr { path: PathBuf },

    /// Describe an image: size, colours, transparency.
    Analyze { path: PathBuf },

    /// Compare two images.
    Compare {
        first: PathBuf,
        second: PathBuf,
        /// How different a channel may be and still count as unchanged.
        #[arg(long, default_value_t = kestrel_tools::compare::DEFAULT_TOLERANCE)]
        tolerance: u8,
        /// Also write a diff picture here.
        #[arg(long)]
        diff: Option<PathBuf>,
    },

    /// The colour at a point in an image, in every notation.
    Color {
        path: PathBuf,
        x: u32,
        y: u32,
        /// Average a square of this radius instead of reading one pixel.
        #[arg(long, default_value_t = 0)]
        radius: u32,
    },

    /// Index a directory to HTML, text, JSON or XML.
    Index {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = IndexFormat::Html)]
        format: IndexFormat,
        /// Write here instead of to standard output.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Convert a video.
    Convert {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = ConvertTarget::Mp4)]
        to: ConvertTarget,
        #[arg(long, default_value_t = 23)]
        crf: u8,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        fps: Option<u32>,
        #[arg(long)]
        mute: bool,
    },

    /// Grab a single frame from a video.
    Thumbnail {
        path: PathBuf,
        /// Seconds into the video.
        #[arg(long, default_value_t = 1.0)]
        at: f32,
        #[arg(long, default_value_t = 480)]
        width: u32,
    },

    /// Read the text in an image.
    Ocr {
        path: PathBuf,
        /// Directory holding text-detection.rten and text-recognition.rten.
        #[arg(long)]
        models: PathBuf,
    },

    /// Expand a ShareX filename pattern, so a pattern can be checked before use.
    Name {
        pattern: String,
        /// Pretend the capture came from a window with this title.
        #[arg(long)]
        window: Option<String>,
    },

    /// Show what a ShareX `.sxcu` custom uploader does.
    Sxcu { path: PathBuf },

    /// Show what a ShareX `.sxie` effect preset contains.
    Sxie { path: PathBuf },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum IndexFormat {
    Html,
    Text,
    Json,
    Xml,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ConvertTarget {
    Mp4,
    Webm,
    Mkv,
    Gif,
    Mp3,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match tools::run(cli.command, cli.json) {
        Ok(outcome) => outcome,
        Err(message) => {
            // Diagnostics go to stderr so `kestrel hash x --json | jq` is not
            // corrupted by an error landing in the middle of the JSON.
            eprintln!("kestrel: {message}");
            ExitCode::FAILURE
        }
    }
}
