//! Print the audio inputs ffmpeg can see on this machine.
//!
//! `cargo run -p kestrel-record --example audio_probe`
//!
//! The unit tests parse captured listings, which cannot catch ffmpeg changing
//! its output or a platform printing a shape nobody anticipated. This runs the
//! real binary. It already earned its keep once: it showed ffmpeg's own
//! "Error opening input: Input/output error" line being offered as a
//! microphone, which would have failed at record time.

fn main() {
    let Some(ffmpeg) = kestrel_record::ffmpeg::find() else {
        eprintln!("ffmpeg is not installed");
        return;
    };

    let devices = kestrel_record::audio::devices(&ffmpeg);
    println!(
        "{} audio input(s) via {}:",
        devices.len(),
        kestrel_record::audio::input_format()
    );

    for device in &devices {
        let hint = if device.likely_loopback {
            "  (looks like system output)"
        } else {
            ""
        };
        println!("  {}{hint}", device.name);
    }

    if let Some(note) = kestrel_record::audio::system_audio_note() {
        println!("\n{note}");
    }
}
