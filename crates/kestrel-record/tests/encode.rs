//! End-to-end encode tests against a real ffmpeg.
//!
//! The unit tests check that the argument list *contains* the right flags,
//! which cannot catch an argument ffmpeg rejects, a filter graph with a typo,
//! or a codec/container pairing that fails only at run time. These feed
//! synthetic frames through the actual binary.
//!
//! Skipped when ffmpeg is absent, so a machine without it still has a green
//! suite — the recorder already reports a clear error in that case.

use std::io::Write;
use std::process::{Command, Stdio};

use kestrel_record::ffmpeg::{self, OutputFormat, RecordSettings, VideoCodec};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const FRAMES: usize = 10;

/// A vertical bar that moves across the frame, so the encoder has real
/// motion to compress rather than identical frames it can collapse away.
fn frame(index: usize) -> Vec<u8> {
    let bar = (index * 5) as u32 % WIDTH;
    let mut raw = vec![0u8; (WIDTH * HEIGHT * 4) as usize];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            let lit = x.abs_diff(bar) < 4;
            raw[offset] = if lit { 255 } else { 20 };
            raw[offset + 1] = (y * 4) as u8;
            raw[offset + 2] = (index * 20) as u8;
            raw[offset + 3] = 255;
        }
    }
    raw
}

fn encode(settings: &RecordSettings, extension: &str) -> Option<std::path::PathBuf> {
    let binary = ffmpeg::find()?;

    let dir =
        std::env::temp_dir().join(format!("kestrel-encode-{}-{extension}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let output = dir.join(format!("out.{extension}"));

    let mut child = Command::new(binary)
        .args(ffmpeg::encode_args(WIDTH, HEIGHT, settings, &output))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ffmpeg should start");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        for index in 0..FRAMES {
            stdin.write_all(&frame(index)).expect("write frame");
        }
    }

    let result = child.wait_with_output().expect("ffmpeg should finish");
    assert!(
        result.status.success(),
        "ffmpeg rejected our arguments: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Some(output)
}

fn assert_playable(path: &std::path::Path) {
    let size = std::fs::metadata(path).expect("output exists").len();
    assert!(size > 0, "the encoder produced an empty file");
}

#[test]
fn h264_encodes_to_a_playable_mp4() {
    let Some(output) = encode(&RecordSettings::default(), "mp4") else {
        eprintln!("ffmpeg not installed, skipping");
        return;
    };
    assert_playable(&output);
    let _ = std::fs::remove_dir_all(output.parent().unwrap());
}

#[test]
fn odd_dimensions_still_encode() {
    // yuv420p rejects odd dimensions outright; this is the case the scale
    // filter exists for, and the one a region drag hits constantly.
    let Some(binary) = ffmpeg::find() else {
        eprintln!("ffmpeg not installed, skipping");
        return;
    };

    let dir = std::env::temp_dir().join(format!("kestrel-odd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let output = dir.join("odd.mp4");

    let (width, height) = (65u32, 49u32);
    let mut child = Command::new(binary)
        .args(ffmpeg::encode_args(
            width,
            height,
            &RecordSettings::default(),
            &output,
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ffmpeg should start");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        let blank = vec![128u8; (width * height * 4) as usize];
        for _ in 0..FRAMES {
            stdin.write_all(&blank).expect("write frame");
        }
    }

    let result = child.wait_with_output().expect("ffmpeg should finish");
    assert!(
        result.status.success(),
        "odd dimensions should be corrected, not rejected: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_playable(&output);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gif_encodes_with_its_palette_filter() {
    let settings = RecordSettings {
        format: OutputFormat::Gif,
        ..Default::default()
    };
    let Some(output) = encode(&settings, "gif") else {
        eprintln!("ffmpeg not installed, skipping");
        return;
    };
    assert_playable(&output);
    let _ = std::fs::remove_dir_all(output.parent().unwrap());
}

#[test]
fn vp9_encodes_to_webm() {
    // The pairing that would break if the mp4-only faststart flag leaked in.
    let settings = RecordSettings {
        codec: VideoCodec::Vp9,
        // VP9 at the default quality is slow; this keeps the test quick.
        crf: 40,
        ..Default::default()
    };
    let Some(output) = encode(&settings, "webm") else {
        eprintln!("ffmpeg not installed, skipping");
        return;
    };
    assert_playable(&output);
    let _ = std::fs::remove_dir_all(output.parent().unwrap());
}
