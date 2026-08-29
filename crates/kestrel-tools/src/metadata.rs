//! Reading and stripping image metadata, as ShareX's "metadata" tool.
//!
//! Stripping matters more than reading. A screenshot rarely carries EXIF, but a
//! photo dropped into Kestrel to be uploaded carries GPS coordinates, a device
//! serial and a timestamp — and publishing those by accident is the kind of
//! mistake that cannot be taken back.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub tag: String,
    pub value: String,
    /// Whether this field can identify a person, a place or a device.
    pub sensitive: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
}

pub type Result<T> = std::result::Result<T, MetadataError>;

/// Tags worth warning about before an upload.
///
/// GPS is the obvious one; a camera serial number identifies the device across
/// every photo it ever took, and the original timestamp places the person.
fn is_sensitive(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    tag.starts_with("gps")
        || tag.contains("serial")
        || tag.contains("owner")
        || tag.contains("artist")
        || tag.contains("copyright")
        || tag.contains("datetimeoriginal")
        || tag.contains("hostcomputer")
        || tag.contains("software")
}

/// Read metadata from a file.
///
/// Returns an empty list for formats with no metadata, rather than an error —
/// most screenshots are PNGs with nothing to report, and that is not a failure.
pub fn read(path: &Path) -> Result<Vec<Field>> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);

    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return Ok(Vec::new());
    };

    let mut fields: Vec<Field> = exif
        .fields()
        .map(|field| {
            let tag = field.tag.to_string();
            Field {
                sensitive: is_sensitive(&tag),
                value: field.display_value().with_unit(&exif).to_string(),
                tag,
            }
        })
        .collect();

    // Sensitive fields first: the point of showing this list is to notice them.
    fields.sort_by(|a, b| b.sensitive.cmp(&a.sensitive).then(a.tag.cmp(&b.tag)));
    Ok(fields)
}

/// Whether a file carries anything worth warning about.
pub fn has_sensitive(path: &Path) -> Result<bool> {
    Ok(read(path)?.iter().any(|field| field.sensitive))
}

/// Write a copy of the image with no metadata.
///
/// The pixels are decoded and re-encoded, which drops every ancillary chunk
/// rather than trying to remove known ones. Anything an editor invented and we
/// have never heard of goes too, which is the only way to be sure.
pub fn strip(source: &Path, destination: &Path) -> Result<()> {
    let image = image::open(source)?;

    // JPEG has no alpha channel and its encoder refuses an image that has one.
    // A PNG screenshot stripped to JPEG would otherwise fail outright, so drop
    // the channel deliberately rather than letting the encoder complain.
    let format = image::ImageFormat::from_path(destination).ok();
    if matches!(format, Some(image::ImageFormat::Jpeg)) {
        image.to_rgb8().save(destination)?;
    } else {
        image.save(destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kestrel-meta-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn gps_and_device_identifiers_are_flagged() {
        for tag in [
            "GPSLatitude",
            "GPSLongitude",
            "BodySerialNumber",
            "CameraOwnerName",
            "Artist",
            "DateTimeOriginal",
        ] {
            assert!(is_sensitive(tag), "{tag} should be flagged");
        }
    }

    #[test]
    fn ordinary_camera_settings_are_not_flagged() {
        // Flagging everything would train people to ignore the warning.
        for tag in ["ExposureTime", "FNumber", "ISOSpeed", "PixelXDimension"] {
            assert!(!is_sensitive(tag), "{tag} should not be flagged");
        }
    }

    #[test]
    fn a_png_with_no_metadata_reads_as_empty_rather_than_failing() {
        let dir = temp_dir("png");
        let path = dir.join("plain.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();

        assert_eq!(read(&path).unwrap(), Vec::new());
        assert!(!has_sensitive(&path).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(read(Path::new("/no/such/file.jpg")).is_err());
    }

    #[test]
    fn stripping_preserves_the_pixels() {
        // A privacy tool that quietly changed the image would be worse than
        // one that did nothing.
        let dir = temp_dir("strip");
        let source = dir.join("in.png");
        let destination = dir.join("out.png");

        let original = image::RgbaImage::from_fn(16, 12, |x, y| {
            image::Rgba([(x * 8) as u8, (y * 8) as u8, 90, 255])
        });
        original.save(&source).unwrap();

        strip(&source, &destination).unwrap();

        let stripped = image::open(&destination).unwrap().to_rgba8();
        assert_eq!(stripped.dimensions(), original.dimensions());
        assert_eq!(stripped, original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stripping_a_jpeg_leaves_no_exif_behind() {
        let dir = temp_dir("jpeg");
        let source = dir.join("in.jpg");
        let destination = dir.join("out.jpg");

        // JPEG has no alpha, so the source has to be written without one.
        image::RgbImage::from_pixel(32, 32, image::Rgb([200, 100, 50]))
            .save(&source)
            .unwrap();
        strip(&source, &destination).unwrap();

        assert!(read(&destination).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stripping_a_transparent_png_to_jpeg_drops_the_alpha_instead_of_failing() {
        // Screenshots are RGBA and JPEG cannot hold that; the encoder rejects
        // it outright, so the conversion has to be explicit.
        let dir = temp_dir("alpha");
        let source = dir.join("in.png");
        let destination = dir.join("out.jpg");

        image::RgbaImage::from_pixel(16, 16, image::Rgba([10, 20, 30, 128]))
            .save(&source)
            .unwrap();

        strip(&source, &destination).expect("should convert rather than refuse");
        assert!(destination.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
