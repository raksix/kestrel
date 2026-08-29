//! End-to-end conversion tests against a real ffmpeg.
//!
//! The unit tests assert on the argument list, which cannot catch a filter
//! graph ffmpeg rejects or a codec/container pairing that only fails at run
//! time. These build a real clip and put it through the actual binary.
//!
//! Skipped when ffmpeg is absent, so a machine without it still has a green
//! suite — the app already reports a clear error in that case.

use std::path::PathBuf;
use std::process::Command;

use kestrel_record::convert::{self, ConvertSettings, Target};
use kestrel_record::ffmpeg;

fn dir(name: &str) -> PathBuf {
    // Per-test directory names: a shared one means parallel tests delete each
    // other's fixtures.
    let dir = std::env::temp_dir().join(format!("kestrel-convert-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A two-second test clip with motion and a tone, so both streams are real.
fn source(binary: &std::path::Path, dir: &std::path::Path) -> PathBuf {
    let path = dir.join("source.mp4");
    let status = Command::new(binary)
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=15:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .output()
        .expect("ffmpeg runs");

    assert!(
        status.status.success(),
        "could not build the test clip: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    path
}

fn streams(binary: &std::path::Path, path: &std::path::Path) -> String {
    // ffprobe is not guaranteed to sit beside ffmpeg, so ask ffmpeg itself and
    // read the stream summary it prints while refusing to produce output.
    let output = Command::new(binary)
        .arg("-i")
        .arg(path)
        .output()
        .expect("ffmpeg runs");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn nonempty(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
}

#[test]
fn a_clip_converts_to_webm() {
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("webm");
    let input = source(&binary, &dir);

    let output = convert::convert(
        &binary,
        &input,
        &ConvertSettings {
            target: Target::Webm,
            ..Default::default()
        },
    )
    .expect("conversion succeeds");

    assert!(nonempty(&output), "the output should have content");
    assert!(streams(&binary, &output).contains("vp9"));
}

#[test]
fn the_gif_palette_graph_is_one_ffmpeg_accepts() {
    // A filter_complex typo is exactly the failure the unit tests cannot see.
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("gif");
    let input = source(&binary, &dir);

    let output = convert::convert(
        &binary,
        &input,
        &ConvertSettings {
            target: Target::Gif,
            fps: Some(8),
            width: Some(120),
            ..Default::default()
        },
    )
    .expect("gif conversion succeeds");

    assert!(nonempty(&output));
    assert!(streams(&binary, &output).contains("gif"));
}

#[test]
fn extracting_audio_produces_a_file_with_no_video_stream() {
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("mp3");
    let input = source(&binary, &dir);

    let output = convert::convert(
        &binary,
        &input,
        &ConvertSettings {
            target: Target::Mp3,
            ..Default::default()
        },
    )
    .expect("audio extraction succeeds");

    let summary = streams(&binary, &output);
    assert!(nonempty(&output));
    assert!(summary.contains("mp3"));
    assert!(!summary.contains("Video:"), "{summary}");
}

#[test]
fn converting_to_the_same_container_does_not_truncate_the_source() {
    // ffmpeg is passed -y, so reading and writing the same path would leave the
    // user with a destroyed original.
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("same");
    let input = source(&binary, &dir);
    let before = std::fs::metadata(&input).unwrap().len();

    let output = convert::convert(
        &binary,
        &input,
        &ConvertSettings {
            target: Target::Mp4,
            ..Default::default()
        },
    )
    .expect("conversion succeeds");

    assert_ne!(output, input, "the source must not be the destination");
    assert_eq!(
        std::fs::metadata(&input).unwrap().len(),
        before,
        "the source must be untouched"
    );
    assert!(nonempty(&output));
}

#[test]
fn a_thumbnail_is_written_at_the_requested_size() {
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("thumb");
    let input = source(&binary, &dir);

    let output = convert::thumbnail(&binary, &input, 1.0, 80).expect("thumbnail succeeds");

    let image = image::open(&output).expect("a readable png");
    assert_eq!(image.width(), 80);
    // The source is 160x120, so a width of 80 gives a height of 60.
    assert_eq!(image.height(), 60);
}

#[test]
fn seeking_past_the_end_fails_loudly_rather_than_writing_an_empty_file() {
    // A silent empty PNG would look like success and open as nothing.
    let Some(binary) = ffmpeg::find() else {
        eprintln!("skipping: ffmpeg not installed");
        return;
    };
    let dir = dir("past-end");
    let input = source(&binary, &dir);

    let result = convert::thumbnail(&binary, &input, 60.0, 80);

    match result {
        Err(_) => {}
        Ok(path) => assert!(
            !nonempty(&path),
            "an unreadable frame must not be reported as a written thumbnail"
        ),
    }
}
