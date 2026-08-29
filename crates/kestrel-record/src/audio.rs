//! Recording audio alongside the screen.
//!
//! **What this can and cannot do, plainly.** Microphone capture works on all
//! three platforms through ffmpeg's native input device. Capturing the *system*
//! output — what you hear — does not have a portable answer:
//!
//! - **Windows** has WASAPI loopback, so it works with no extra software.
//! - **Linux** has PulseAudio and PipeWire monitor sources, so it works.
//! - **macOS** has no such thing. Apple provides no API for recording another
//!   process's output, and `avfoundation` exposes only real input devices. It
//!   needs a loopback driver — BlackHole, Loopback, Soundflower — installed and
//!   selected as an input.
//!
//! So macOS system audio is reported as unavailable with that explanation
//! rather than being offered and then silently producing a silent track. A
//! recording that looks fine and has no sound is the failure people notice an
//! hour later, when the meeting is over.
//!
//! **How far this is tested.** Device enumeration runs against the real ffmpeg
//! in `examples/audio_probe.rs`, and doing so caught ffmpeg's own error line
//! being listed as a microphone. The command construction is covered by unit
//! tests. Actually capturing from a device is *not* covered: it needs a
//! microphone permission and it would record whatever is in the room, neither
//! of which belongs in a test suite. That path is verified by running the app.

use std::process::Command;

use serde::{Deserialize, Serialize};

/// Which audio, if any, to mix into a recording.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// The ffmpeg device name to record from, as reported by `devices()`.
    ///
    /// `None` records no audio, which is the default: a screen recording that
    /// unexpectedly contains the room is a privacy problem, not a feature.
    pub device: Option<String>,
    /// Encoder bitrate in kbit/s.
    pub bitrate_kbps: u32,
}

impl AudioSettings {
    pub fn enabled(&self) -> bool {
        self.device.is_some()
    }

    pub fn bitrate(&self) -> u32 {
        // A zero from an emptied number field would make ffmpeg fail.
        if self.bitrate_kbps == 0 {
            128
        } else {
            self.bitrate_kbps.clamp(32, 320)
        }
    }
}

/// One capturable audio input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// What ffmpeg calls it, which is what goes back in `AudioSettings`.
    pub id: String,
    pub name: String,
    /// True when this is believed to carry the system's output rather than a
    /// microphone. A guess from the name, so it is a hint for the UI to sort
    /// by — not a promise.
    pub likely_loopback: bool,
}

/// Whether this platform can record system output without extra software, and
/// what to say when it cannot.
pub fn system_audio_note() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        Some(
            "macOS'ta sistem sesini kaydetmek için sanal bir ses aygıtı gerekir \
             (BlackHole, Loopback). Apple başka bir uygulamanın çıkışını kaydetmek \
             için bir API sunmuyor. Kurulu bir aygıt varsa aşağıdaki listede görünür.",
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The input device ffmpeg uses on this platform.
pub fn input_format() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "avfoundation"
    }
    #[cfg(target_os = "windows")]
    {
        "dshow"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "pulse"
    }
}

/// Ask ffmpeg what audio inputs exist.
///
/// ffmpeg lists devices by being asked to open a device that does not exist and
/// printing the catalogue while it complains, so a non-zero exit here is the
/// expected outcome and the output is on stderr.
pub fn devices(ffmpeg: &std::path::Path) -> Vec<Device> {
    let output = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-f",
            input_format(),
            "-list_devices",
            "true",
            "-i",
            "",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    parse_devices(&String::from_utf8_lossy(&output.stderr))
}

/// Pull device names out of ffmpeg's listing.
///
/// The formats differ per platform, so this matches the shape they share: a
/// bracketed index or a quoted name on a line that mentions audio.
fn parse_devices(listing: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    let mut in_audio_section = false;

    for line in listing.lines() {
        let lower = line.to_lowercase();

        // avfoundation prints a "AVFoundation audio devices:" header and then
        // indexes; before it, the same index numbers mean cameras.
        if lower.contains("audio devices") {
            in_audio_section = true;
            continue;
        }
        if lower.contains("video devices") {
            in_audio_section = false;
            continue;
        }

        // dshow marks each line instead of using sections.
        let dshow_audio = lower.contains("(audio)");
        if !in_audio_section && !dshow_audio {
            continue;
        }

        if let Some(name) = device_name(line) {
            if name.trim().is_empty() {
                continue;
            }
            devices.push(Device {
                likely_loopback: looks_like_loopback(&name),
                id: name.clone(),
                name,
            });
        }
    }

    devices
}

