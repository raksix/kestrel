//! Running each subcommand.
//!
//! Two rules hold throughout.
//!
//! Results go to standard output and diagnostics to standard error, so
//! `kestrel hash x --json | jq` cannot be corrupted by a warning landing in the
//! middle of the JSON.
//!
//! A command that answers a yes/no question exits non-zero for "no". That is
//! what makes `kestrel hash file --expect abc123 && deploy` mean something.

use std::process::ExitCode;

use crate::{Command, ConvertTarget, IndexFormat};

pub type Result<T> = std::result::Result<T, String>;

pub fn run(command: Command, json: bool) -> Result<ExitCode> {
    match command {
        Command::Hash { path, expect } => hash(&path, expect.as_deref(), json),
        Command::Metadata { path, strip } => metadata(&path, strip, json),
        Command::Qr { path } => qr(&path, json),
        Command::Analyze { path } => analyze(&path, json),
        Command::Compare {
            first,
            second,
            tolerance,
            diff,
        } => compare(&first, &second, tolerance, diff.as_deref(), json),
        Command::Color { path, x, y, radius } => color(&path, x, y, radius, json),
        Command::Index { path, format, out } => index(&path, format, out.as_deref()),
        Command::Convert {
            path,
            to,
            crf,
            width,
            fps,
            mute,
        } => convert(&path, to, crf, width, fps, mute),
        Command::Thumbnail { path, at, width } => thumbnail(&path, at, width),
        Command::Ocr { path, models } => ocr(&path, &models, json),
        Command::Name { pattern, window } => name(&pattern, window),
        Command::Sxcu { path } => sxcu(&path, json),
        Command::Sxie { path } => sxie(&path, json),
    }
}

fn open_image(path: &std::path::Path) -> Result<image::RgbaImage> {
    Ok(image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgba8())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<ExitCode> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(ExitCode::SUCCESS)
}

// ── Commands ────────────────────────────────────────────────────────────

