//! Times each stage of the capture pipeline on a real screenshot.
//! `cargo run -p kestrel-capture --example bench`

use std::time::Instant;

use image::ImageFormat;
use kestrel_capture::{backend, CaptureBackend};

fn main() {
    let b = backend();

    let t = Instant::now();
    let capture = b.capture_all_displays().expect("capture");
    println!(
        "capture            {:>8.0?}  {}x{}",
        t.elapsed(),
        capture.width(),
        capture.height()
    );

    let image = capture.image;

    let t = Instant::now();
    let thumb = image::imageops::thumbnail(&image, 480, 312);
    println!("thumbnail          {:>8.0?}", t.elapsed());

    let t = Instant::now();
    let mut buf = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut buf, ImageFormat::Png).unwrap();
    println!(
        "thumb png encode   {:>8.0?}  {} bytes",
        t.elapsed(),
        buf.get_ref().len()
    );

    let t = Instant::now();
    let mut buf = std::io::Cursor::new(Vec::new());
    image.write_to(&mut buf, ImageFormat::Png).unwrap();
    println!(
        "full png encode    {:>8.0?}  {} bytes",
        t.elapsed(),
        buf.get_ref().len()
    );

    let t = Instant::now();
    let path = std::env::temp_dir().join("kestrel-bench.png");
    image.save_with_format(&path, ImageFormat::Png).unwrap();
    let default_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "save (default)     {:>8.0?}  {default_size} bytes",
        t.elapsed()
    );

    let t = Instant::now();
    let file = std::fs::File::create(&path).unwrap();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    image.write_with_encoder(encoder).unwrap();
    let fast_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "save (fast)        {:>8.0?}  {fast_size} bytes",
        t.elapsed()
    );
    let _ = std::fs::remove_file(&path);

    let t = Instant::now();
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            let data = arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: std::borrow::Cow::Borrowed(image.as_raw()),
            };
            match clipboard.set_image(data) {
                Ok(()) => println!("clipboard          {:>8.0?}", t.elapsed()),
                Err(e) => println!("clipboard FAILED   {:>8.0?}  {e}", t.elapsed()),
            }
        }
        Err(e) => println!("clipboard open failed: {e}"),
    }
}
