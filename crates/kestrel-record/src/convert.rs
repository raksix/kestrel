//! Video conversion and thumbnails, as ShareX's video converter and thumbnailer.
//!
//! Both are ffmpeg command lines, built here and run here so the arguments can
//! be tested without spawning anything. That matters more than it sounds: an
//! ffmpeg invocation is easy to get subtly wrong in a way that only shows up as
//! a corrupt file, and asserting on the argument list catches those in a
//! millisecond instead of a minute.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ffmpeg::{FfmpegError, Result, VideoCodec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    #[default]
    Mp4,
    Webm,
    Mkv,
    /// Animated GIF, with a palette generated from the clip.
    Gif,
    /// Audio only, for pulling the sound out of a screen recording.
    Mp3,
}

impl Target {
    pub fn extension(self) -> &'static str {
        match self {
            Target::Mp4 => "mp4",
            Target::Webm => "webm",
            Target::Mkv => "mkv",
            Target::Gif => "gif",
            Target::Mp3 => "mp3",
        }
    }

    /// The audio encoder the container can actually hold.
    ///
    /// This is not cosmetic: WebM only carries Vorbis or Opus, and handing it
    /// an AAC track makes ffmpeg write nothing at all and exit non-zero.
    fn audio_encoder(self) -> &'static str {
        match self {
            Target::Webm => "libopus",
            _ => "aac",
        }
    }

    fn codec(self) -> Option<VideoCodec> {
        match self {
            Target::Mp4 => Some(VideoCodec::H264),
            Target::Webm => Some(VideoCodec::Vp9),
            Target::Mkv => Some(VideoCodec::H264),
            Target::Gif | Target::Mp3 => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConvertSettings {
    pub target: Target,
    /// Constant rate factor. Lower is better quality and a bigger file.
    pub crf: u8,
    /// Output frame rate; `None` keeps the source's.
    pub fps: Option<u32>,
    /// Output width; height follows to preserve the aspect ratio. `None` keeps
    /// the source size.
    pub width: Option<u32>,
    /// Drop the audio track.
    pub mute: bool,
}

impl Default for ConvertSettings {
    fn default() -> Self {
        Self {
            target: Target::Mp4,
            crf: 23,
            fps: None,
            width: None,
            mute: false,
        }
    }
}

/// The argument list for a conversion.
///
/// GIF gets a two-pass palette in a single filter graph. The one-pass path
/// quantises to a fixed 216-colour web palette, which turns a screen recording
/// of an editor into a banded mess; generating the palette from the clip is the
/// difference between usable and not.
pub fn convert_args(input: &Path, output: &Path, settings: &ConvertSettings) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ];

    let mut filters: Vec<String> = Vec::new();
    if let Some(fps) = settings.fps.filter(|fps| *fps > 0) {
        filters.push(format!("fps={fps}"));
    }
    if let Some(width) = settings.width.filter(|width| *width > 0) {
        // -2 rather than -1: H.264 needs even dimensions, and an odd height is
        // a hard encoder error rather than a warning.
        filters.push(format!("scale={width}:-2:flags=lanczos"));
    }

    match settings.target {
        Target::Gif => {
            filters.push("split[a][b];[a]palettegen[p];[b][p]paletteuse".into());
            args.push("-filter_complex".into());
            args.push(filters.join(","));
            // A GIF has no sound to keep.
            args.push("-an".into());
        }
        Target::Mp3 => {
            // Video is dropped, not transcoded to nothing.
            args.push("-vn".into());
            args.push("-c:a".into());
            args.push("libmp3lame".into());
            args.push("-q:a".into());
            args.push("2".into());
        }
        _ => {
            if !filters.is_empty() {
                args.push("-vf".into());
                args.push(filters.join(","));
            }
            if let Some(codec) = settings.target.codec() {
                args.push("-c:v".into());
                args.push(codec.encoder().into());
                args.push("-crf".into());
                args.push(settings.crf.to_string());
                args.push("-pix_fmt".into());
                // Without this, players that only handle 4:2:0 show a green or
                // black frame — Safari and QuickTime among them.
                args.push("yuv420p".into());
            }
            if settings.mute {
                args.push("-an".into());
            } else {
                // Re-encode rather than copy: the source track may be in a
                // codec the target container cannot hold.
                args.push("-c:a".into());
                args.push(settings.target.audio_encoder().into());
            }
        }
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

/// The argument list for a single thumbnail frame.
///
/// `-ss` is placed before `-i` so ffmpeg seeks rather than decoding from the
/// start. On a long recording that is the difference between instant and
/// several seconds.
pub fn thumbnail_args(input: &Path, output: &Path, at_seconds: f32, width: u32) -> Vec<String> {
    vec![
        "-y".into(),
        "-ss".into(),
        format!("{:.3}", at_seconds.max(0.0)),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        format!("scale={}:-2:flags=lanczos", width.max(1)),
        output.to_string_lossy().into_owned(),
    ]
}

/// Run ffmpeg with `args`, turning a non-zero exit into an error that carries
/// the reason.
///
/// ffmpeg writes its diagnostics to stderr and exits non-zero, so discarding
/// stderr here would leave the user with "conversion failed" and nothing else.
pub fn run(ffmpeg: &Path, args: &[String]) -> Result<()> {
    let output = Command::new(ffmpeg).args(args).output()?;

    if !output.status.success() {
        return Err(FfmpegError::Failed {
            status: output.status.to_string(),
            stderr: tail(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(())
}

/// The last few lines of ffmpeg's output.
///
/// ffmpeg prints its whole build configuration before saying what went wrong,
/// and putting that in a dialog buries the one line that matters.
fn tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(6)..].join("\n")
}

/// Convert a video, choosing the output path from the target format.
pub fn convert(ffmpeg: &Path, input: &Path, settings: &ConvertSettings) -> Result<PathBuf> {
    let output = input.with_extension(settings.target.extension());

    // Refuse to overwrite the source. `-y` means ffmpeg would happily read and
    // write the same file, and the result is a truncated original.
    let output = if output == input {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        input.with_file_name(format!("{stem}-converted.{}", settings.target.extension()))
    } else {
        output
    };

    run(ffmpeg, &convert_args(input, &output, settings))?;
    Ok(output)
}

/// Write a thumbnail next to the video.
pub fn thumbnail(ffmpeg: &Path, input: &Path, at_seconds: f32, width: u32) -> Result<PathBuf> {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let output = input.with_file_name(format!("{stem}-thumb.png"));

    run(ffmpeg, &thumbnail_args(input, &output, at_seconds, width))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(settings: &ConvertSettings) -> Vec<String> {
        convert_args(Path::new("/in.mkv"), Path::new("/out.mp4"), settings)
    }

    fn joined(args: &[String]) -> String {
        args.join(" ")
    }

    #[test]
    fn the_input_and_output_bracket_the_options() {
        let args = args(&ConvertSettings::default());

        assert_eq!(args[1], "-i");
        assert_eq!(args[2], "/in.mkv");
        assert_eq!(args.last().unwrap(), "/out.mp4");
    }

    #[test]
    fn mp4_output_is_encoded_for_players_that_only_do_4_2_0() {
        // Without yuv420p, Safari and QuickTime show a green or black frame.
        let args = joined(&args(&ConvertSettings::default()));

        assert!(args.contains("-c:v libx264"), "{args}");
        assert!(args.contains("-pix_fmt yuv420p"), "{args}");
    }

    #[test]
    fn webm_uses_vp9_rather_than_the_mp4_encoder() {
        let args = joined(&args(&ConvertSettings {
            target: Target::Webm,
            ..Default::default()
        }));

        assert!(args.contains("-c:v libvpx-vp9"), "{args}");
    }

    #[test]
    fn gif_builds_its_palette_from_the_clip() {
        // The one-pass path quantises to a fixed web palette, which turns a
        // screen recording into a banded mess.
        let args = joined(&args(&ConvertSettings {
            target: Target::Gif,
            ..Default::default()
        }));

        assert!(args.contains("palettegen"), "{args}");
        assert!(args.contains("paletteuse"), "{args}");
        assert!(!args.contains("-c:v"), "a GIF has no video codec: {args}");
    }

    #[test]
    fn gif_output_has_no_audio_stream() {
        let args = joined(&args(&ConvertSettings {
            target: Target::Gif,
            ..Default::default()
        }));

        assert!(args.contains("-an"), "{args}");
    }

    #[test]
    fn audio_extraction_drops_the_video_rather_than_transcoding_it() {
        let args = joined(&args(&ConvertSettings {
            target: Target::Mp3,
            ..Default::default()
        }));

        assert!(args.contains("-vn"), "{args}");
        assert!(args.contains("libmp3lame"), "{args}");
    }

    #[test]
    fn scaling_keeps_dimensions_even() {
        // H.264 rejects an odd height outright, so -1 is not safe here.
        let args = joined(&args(&ConvertSettings {
            width: Some(640),
            ..Default::default()
        }));

        assert!(args.contains("scale=640:-2"), "{args}");
    }

    #[test]
    fn an_unset_size_and_rate_add_no_filter_at_all() {
        let args = args(&ConvertSettings::default());
        assert!(!args.iter().any(|arg| arg == "-vf"), "{args:?}");
    }

    #[test]
    fn a_zero_size_or_rate_is_ignored_rather_than_producing_a_broken_filter() {
        // `scale=0:-2` is a hard ffmpeg error, and a zero is what an emptied
        // number field sends.
        let args = joined(&args(&ConvertSettings {
            width: Some(0),
            fps: Some(0),
            ..Default::default()
        }));

        assert!(!args.contains("scale=0"), "{args}");
        assert!(!args.contains("fps=0"), "{args}");
    }

    #[test]
    fn both_filters_are_combined_into_one_chain() {
        let args = joined(&args(&ConvertSettings {
            width: Some(1280),
            fps: Some(24),
            ..Default::default()
        }));

        assert!(args.contains("fps=24,scale=1280:-2"), "{args}");
    }

    #[test]
    fn muting_drops_the_audio_and_not_muting_re_encodes_it() {
        // Copying could carry a codec the target container cannot hold.
        let muted = joined(&args(&ConvertSettings {
            mute: true,
            ..Default::default()
        }));
        let kept = joined(&args(&ConvertSettings::default()));

        assert!(muted.contains("-an"), "{muted}");
        assert!(kept.contains("-c:a aac"), "{kept}");
    }

    #[test]
    fn webm_gets_an_audio_codec_the_container_can_hold() {
        // WebM carries only Vorbis or Opus. Handing it AAC makes ffmpeg write
        // nothing at all and exit non-zero — caught by the integration test
        // against a real binary, which is what that test is for.
        let args = joined(&args(&ConvertSettings {
            target: Target::Webm,
            ..Default::default()
        }));

        assert!(args.contains("-c:a libopus"), "{args}");
        assert!(!args.contains("aac"), "{args}");
    }

    #[test]
    fn the_thumbnail_seeks_before_opening_the_input() {
        // -ss after -i decodes from the start, which on a long recording is
        // seconds instead of instant.
        let args = thumbnail_args(Path::new("/clip.mp4"), Path::new("/t.png"), 12.5, 320);

        let ss = args.iter().position(|a| a == "-ss").unwrap();
        let i = args.iter().position(|a| a == "-i").unwrap();

        assert!(ss < i, "{args:?}");
        assert_eq!(args[ss + 1], "12.500");
    }

    #[test]
    fn a_negative_timestamp_is_clamped_rather_than_passed_through() {
        let args = thumbnail_args(Path::new("/clip.mp4"), Path::new("/t.png"), -5.0, 320);
        assert_eq!(args[2], "0.000");
    }

    #[test]
    fn the_thumbnail_takes_exactly_one_frame() {
        let args = joined(&thumbnail_args(
            Path::new("/clip.mp4"),
            Path::new("/t.png"),
            1.0,
            320,
        ));

        assert!(args.contains("-frames:v 1"), "{args}");
    }

    #[test]
    fn a_zero_width_thumbnail_does_not_produce_a_broken_filter() {
        let args = joined(&thumbnail_args(
            Path::new("/clip.mp4"),
            Path::new("/t.png"),
            1.0,
            0,
        ));

        assert!(args.contains("scale=1:-2"), "{args}");
    }

    #[test]
    fn only_the_last_lines_of_an_ffmpeg_failure_are_kept() {
        // ffmpeg prints its whole build configuration before the actual error;
        // putting that in a dialog buries the one line that matters.
        let noise: String = (0..40)
            .map(|i| format!("configuration line {i}\n"))
            .collect::<String>()
            + "Error: No such file or directory\n";

        let message = tail(&noise);

        assert!(message.contains("No such file"));
        assert!(message.lines().count() <= 6);
        assert!(!message.contains("configuration line 0"));
    }

    #[test]
    fn blank_lines_do_not_crowd_out_the_error() {
        let message = tail("real error\n\n\n\n\n\n\n\n");
        assert!(message.contains("real error"));
    }

    #[test]
    fn settings_survive_a_json_round_trip() {
        let settings = ConvertSettings {
            target: Target::Webm,
            crf: 30,
            fps: Some(15),
            width: Some(800),
            mute: true,
        };
        let json = serde_json::to_string(&settings).unwrap();

        assert_eq!(
            serde_json::from_str::<ConvertSettings>(&json).unwrap(),
            settings
        );
    }
}