fn hash(path: &std::path::Path, expect: Option<&str>, json: bool) -> Result<ExitCode> {
    let digests = kestrel_tools::hash_file_all(path).map_err(|e| e.to_string())?;

    if let Some(expected) = expect {
        // Comparison is paste-tolerant on purpose: a published checksum is
        // often copied with surrounding whitespace or in the other case.
        let matched = digests
            .iter()
            .any(|(_, digest)| digest.eq_ignore_ascii_case(expected.trim()));

        if json {
            print_json(&serde_json::json!({ "matched": matched, "expected": expected }))?;
        } else if matched {
            println!("ok");
        } else {
            println!("no match");
        }
        // Non-zero for "no", so this can gate a shell `&&`.
        return Ok(if matched {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    if json {
        let map: std::collections::BTreeMap<String, &String> = digests
            .iter()
            .map(|(algorithm, digest)| (format!("{algorithm:?}").to_lowercase(), digest))
            .collect();
        return print_json(&map);
    }

    for (algorithm, digest) in &digests {
        println!("{algorithm:?}  {digest}");
    }
    Ok(ExitCode::SUCCESS)
}

fn metadata(path: &std::path::Path, strip: bool, json: bool) -> Result<ExitCode> {
    if strip {
        // Written beside the original rather than over it. Stripping metadata
        // is not reversible, and doing it in place would destroy the only copy.
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = path.extension().unwrap_or_default().to_string_lossy();
        let destination = path.with_file_name(format!("{stem}-clean.{extension}"));

        kestrel_tools::strip_metadata(path, &destination).map_err(|e| e.to_string())?;
        println!("{}", destination.display());
        return Ok(ExitCode::SUCCESS);
    }

    let fields = kestrel_tools::read_metadata(path).map_err(|e| e.to_string())?;
    if json {
        return print_json(&fields);
    }

    if fields.is_empty() {
        println!("no metadata");
    }
    for field in &fields {
        // The marker is the point of the tool: which fields identify a person,
        // a place or a device.
        let marker = if field.sensitive { "!" } else { " " };
        println!("{marker} {:<28} {}", field.tag, field.value);
    }
    Ok(ExitCode::SUCCESS)
}

fn qr(path: &std::path::Path, json: bool) -> Result<ExitCode> {
    let found = kestrel_tools::decode(&open_image(path)?);

    if json {
        return print_json(&found);
    }
    if found.is_empty() {
        // Not an error: an image with no QR code is a perfectly good answer.
        println!("no codes found");
    }
    for code in &found {
        println!("{}", code.text);
    }
    Ok(ExitCode::SUCCESS)
}

fn analyze(path: &std::path::Path, json: bool) -> Result<ExitCode> {
    let analysis = kestrel_tools::analyze(&open_image(path)?);

    if json {
        return print_json(&analysis);
    }
    println!("size          {}x{}", analysis.width, analysis.height);
    println!(
        "colours       {}{}",
        analysis.unique_colours,
        if analysis.unique_colours_capped {
            "+"
        } else {
            ""
        }
    );
    println!("transparency  {}", analysis.has_transparency);
    println!("dominant      {}", analysis.dominant.join(", "));
    Ok(ExitCode::SUCCESS)
}

fn compare(
    first: &std::path::Path,
    second: &std::path::Path,
    tolerance: u8,
    diff: Option<&std::path::Path>,
    json: bool,
) -> Result<ExitCode> {
    let a = open_image(first)?;
    let b = open_image(second)?;
    let result = kestrel_tools::compare(&a, &b, tolerance);

    if let Some(destination) = diff {
        kestrel_tools::diff_image(&a, &b, tolerance)
            .save(destination)
            .map_err(|e| format!("{}: {e}", destination.display()))?;
    }

    if json {
        print_json(&result)?;
    } else if result.identical() {
        println!("identical");
    } else {
        println!(
            "{:.2}% differ ({} of {} pixels, largest channel delta {})",
            result.difference_percent,
            result.changed_pixels,
            result.total_pixels,
            result.max_channel_delta
        );
        if result.sizes_differ {
            println!(
                "sizes differ; compared the {}x{} overlap",
                result.compared_width, result.compared_height
            );
        }
    }

    // Non-zero when they differ, so this can gate a shell `&&`.
    Ok(if result.identical() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn color(path: &std::path::Path, x: u32, y: u32, radius: u32, json: bool) -> Result<ExitCode> {
    let image = open_image(path)?;
    let swatch = match radius {
        0 => kestrel_tools::pick_color(&image, x, y),
        radius => kestrel_tools::pick_average(&image, x, y, radius),
    }
    .ok_or_else(|| {
        format!(
            "{x},{y} is outside the image, which is {}x{}",
            image.width(),
            image.height()
        )
    })?;

    if json {
        return print_json(&swatch);
    }
    println!("hex   {}", swatch.hex);
    println!("rgb   {}, {}, {}", swatch.rgb.r, swatch.rgb.g, swatch.rgb.b);
    println!(
        "hsl   {:.0}°, {:.0}%, {:.0}%",
        swatch.hsl.0, swatch.hsl.1, swatch.hsl.2
    );
    println!(
        "hsv   {:.0}°, {:.0}%, {:.0}%",
        swatch.hsv.0, swatch.hsv.1, swatch.hsv.2
    );
    println!(
        "cmyk  {:.0}%, {:.0}%, {:.0}%, {:.0}%",
        swatch.cmyk.0, swatch.cmyk.1, swatch.cmyk.2, swatch.cmyk.3
    );
    Ok(ExitCode::SUCCESS)
}

fn index(
    path: &std::path::Path,
    format: IndexFormat,
    out: Option<&std::path::Path>,
) -> Result<ExitCode> {
    let options = kestrel_tools::IndexOptions {
        format: match format {
            IndexFormat::Html => kestrel_tools::indexer::Format::Html,
            IndexFormat::Text => kestrel_tools::indexer::Format::Text,
            IndexFormat::Json => kestrel_tools::indexer::Format::Json,
            IndexFormat::Xml => kestrel_tools::indexer::Format::Xml,
        },
        ..Default::default()
    };

    let entry = kestrel_tools::index(path, &options).map_err(|e| e.to_string())?;
    let rendered = kestrel_tools::indexer::render(&entry, &options).map_err(|e| e.to_string())?;

    match out {
        Some(destination) => {
            std::fs::write(destination, rendered)
                .map_err(|e| format!("{}: {e}", destination.display()))?;
            println!("{}", destination.display());
        }
        None => println!("{rendered}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn ffmpeg() -> Result<std::path::PathBuf> {
    kestrel_record::ffmpeg::find()
        .ok_or_else(|| kestrel_record::ffmpeg::FfmpegError::NotFound.to_string())
}

fn convert(
    path: &std::path::Path,
    to: ConvertTarget,
    crf: u8,
    width: Option<u32>,
    fps: Option<u32>,
    mute: bool,
) -> Result<ExitCode> {
    let settings = kestrel_record::ConvertSettings {
        target: match to {
            ConvertTarget::Mp4 => kestrel_record::Target::Mp4,
            ConvertTarget::Webm => kestrel_record::Target::Webm,
            ConvertTarget::Mkv => kestrel_record::Target::Mkv,
            ConvertTarget::Gif => kestrel_record::Target::Gif,
            ConvertTarget::Mp3 => kestrel_record::Target::Mp3,
        },
        crf,
        fps,
        width,
        mute,
    };

    let output = kestrel_record::convert(&ffmpeg()?, path, &settings).map_err(|e| e.to_string())?;
    println!("{}", output.display());
    Ok(ExitCode::SUCCESS)
}

fn thumbnail(path: &std::path::Path, at: f32, width: u32) -> Result<ExitCode> {
    let output =
        kestrel_record::thumbnail(&ffmpeg()?, path, at, width).map_err(|e| e.to_string())?;
    println!("{}", output.display());
    Ok(ExitCode::SUCCESS)
}

fn ocr(path: &std::path::Path, models: &std::path::Path, json: bool) -> Result<ExitCode> {
    let models = kestrel_tools::ocr::Models::in_directory(models);
    let engine = kestrel_tools::ocr::Engine::load(&models).map_err(|e| e.to_string())?;
    let recognised = engine.read(&open_image(path)?).map_err(|e| e.to_string())?;

    if json {
        return print_json(&recognised);
    }
    print!("{}", recognised.text);
    if !recognised.text.is_empty() {
        println!();
    }
    Ok(ExitCode::SUCCESS)
}

fn name(pattern: &str, window: Option<String>) -> Result<ExitCode> {
    let context = kestrel_core::name_pattern::NameContext {
        window_title: window,
        ..Default::default()
    };

    // Sanitised, because the point of checking a pattern is to see the filename
    // it will actually produce.
    println!(
        "{}",
        kestrel_core::name_pattern::expand_sanitized(pattern, &context)
    );
    Ok(ExitCode::SUCCESS)
}

fn sxcu(path: &std::path::Path, json: bool) -> Result<ExitCode> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let uploader = kestrel_upload::sxcu::CustomUploader::parse(&text).map_err(|e| e.to_string())?;

    if json {
        return print_json(&uploader);
    }
    println!("name    {}", uploader.display_name());
    println!("method  {:?}", uploader.request_method);
    println!("url     {}", uploader.request_url);
    Ok(ExitCode::SUCCESS)
}

fn sxie(path: &std::path::Path, json: bool) -> Result<ExitCode> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let imported = kestrel_editor::import_sxie(&text).map_err(|e| e.to_string())?;

    if json {
        return print_json(&serde_json::json!({
            "name": imported.name,
            "effects": imported.chain,
            "unsupported": imported.unsupported,
        }));
    }

    if let Some(name) = &imported.name {
        println!("name    {name}");
    }
    for effect in &imported.chain.0 {
        println!("        {effect:?}");
    }
    if !imported.is_complete() {
        // To stderr, so a piped effect list is not polluted by the warning —
        // but said out loud, because a partial import is not a success.
        eprintln!(
            "kestrel: {} effect(s) could not be imported: {}",
            imported.unsupported.len(),
            imported.unsupported.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}
