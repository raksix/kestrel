//! QR codes: generate one, and read them out of a capture.
//!
//! Reading matters more than writing here — ShareX's "scan QR code"
//! after-capture task is how people get a link out of a screenshot of a poster
//! or a login screen, without retyping it.

use image::{DynamicImage, Luma, RgbaImage};

#[derive(Debug, thiserror::Error)]
pub enum QrError {
    #[error("nothing to encode")]
    Empty,
    #[error("the text is too long for a QR code")]
    TooLong,
    #[error("QR encoding failed: {0}")]
    Encode(String),
}

pub type Result<T> = std::result::Result<T, QrError>;

/// One QR code found in an image.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decoded {
    pub text: String,
    /// Where it was found, so the UI can point at it.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Render `text` as a QR code, `module_size` pixels per module.
pub fn encode(text: &str, module_size: u32) -> Result<RgbaImage> {
    if text.is_empty() {
        return Err(QrError::Empty);
    }

    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|err| match err {
        qrcode::types::QrError::DataTooLong => QrError::TooLong,
        other => QrError::Encode(other.to_string()),
    })?;

    // A quiet zone is part of the spec, not decoration: without it many
    // scanners will not lock on.
    let image = code
        .render::<Luma<u8>>()
        .module_dimensions(module_size.max(1), module_size.max(1))
        .quiet_zone(true)
        .build();

    Ok(DynamicImage::ImageLuma8(image).to_rgba8())
}

/// Find and decode every QR code in an image.
///
/// Returns an empty list rather than an error when there is nothing to find —
/// "no QR code here" is an ordinary outcome of scanning a screenshot, not a
/// failure.
pub fn decode(image: &RgbaImage) -> Vec<Decoded> {
    let grey = DynamicImage::ImageRgba8(image.clone()).to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(grey);

    prepared
        .detect_grids()
        .into_iter()
        .filter_map(|grid| {
            let (_meta, text) = grid.decode().ok()?;
            let bounds = grid.bounds;

            // The corners come back in image order; the bounding box is what a
            // UI can actually draw.
            let xs = bounds.iter().map(|p| p.x);
            let ys = bounds.iter().map(|p| p.y);
            let min_x = xs.clone().min()?.max(0) as u32;
            let min_y = ys.clone().min()?.max(0) as u32;
            let max_x = xs.max()?.max(0) as u32;
            let max_y = ys.max()?.max(0) as u32;

            Some(Decoded {
                text,
                x: min_x,
                y: min_y,
                width: max_x.saturating_sub(min_x),
                height: max_y.saturating_sub(min_y),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_code_reads_back() {
        // The only test that really matters: the two halves have to agree.
        let text = "https://example.com/kestrel";
        let image = encode(text, 8).expect("should encode");

        let found = decode(&image);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, text);
    }

    #[test]
    fn turkish_text_survives_the_round_trip() {
        let text = "Merhaba dünya — ğüşiöç";
        let image = encode(text, 8).unwrap();
        assert_eq!(decode(&image).first().map(|d| d.text.as_str()), Some(text));
    }

    #[test]
    fn a_code_reports_where_it_was_found() {
        let image = encode("hello", 6).unwrap();
        let found = &decode(&image)[0];

        assert!(found.width > 0 && found.height > 0);
        // The quiet zone means the code does not start at the very edge.
        assert!(found.x > 0 && found.y > 0);
    }

    #[test]
    fn a_larger_module_size_makes_a_larger_image() {
        let small = encode("hello", 2).unwrap();
        let large = encode("hello", 10).unwrap();
        assert!(large.width() > small.width());
    }

    #[test]
    fn an_image_with_no_code_yields_nothing_rather_than_an_error() {
        // Scanning a screenshot that has no QR code is ordinary, not a failure.
        let blank = RgbaImage::from_pixel(200, 200, image::Rgba([255, 255, 255, 255]));
        assert!(decode(&blank).is_empty());
    }

    #[test]
    fn empty_text_is_rejected() {
        assert!(matches!(encode("", 8), Err(QrError::Empty)));
    }

    #[test]
    fn text_beyond_the_format_limit_is_reported_as_too_long() {
        // A QR code tops out around 3 KB; the error should say which limit was
        // hit rather than surfacing a library message.
        let huge = "a".repeat(8000);
        assert!(matches!(encode(&huge, 4), Err(QrError::TooLong)));
    }

    #[test]
    fn a_zero_module_size_still_produces_an_image() {
        // Guarding here keeps a bad setting from producing a zero-sized image
        // that no encoder will write.
        let image = encode("hello", 0).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
    }
}