/// The device name in one line of ffmpeg's listing.
///
/// ffmpeg prefixes every line with `[component @ address] `, and each platform
/// writes the entry differently after that:
///
/// - avfoundation: `[1] Built-in Microphone`
/// - dshow: `"Stereo Mix" (audio)`
/// - pulse: `0  alsa_output.pci-0000_00_1f.3.analog-stereo.monitor`
///
/// The index is what makes an entry an entry. Requiring it is what keeps
/// ffmpeg's own diagnostics out of the list — `[in#0 @ 0x…] Error opening
/// input: Input/output error` is printed right after the devices, and without
/// this check it was being offered as a microphone.
///
/// The pulse form here is written from ffmpeg's documented output and has not
/// been run against a real PulseAudio host.
fn device_name(line: &str) -> Option<String> {
    if let Some(quoted) = line.split('"').nth(1) {
        return Some(quoted.to_string());
    }

    // Drop ffmpeg's `[component @ address] ` prefix, if there is one.
    let entry = match line.split_once("] ") {
        Some((_, rest)) => rest,
        None => line,
    }
    .trim();

    // `[1] Name`
    if let Some(rest) = entry.strip_prefix('[') {
        let (index, name) = rest.split_once("] ")?;
        return all_digits(index).then(|| name.trim().to_string());
    }

    // `0  Name`
    let (index, name) = entry.split_once(char::is_whitespace)?;
    all_digits(index).then(|| name.trim().to_string())
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A guess at whether a device carries system output.
///
/// Only a hint for sorting. Naming is a convention, not a contract, and a
/// device called "Meeting Audio" could be either.
fn looks_like_loopback(name: &str) -> bool {
    const HINTS: [&str; 7] = [
        "blackhole",
        "loopback",
        "soundflower",
        "stereo mix",
        "monitor",
        "what u hear",
        "wasapi",
    ];
    let lower = name.to_lowercase();
    HINTS.iter().any(|hint| lower.contains(hint))
}

/// ffmpeg arguments that add an audio input, to be inserted before the output.
///
/// Returned separately from the video arguments because the input has to be
/// declared before the codec options and the output path, and splicing it into
/// an already-assembled list is how the ordering gets broken.
pub fn input_args(settings: &AudioSettings) -> Vec<String> {
    let Some(device) = settings.device.as_deref() else {
        return Vec::new();
    };

    vec![
        "-f".into(),
        input_format().into(),
        // Audio arrives on its own clock, so let ffmpeg drop or duplicate
        // samples to keep it aligned with the frames we push. Without this the
        // sound drifts out of sync over a long recording.
        "-thread_queue_size".into(),
        "512".into(),
        "-i".into(),
        audio_input_spec(device),
    ]
}

/// How the device is named on the command line.
///
/// avfoundation wants `video:audio`, and a leading colon means "audio only" —
/// which is what we want, because the frames come in over stdin.
fn audio_input_spec(device: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        format!(":{device}")
    }
    #[cfg(target_os = "windows")]
    {
        format!("audio={device}")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        device.to_string()
    }
}

