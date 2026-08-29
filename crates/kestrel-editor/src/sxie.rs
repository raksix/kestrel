//! Importing ShareX image-effect presets (`.sxie`).
//!
//! **The schema here is inferred, not documented.** ShareX publishes the `.sxcu`
//! format in detail but not `.sxie`; these files are Json.NET output from
//! `ShareX.ImageEffectsLib`, which means a `$type` discriminator naming a .NET
//! class and PascalCase properties. The importer is written to that shape and
//! to be tolerant of it being wrong in places.
//!
//! Two consequences follow, and both are deliberate:
//!
//! - An effect we do not recognise is **reported**, not dropped. Silently
//!   importing half a preset would hand someone a picture that is not the one
//!   they built, with no indication why.
//! - A property we cannot read falls back to a sensible default rather than
//!   failing the whole file, so one unfamiliar field cannot cost the rest.

use serde_json::Value;

use crate::effects::{Chain, Effect, Rotation};
use crate::shape::Color;

/// What an import produced, including what it could not handle.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub name: Option<String>,
    pub chain: Chain,
    /// Effects present in the file that Kestrel has no equivalent for.
    pub unsupported: Vec<String>,
}

impl Imported {
    /// Whether every effect in the file made it into the chain.
    pub fn is_complete(&self) -> bool {
        self.unsupported.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SxieError {
    #[error("not valid json: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("no effects found in the file")]
    NoEffects,
}

pub type Result<T> = std::result::Result<T, SxieError>;

pub fn import(json: &str) -> Result<Imported> {
    let root: Value = serde_json::from_str(json)?;

    // The list has appeared under a couple of names across ShareX versions, and
    // a bare array is also plausible.
    let effects = root
        .get("Effects")
        .or_else(|| root.get("effects"))
        .or_else(|| root.get("ImageEffects"))
        .or(if root.is_array() { Some(&root) } else { None })
        .and_then(Value::as_array)
        .ok_or(SxieError::NoEffects)?;

    let mut chain = Vec::new();
    let mut unsupported = Vec::new();

    for value in effects {
        let name = type_name(value);
        match convert(&name, value) {
            Some(effect) => chain.push(effect),
            None if name.is_empty() => unsupported.push("(unnamed effect)".to_string()),
            None => unsupported.push(name),
        }
    }

    Ok(Imported {
        name: root
            .get("Name")
            .or_else(|| root.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        chain: Chain::new(chain),
        unsupported,
    })
}

/// Pull the class name out of a Json.NET `$type`.
///
/// The value looks like `ShareX.ImageEffectsLib.Grayscale, ShareX.ImageEffectsLib`,
/// so the name is what sits between the last dot and the comma.
fn type_name(value: &Value) -> String {
    let raw = value
        .get("$type")
        .or_else(|| value.get("Type"))
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    raw.split(',')
        .next()
        .unwrap_or_default()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn convert(name: &str, value: &Value) -> Option<Effect> {
    // ShareX's class names are stable, but matching case-insensitively costs
    // nothing and survives a capitalisation change.
    Some(match name.to_ascii_lowercase().as_str() {
        "grayscale" | "greyscale" => Effect::Grayscale,
        "sepia" => Effect::Sepia,
        "inverse" | "invert" => Effect::Invert,

        "brightness" => Effect::Brightness {
            amount: number(value, &["Value", "Brightness"], 0.0),
        },
        "contrast" => Effect::Contrast {
            amount: number(value, &["Value", "Contrast"], 0.0),
        },
        "gamma" => Effect::Gamma {
            value: number(value, &["Value", "Gamma"], 1.0),
        },
        "saturation" | "saturate" => Effect::Saturation {
            amount: number(value, &["Value", "Saturation"], 0.0),
        },
        "alpha" | "opacity" => Effect::Opacity {
            amount: number(value, &["Value", "Alpha", "Opacity"], 1.0),
        },

        "blur" | "gaussianblur" => Effect::Blur {
            radius: number(value, &["Radius", "Size", "Value"], 5.0),
        },
        "sharpen" => Effect::Sharpen {
            amount: number(value, &["Value", "Amount"], 0.5),
        },
        "pixelate" => Effect::Pixelate {
            block: number(value, &["Size", "PixelSize", "Value"], 10.0).max(2.0) as u32,
        },

        "resize" => {
            // Resizing to an invented size is worse than not resizing at all,
            // so a Resize with neither dimension is reported instead.
            let width = number(value, &["Width"], 0.0);
            let height = number(value, &["Height"], 0.0);
            if width < 1.0 && height < 1.0 {
                return None;
            }
            Effect::Resize {
                width: width.max(1.0) as u32,
                height: height.max(1.0) as u32,
                keep_aspect: boolean(value, &["KeepAspectRatio", "AutoHeight"], true),
            }
        }
        "rotate" => Effect::Rotate {
            rotation: rotation(number(value, &["Angle", "Value"], 0.0)),
        },
        "flip" => Effect::Flip {
            horizontal: boolean(value, &["Horizontally", "Horizontal"], false),
            vertical: boolean(value, &["Vertically", "Vertical"], false),
        },
        "autocrop" => Effect::AutoCrop {
            tolerance: number(value, &["Tolerance"], 0.0).clamp(0.0, 255.0) as u8,
        },

        "border" | "outline" => Effect::Border {
            width: number(value, &["Size", "Width"], 1.0).max(0.0) as u32,
            color: colour(value, &["Color", "Colour"]).unwrap_or(Color::BLACK),
        },

        _ => return None,
    })
}

fn number(value: &Value, keys: &[&str], fallback: f32) -> f32 {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_f64))
        .map(|found| found as f32)
        .unwrap_or(fallback)
}

fn boolean(value: &Value, keys: &[&str], fallback: bool) -> bool {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_bool))
        .unwrap_or(fallback)
}

