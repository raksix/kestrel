//! Finding ffmpeg, and building the command line for an encode.
//!
//! Kestrel captures frames itself and pipes them to ffmpeg as raw RGBA, rather
//! than letting ffmpeg grab the screen. That keeps one code path across
//! platforms — the alternative is `avfoundation`, `gdigrab` and `x11grab`, each
//! with its own device discovery, region syntax and quirks — and it means
//! region and window recording come for free from the capture layer that
//! already exists.
//!
//! ShareX bundles ffmpeg the same way, for the same reasons.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    Vp9,
    Av1,
}

impl VideoCodec {
    pub(crate) fn encoder(self) -> &'static str {
        match self {
            VideoCodec::H264 => "libx264",
            VideoCodec::Hevc => "libx265",
            VideoCodec::Vp9 => "libvpx-vp9",
            VideoCodec::Av1 => "libsvtav1",
        }
    }

    /// Container that suits the codec when the caller has no preference.
    pub fn container(self) -> &'static str {
        match self {
            VideoCodec::H264 | VideoCodec::Hevc => "mp4",
            VideoCodec::Vp9 => "webm",
            VideoCodec::Av1 => "mkv",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Video,
    /// Animated GIF, as ShareX's "screen recording (GIF)".
    Gif,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RecordSettings {
    pub fps: u32,
    pub codec: VideoCodec,
    /// Constant rate factor. Lower is better quality and a bigger file.
    pub crf: u8,
    pub format: OutputFormat,
    /// Audio to mix in. Silent by default — see `audio::AudioSettings`.
    #[serde(default)]
    pub audio: crate::audio::AudioSettings,
}

impl Default for RecordSettings {
    fn default() -> Self {
        Self {
            fps: 30,
            codec: VideoCodec::H264,
            crf: 23,
            format: OutputFormat::Video,
            audio: crate::audio::AudioSettings::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FfmpegError {
    #[error("ffmpeg is not installed. Install it and try again — on macOS: brew install ffmpeg")]
    NotFound,
    #[error("ffmpeg could not be started: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffmpeg exited with status {status}: {stderr}")]
    Failed { status: String, stderr: String },
}

pub type Result<T> = std::result::Result<T, FfmpegError>;

/// Locate ffmpeg.
///
/// `PATH` first, then the places package managers put it. A GUI application
/// launched from Finder or the Dock does not inherit a login shell's `PATH`, so
/// a Homebrew install would otherwise look missing to the app and present to
/// the user.
pub fn find() -> Option<PathBuf> {
    if let Ok(configured) = std::env::var("KESTREL_FFMPEG") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(found) = which("ffmpeg") {
        return Some(found);
    }

    const COMMON: &[&str] = &[
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        "/snap/bin/ffmpeg",
        "C:\\ffmpeg\\bin\\ffmpeg.exe",
        "C:\\Program Files\\ffmpeg\\bin\\ffmpeg.exe",
    ];
    COMMON.iter().map(PathBuf::from).find(|path| path.is_file())
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// The version string ffmpeg reports, for the settings UI.
pub fn version(ffmpeg: &std::path::Path) -> Result<String> {
    let output = Command::new(ffmpeg).arg("-version").output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .next()
        .unwrap_or("bilinmeyen sürüm")
        .to_string())
}

/// Build the argument list for encoding raw RGBA frames to `output`.
///
/// `width` and `height` are the frame size as captured; the arguments correct
/// for odd dimensions themselves (see below).
pub fn encode_args(
    width: u32,
    height: u32,
    settings: &RecordSettings,
    output: &std::path::Path,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        // Overwrite: the caller has already chosen a unique filename.
        "-y".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        "rgba".into(),
        "-video_size".into(),
        format!("{width}x{height}"),
        "-framerate".into(),
        settings.fps.max(1).to_string(),
        "-i".into(),
        "-".into(),
    ];

    // The audio input is a second `-i`, so it goes after the video input and
    // before any encoder option. A GIF has no audio stream to put it in.
    if settings.format == OutputFormat::Video {
        args.extend(crate::audio::input_args(&settings.audio));
    }

    match settings.format {
        OutputFormat::Gif => {
            // One pass: generate a palette from the whole clip and apply it.
            // Without a palette a GIF is quantised to a fixed 216-colour cube
            // and screenshots of text come out visibly banded.
            args.push("-lavfi".into());
            args.push(format!(
                "{EVEN_DIMENSIONS},split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a"
            ));
        }
        OutputFormat::Video => {
            args.push("-c:v".into());
            args.push(settings.codec.encoder().into());
            args.push("-crf".into());
            args.push(settings.crf.clamp(0, 63).to_string());

            if matches!(settings.codec, VideoCodec::H264 | VideoCodec::Hevc) {
                args.push("-preset".into());
                args.push("medium".into());
            }

            // yuv420p is what players and browsers actually accept, and it
            // requires even dimensions — a region drag produces odd ones about
            // half the time.
            args.push("-vf".into());
            args.push(EVEN_DIMENSIONS.into());
            args.push("-pix_fmt".into());
            args.push("yuv420p".into());

            if matches!(settings.codec, VideoCodec::Hevc) {
                // Without this tag QuickTime refuses to open the file.
                args.push("-tag:v".into());
                args.push("hvc1".into());
            }
            if settings.codec.container() == "mp4" {
                // Move the index to the front so the file plays while it is
                // still being copied or streamed.
                args.push("-movflags".into());
                args.push("+faststart".into());
            }
        }
    }

    if settings.format == OutputFormat::Video {
        args.extend(crate::audio::encode_args(&settings.audio));
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

/// Round both dimensions down to even numbers.
const EVEN_DIMENSIONS: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

#[cfg(test)]
mod tests {

    #[test]
    fn a_silent_recording_mentions_no_audio_at_all() {
        // The default has to stay silent, and the absence has to be total: a
        // stray -c:a with no input makes ffmpeg fail rather than record.
        let args = encode_args(
            640,
            480,
            &RecordSettings::default(),
            std::path::Path::new("/out.mp4"),
        )
        .join(" ");

        assert!(!args.contains("-c:a"), "{args}");
        assert!(!args.contains("avfoundation"), "{args}");
    }

    #[test]
    fn the_audio_input_comes_after_the_video_input_and_before_the_output() {
        // ffmpeg reads options positionally. An -i after the encoder options,
        // or after the output path, is a different command entirely.
        let settings = RecordSettings {
            audio: crate::audio::AudioSettings {
                device: Some("mic".into()),
                bitrate_kbps: 128,
            },
            ..Default::default()
        };
        let args = encode_args(640, 480, &settings, std::path::Path::new("/out.mp4"));

        let inputs: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, arg)| *arg == "-i")
            .map(|(i, _)| i)
            .collect();
        let codec = args
            .iter()
            .position(|a| a == "-c:v")
            .expect("a video codec");
        let output = args.len() - 1;

        assert_eq!(
            inputs.len(),
            2,
            "one raw video input and one audio: {args:?}"
        );
        assert!(inputs[1] < codec, "{args:?}");
        assert!(codec < output, "{args:?}");
    }

    #[test]
    fn a_gif_never_gets_an_audio_stream_even_when_one_is_configured() {
        // A GIF cannot carry sound, and asking ffmpeg to put it there fails.
        let settings = RecordSettings {
            format: OutputFormat::Gif,
            audio: crate::audio::AudioSettings {
                device: Some("mic".into()),
                bitrate_kbps: 128,
            },
            ..Default::default()
        };
        let args = encode_args(640, 480, &settings, std::path::Path::new("/out.gif")).join(" ");

        assert!(!args.contains("-c:a"), "{args}");
        assert!(!args.contains("mic"), "{args}");
    }

    use super::*;
    use std::path::Path;

    fn args_for(settings: &RecordSettings) -> Vec<String> {
        encode_args(1280, 720, settings, Path::new("/tmp/out.mp4"))
    }

    fn contains_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2)
            .any(|pair| pair[0] == flag && pair[1] == value)
    }

    #[test]
    fn the_input_is_described_as_raw_rgba_frames_on_stdin() {
        let args = args_for(&RecordSettings::default());

        assert!(contains_pair(&args, "-f", "rawvideo"));
        assert!(contains_pair(&args, "-pixel_format", "rgba"));
        assert!(contains_pair(&args, "-video_size", "1280x720"));
        assert!(contains_pair(&args, "-i", "-"));
    }

    #[test]
    fn odd_dimensions_are_rounded_to_even() {
        // yuv420p rejects odd dimensions, and a region drag produces them
        // roughly half the time. Without this every other recording fails.
        let args = encode_args(
            1281,
            721,
            &RecordSettings::default(),
            Path::new("/tmp/o.mp4"),
        );
        assert!(contains_pair(&args, "-vf", EVEN_DIMENSIONS));
        assert!(contains_pair(&args, "-pix_fmt", "yuv420p"));
    }

    #[test]
    fn each_codec_selects_its_encoder() {
        for (codec, encoder) in [
            (VideoCodec::H264, "libx264"),
            (VideoCodec::Hevc, "libx265"),
            (VideoCodec::Vp9, "libvpx-vp9"),
            (VideoCodec::Av1, "libsvtav1"),
        ] {
            let args = args_for(&RecordSettings {
                codec,
                ..Default::default()
            });
            assert!(contains_pair(&args, "-c:v", encoder), "{codec:?}");
        }
    }

    #[test]
    fn hevc_is_tagged_so_quicktime_will_open_it() {
        let args = args_for(&RecordSettings {
            codec: VideoCodec::Hevc,
            ..Default::default()
        });
        assert!(contains_pair(&args, "-tag:v", "hvc1"));
    }

    #[test]
    fn mp4_output_is_made_streamable_but_webm_is_not_asked_to_be() {
        let h264 = args_for(&RecordSettings::default());
        assert!(contains_pair(&h264, "-movflags", "+faststart"));

        let vp9 = args_for(&RecordSettings {
            codec: VideoCodec::Vp9,
            ..Default::default()
        });
        assert!(
            !vp9.iter().any(|a| a == "-movflags"),
            "faststart is an mp4 flag and errors on webm"
        );
    }

    #[test]
    fn gif_output_generates_a_palette() {
        // The default GIF quantiser bands text badly; a per-clip palette is the
        // difference between readable and not.
        let args = args_for(&RecordSettings {
            format: OutputFormat::Gif,
            ..Default::default()
        });
        let filter = args
            .windows(2)
            .find(|pair| pair[0] == "-lavfi")
            .map(|pair| pair[1].clone())
            .expect("a filter graph");

        assert!(filter.contains("palettegen"));
        assert!(filter.contains("paletteuse"));
        assert!(filter.contains(EVEN_DIMENSIONS));
        assert!(
            !args.iter().any(|a| a == "-c:v"),
            "the filter picks the encoder"
        );
    }

    #[test]
    fn the_frame_rate_is_never_zero() {
        // A zero frame rate makes ffmpeg divide by zero and abort.
        let args = args_for(&RecordSettings {
            fps: 0,
            ..Default::default()
        });
        assert!(contains_pair(&args, "-framerate", "1"));
    }

    #[test]
    fn the_crf_is_clamped_to_a_range_ffmpeg_accepts() {
        let args = args_for(&RecordSettings {
            crf: 200,
            ..Default::default()
        });
        assert!(contains_pair(&args, "-crf", "63"));
    }

    #[test]
    fn the_output_path_comes_last() {
        let args = encode_args(
            640,
            480,
            &RecordSettings::default(),
            Path::new("/tmp/kayıt 1.mp4"),
        );
        assert_eq!(args.last().map(String::as_str), Some("/tmp/kayıt 1.mp4"));
    }

    #[test]
    fn each_codec_has_a_container_that_can_hold_it() {
        assert_eq!(VideoCodec::H264.container(), "mp4");
        assert_eq!(VideoCodec::Hevc.container(), "mp4");
        assert_eq!(VideoCodec::Vp9.container(), "webm");
        assert_eq!(VideoCodec::Av1.container(), "mkv");
    }

    #[test]
    fn settings_survive_a_json_round_trip() {
        let settings = RecordSettings {
            fps: 60,
            codec: VideoCodec::Hevc,
            crf: 18,
            format: OutputFormat::Gif,
            audio: crate::audio::AudioSettings {
                device: Some("mic".into()),
                bitrate_kbps: 160,
            },
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<RecordSettings>(&json).unwrap(),
            settings
        );
    }

    #[test]
    fn older_settings_files_load_with_defaults() {
        let settings: RecordSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, RecordSettings::default());
    }
}