/// Encoder options for the audio stream.
pub fn encode_args(settings: &AudioSettings) -> Vec<String> {
    if !settings.enabled() {
        return Vec::new();
    }

    vec![
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        format!("{}k", settings.bitrate()),
        // Stop when the video does. Without it ffmpeg keeps recording audio
        // after the frame pump closes and the file ends with silence.
        "-shortest".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_device_means_no_audio_anywhere_in_the_command() {
        // The default has to be silent: a screen recording that unexpectedly
        // contains the room is a privacy problem, not a feature.
        let settings = AudioSettings::default();

        assert!(!settings.enabled());
        assert!(input_args(&settings).is_empty());
        assert!(encode_args(&settings).is_empty());
    }

    #[test]
    fn a_chosen_device_produces_an_input_and_an_encoder() {
        let settings = AudioSettings {
            device: Some("Built-in Microphone".into()),
            bitrate_kbps: 192,
        };

        let input = input_args(&settings).join(" ");
        let encode = encode_args(&settings).join(" ");

        assert!(input.contains("Built-in Microphone"), "{input}");
        assert!(encode.contains("-c:a aac"), "{encode}");
        assert!(encode.contains("192k"), "{encode}");
    }

    #[test]
    fn the_audio_stream_stops_with_the_video() {
        // Without -shortest, ffmpeg keeps recording after the frame pump closes
        // and the file ends with a tail of silence.
        let settings = AudioSettings {
            device: Some("mic".into()),
            bitrate_kbps: 128,
        };

        assert!(encode_args(&settings).contains(&"-shortest".to_string()));
    }

    #[test]
    fn a_zero_bitrate_falls_back_rather_than_failing_the_encode() {
        // Zero is what an emptied number field sends, and ffmpeg rejects it.
        let settings = AudioSettings {
            device: Some("mic".into()),
            bitrate_kbps: 0,
        };

        assert_eq!(settings.bitrate(), 128);
    }

    #[test]
    fn an_absurd_bitrate_is_clamped() {
        let settings = AudioSettings {
            device: Some("mic".into()),
            bitrate_kbps: 100_000,
        };

        assert_eq!(settings.bitrate(), 320);
    }

    #[test]
    fn avfoundation_audio_devices_are_read_and_cameras_are_not() {
        // The same index numbers appear under both headings, so a parser that
        // ignored the sections would offer the webcam as a microphone.
        let listing = "\
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] FaceTime HD Camera
[AVFoundation indev @ 0x1] [1] Capture screen 0
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] Built-in Microphone
[AVFoundation indev @ 0x1] [1] BlackHole 2ch
";

        let devices = parse_devices(listing);
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();

        assert_eq!(names, ["Built-in Microphone", "BlackHole 2ch"]);
    }

    #[test]
    fn dshow_devices_are_read_from_quoted_names() {
        let listing = r#"
[dshow @ 0x1] "Integrated Camera" (video)
[dshow @ 0x1] "Microphone Array" (audio)
[dshow @ 0x1] "Stereo Mix" (audio)
"#;

        let names: Vec<String> = parse_devices(listing).into_iter().map(|d| d.name).collect();

        assert_eq!(names, ["Microphone Array", "Stereo Mix"]);
    }

    #[test]
    fn loopback_devices_are_flagged_for_the_ui_to_sort_by() {
        assert!(looks_like_loopback("BlackHole 2ch"));
        assert!(looks_like_loopback("Stereo Mix"));
        assert!(looks_like_loopback("Monitor of Built-in Audio"));

        assert!(!looks_like_loopback("Built-in Microphone"));
        assert!(!looks_like_loopback("Yeti Nano"));
    }

    #[test]
    fn ffmpegs_own_error_lines_are_not_offered_as_devices() {
        // ffmpeg prints these immediately after the device list, because
        // listing devices *is* a failed open. Found by running the real binary:
        // "Error opening input: Input/output error" was showing up in the
        // picker and would have failed at record time.
        let listing = "\
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] furkan Mikrofonu
[in#0 @ 0x2] Error opening input: Input/output error
Error opening input file .
Error opening input files: Input/output error
";

        let names: Vec<String> = parse_devices(listing).into_iter().map(|d| d.name).collect();

        assert_eq!(names, ["furkan Mikrofonu"]);
    }

    #[test]
    fn pulse_style_entries_are_read() {
        // Written from ffmpeg's documented output; not verified against a real
        // PulseAudio host.
        let listing = "\
[pulse @ 0x1] audio devices:
[pulse @ 0x1] 0  alsa_input.pci-0000_00_1f.3.analog-stereo
[pulse @ 0x1] 1  alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
";

        let devices = parse_devices(listing);

        assert_eq!(devices.len(), 2);
        assert!(devices[1].likely_loopback, "a monitor source is loopback");
    }

    #[test]
    fn an_empty_or_unrecognised_listing_yields_nothing_rather_than_junk() {
        // Offering a garbage device name would fail at record time, which is
        // the worst moment to find out.
        assert!(parse_devices("").is_empty());
        assert!(parse_devices("ffmpeg version 7.1\nbuilt with clang").is_empty());
    }

    #[test]
    fn settings_survive_a_json_round_trip() {
        let settings = AudioSettings {
            device: Some("mic".into()),
            bitrate_kbps: 160,
        };
        let json = serde_json::to_string(&settings).unwrap();

        assert_eq!(
            serde_json::from_str::<AudioSettings>(&json).unwrap(),
            settings
        );
    }
}