/// Colours appear as `#rrggbb`, as `rrggbb`, or as an ARGB object.
fn colour(value: &Value, keys: &[&str]) -> Option<Color> {
    let found = keys.iter().find_map(|key| value.get(key))?;

    if let Some(text) = found.as_str() {
        return Color::from_hex(text);
    }
    if found.is_object() {
        return Some(Color::rgba(
            number(found, &["R", "Red"], 0.0) as u8,
            number(found, &["G", "Green"], 0.0) as u8,
            number(found, &["B", "Blue"], 0.0) as u8,
            number(found, &["A", "Alpha"], 255.0) as u8,
        ));
    }
    None
}

/// ShareX stores an arbitrary angle; Kestrel only turns in quarters, so anything
/// else snaps to the nearest one. Refusing the effect would lose the intent
/// entirely, which is the worse trade.
fn rotation(angle: f32) -> Rotation {
    let normalised = ((angle % 360.0) + 360.0) % 360.0;
    match (normalised / 90.0).round() as i32 % 4 {
        1 => Rotation::Quarter,
        2 => Rotation::Half,
        3 => Rotation::ThreeQuarters,
        _ => Rotation::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sxie(effects: &str) -> String {
        format!(r#"{{"Name":"Test","Effects":{effects}}}"#)
    }

    fn typed(name: &str, extra: &str) -> String {
        let comma = if extra.is_empty() { "" } else { "," };
        format!(
            r#"{{"$type":"ShareX.ImageEffectsLib.{name}, ShareX.ImageEffectsLib"{comma}{extra}}}"#
        )
    }

    #[test]
    fn a_preset_name_is_kept() {
        let imported = import(&sxie(&format!("[{}]", typed("Grayscale", "")))).unwrap();
        assert_eq!(imported.name.as_deref(), Some("Test"));
    }

    #[test]
    fn the_dotnet_type_discriminator_is_understood() {
        let imported = import(&sxie(&format!("[{}]", typed("Grayscale", "")))).unwrap();

        assert_eq!(imported.chain.0, vec![Effect::Grayscale]);
        assert!(imported.is_complete());
    }

    #[test]
    fn effect_order_is_preserved() {
        // The chain is ordered data; importing it as a set would silently
        // change the picture.
        let json = sxie(&format!(
            "[{},{},{}]",
            typed("Grayscale", ""),
            typed("Blur", r#""Radius":3"#),
            typed("Sepia", "")
        ));
        let imported = import(&json).unwrap();

        assert_eq!(
            imported.chain.0,
            vec![
                Effect::Grayscale,
                Effect::Blur { radius: 3.0 },
                Effect::Sepia
            ]
        );
    }

    #[test]
    fn properties_are_read() {
        let json = sxie(&format!(
            "[{}]",
            typed("Border", r##""Size":6,"Color":"#ff0000""##)
        ));
        let imported = import(&json).unwrap();

        assert_eq!(
            imported.chain.0,
            vec![Effect::Border {
                width: 6,
                color: Color::rgb(255, 0, 0)
            }]
        );
    }

    #[test]
    fn a_colour_stored_as_an_object_is_read() {
        let json = sxie(&format!(
            "[{}]",
            typed(
                "Border",
                r#""Size":2,"Color":{"R":10,"G":20,"B":30,"A":255}"#
            )
        ));
        let imported = import(&json).unwrap();

        assert_eq!(
            imported.chain.0,
            vec![Effect::Border {
                width: 2,
                color: Color::rgba(10, 20, 30, 255)
            }]
        );
    }

    #[test]
    fn a_missing_property_falls_back_rather_than_failing_the_file() {
        // One unfamiliar field must not cost the rest of the preset.
        let json = sxie(&format!("[{}]", typed("Blur", "")));
        let imported = import(&json).unwrap();

        assert_eq!(imported.chain.len(), 1);
        assert!(imported.is_complete());
    }

    #[test]
    fn an_unknown_effect_is_reported_rather_than_dropped() {
        // Importing half a preset silently would hand someone a picture that is
        // not the one they built, with no indication why.
        let json = sxie(&format!(
            "[{},{}]",
            typed("Grayscale", ""),
            typed("ParticleEffect", "")
        ));
        let imported = import(&json).unwrap();

        assert_eq!(imported.chain.0, vec![Effect::Grayscale]);
        assert_eq!(imported.unsupported, ["ParticleEffect"]);
        assert!(!imported.is_complete());
    }

    #[test]
    fn a_resize_with_no_dimensions_is_reported_not_guessed() {
        let json = sxie(&format!("[{}]", typed("Resize", "")));
        let imported = import(&json).unwrap();

        assert!(imported.chain.is_empty());
        assert_eq!(imported.unsupported, ["Resize"]);
    }

    #[test]
    fn a_bare_array_of_effects_is_accepted() {
        // Not every file has a wrapper object.
        let json = format!("[{}]", typed("Sepia", ""));
        let imported = import(&json).unwrap();

        assert_eq!(imported.chain.0, vec![Effect::Sepia]);
        assert_eq!(imported.name, None);
    }

    #[test]
    fn a_file_with_no_effect_list_is_rejected() {
        assert!(matches!(
            import(r#"{"Name":"empty"}"#),
            Err(SxieError::NoEffects)
        ));
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(matches!(import("not json"), Err(SxieError::Parse(_))));
    }

    #[test]
    fn angles_snap_to_the_nearest_quarter_turn() {
        assert_eq!(rotation(0.0), Rotation::None);
        assert_eq!(rotation(90.0), Rotation::Quarter);
        assert_eq!(rotation(178.0), Rotation::Half);
        assert_eq!(rotation(-90.0), Rotation::ThreeQuarters);
        assert_eq!(rotation(450.0), Rotation::Quarter);
    }

    #[test]
    fn type_names_are_matched_case_insensitively() {
        let json = sxie(&format!("[{}]", typed("GRAYSCALE", "")));
        assert_eq!(import(&json).unwrap().chain.0, vec![Effect::Grayscale]);
    }

    #[test]
    fn an_imported_chain_actually_applies() {
        // The point of importing is to get the picture, so the round trip has
        // to reach real pixels.
        let json = sxie(&format!("[{}]", typed("Grayscale", "")));
        let chain = import(&json).unwrap().chain;

        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 50, 20, 255]));
        let result = chain.apply(&image);
        let pixel = result.get_pixel(0, 0);

        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
    }
}
