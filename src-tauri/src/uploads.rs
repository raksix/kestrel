//! Destination storage and the upload path.
//!
//! Custom uploaders are stored as the `.sxcu` files they came from, one per
//! file, rather than being folded into `settings.json`. That keeps them
//! shareable: a user can drop a file in, copy one out, or keep the folder in a
//! dotfiles repo, exactly as they would with ShareX.

use std::path::{Path, PathBuf};

use kestrel_upload::client::{self, Payload};
use kestrel_upload::sxcu::CustomUploader;
use kestrel_upload::syntax::{Context, Prompter};
use serde::{Deserialize, Serialize};

/// A destination as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    /// Stable id, derived from the file name.
    pub id: String,
    pub name: String,
    pub host: String,
    pub accepts_image: bool,
    pub accepts_text: bool,
    pub accepts_file: bool,
    pub shortens_urls: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("no destination is configured — add a custom uploader first")]
    NoDestination,
    #[error("no destination with id {0}")]
    UnknownDestination(String),
    #[error(transparent)]
    Sxcu(#[from] kestrel_upload::sxcu::SxcuError),
    #[error(transparent)]
    Transport(#[from] client::ClientError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The service answered, and what it said was a refusal.
    #[error("{0}")]
    Service(String),
    #[error("the upload succeeded but no URL could be parsed from the response")]
    NoUrl,
}

pub type Result<T> = std::result::Result<T, UploadError>;

/// Where `.sxcu` files live.
pub fn uploaders_dir() -> Result<PathBuf> {
    let dir = crate::settings::config_dir()
        .map_err(|e| UploadError::Io(std::io::Error::other(e.to_string())))?
        .join("uploaders");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn id_for(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Load every stored uploader, skipping any file that will not parse.
///
/// One broken file must not hide the rest: a user who hand-edits a `.sxcu` and
/// makes a typo should lose that uploader, not the whole list.
pub fn load_all() -> Result<Vec<(String, CustomUploader)>> {
    let dir = uploaders_dir()?;
    let mut found = Vec::new();

    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("sxcu")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match CustomUploader::parse(&text) {
            Ok(uploader) => found.push((id_for(&path), uploader)),
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "skipping unreadable uploader")
            }
        }
    }

    found.sort_by(|a, b| a.1.display_name().cmp(&b.1.display_name()));
    Ok(found)
}

pub fn list() -> Result<Vec<Destination>> {
    Ok(load_all()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, uploader)| Destination {
            id,
            name: uploader.display_name(),
            host: uploader.request_url.clone(),
            accepts_image: uploader.destination_type.image,
            accepts_text: uploader.destination_type.text,
            accepts_file: uploader.destination_type.file,
            shortens_urls: uploader.destination_type.url_shortener,
        })
        .collect())
}

/// Import a `.sxcu` file, returning the destination it became.
///
/// The file is validated before being stored, so an unusable uploader never
/// reaches the destination list.
pub fn import(source: &Path) -> Result<Destination> {
    let text = std::fs::read_to_string(source)?;
    let uploader = CustomUploader::parse(&text)?;

    let stem = sanitize_id(&uploader.display_name());
    let target = uploaders_dir()?.join(format!("{stem}.sxcu"));
    std::fs::write(&target, &text)?;
    tracing::info!(path = %target.display(), "imported custom uploader");

    Ok(Destination {
        id: stem,
        name: uploader.display_name(),
        host: uploader.request_url.clone(),
        accepts_image: uploader.destination_type.image,
        accepts_text: uploader.destination_type.text,
        accepts_file: uploader.destination_type.file,
        shortens_urls: uploader.destination_type.url_shortener,
    })
}

pub fn remove(id: &str) -> Result<()> {
    let path = uploaders_dir()?.join(format!("{}.sxcu", sanitize_id(id)));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Keep a display name usable as a file name on every platform.
fn sanitize_id(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "uploader".to_string()
    } else {
        trimmed
    }
}

fn find(id: &str) -> Result<CustomUploader> {
    load_all()?
        .into_iter()
        .find(|(candidate, _)| candidate == id)
        .map(|(_, uploader)| uploader)
        .ok_or_else(|| UploadError::UnknownDestination(id.to_string()))
}

/// The outcome of an upload, as the UI sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uploaded {
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub deletion_url: Option<String>,
    pub destination: String,
}

/// Upload a file from disk to the default destination.
///
/// Used by the watch folder, where there is no capture in memory — only a path
/// that appeared. The MIME type is guessed from the extension because that is
/// all the information there is; an uploader that cares will reject a wrong
/// guess, which is a better outcome than refusing to send anything.
pub async fn upload_path(
    app: &tauri::AppHandle,
    path: &Path,
    prompter: &dyn Prompter,
) -> Result<Uploaded> {
    use tauri::Manager;

    let bytes = std::fs::read(path)?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "upload".to_string());

    let configured = app
        .state::<crate::settings::SettingsState>()
        .snapshot()
        .default_destination
        .clone();
    let destination = resolve_destination(configured)?;

    let payload = if is_text(path) {
        Payload::Text(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        Payload::File {
            bytes,
            mime: mime_for(path).to_string(),
            filename,
        }
    };

    upload(&destination, payload, prompter).await
}

fn is_text(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("txt") | Some("md"))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// A MIME type for the extensions the watch folder accepts.
fn mime_for(path: &Path) -> &'static str {
    match extension(path).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("txt") => "text/plain",
        // Not a lie so much as an admission: this is bytes of unknown kind.
        _ => "application/octet-stream",
    }
}

/// Upload a payload to one destination.
pub async fn upload(
    destination_id: &str,
    payload: Payload,
    prompter: &dyn Prompter,
) -> Result<Uploaded> {
    let uploader = find(destination_id)?;

    let request_ctx = Context {
        input: payload.as_input().to_string(),
        filename: payload.filename().to_string(),
        ..Default::default()
    };
    let request = uploader.prepare(&request_ctx, prompter)?;

    tracing::info!(url = %request.url, method = request.method.as_str(), "uploading");
    let response = client::execute(&client::client(), &request, &payload).await?;
    let status = response.status;

    let result = uploader.parse_response(&response.into_context(&payload), prompter)?;

    // A parsed error message is the service explaining itself and always beats
    // a bare status code.
    if let Some(message) = result.error {
        return Err(UploadError::Service(message));
    }
    if result.url.trim().is_empty() {
        // Without an error template there is nothing better to report than the
        // status, so say that rather than handing back an empty URL.
        return Err(if (200..300).contains(&status) {
            UploadError::NoUrl
        } else {
            UploadError::Service(format!("sunucu {status} döndürdü"))
        });
    }

    Ok(Uploaded {
        url: result.url,
        thumbnail_url: result.thumbnail_url,
        deletion_url: result.deletion_url,
        destination: uploader.display_name(),
    })
}

/// Resolve which destination an upload should go to.
///
/// Falling back to a sole destination means someone who imported exactly one
/// uploader never has to also select it.
pub fn resolve_destination(configured: Option<String>) -> Result<String> {
    if let Some(id) = configured.filter(|id| !id.is_empty()) {
        // A destination that has since been deleted should not silently
        // redirect the upload somewhere else.
        if list()?.iter().any(|d| d.id == id) {
            return Ok(id);
        }
        return Err(UploadError::UnknownDestination(id));
    }

    match list()?.as_slice() {
        [only] => Ok(only.id.clone()),
        _ => Err(UploadError::NoDestination),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_safe_as_file_names() {
        assert_eq!(sanitize_id("My Host"), "My-Host");
        assert_eq!(sanitize_id("a/b\\c:d"), "a-b-c-d");
        assert_eq!(sanitize_id("örnek.com"), "örnek.com");
    }

    #[test]
    fn an_empty_or_symbolic_name_still_yields_an_id() {
        // Otherwise the file would be written as ".sxcu", which is hidden on
        // Unix and rejected on Windows.
        assert_eq!(sanitize_id(""), "uploader");
        assert_eq!(sanitize_id("///"), "uploader");
    }

    #[test]
    fn ids_come_from_the_file_stem() {
        assert_eq!(id_for(Path::new("/tmp/example.com.sxcu")), "example.com");
    }

    #[test]
    fn no_destinations_configured_is_a_named_error() {
        // With none stored there is nothing to fall back to, and the error has
        // to say so rather than surfacing as an empty URL later on.
        if list().map(|l| l.is_empty()).unwrap_or(true) {
            assert!(matches!(
                resolve_destination(None),
                Err(UploadError::NoDestination)
            ));
        }
    }

    #[test]
    fn a_destination_that_no_longer_exists_is_rejected() {
        // Silently redirecting to some other uploader would send the user's
        // screenshot somewhere they did not choose.
        assert!(matches!(
            resolve_destination(Some("deleted-uploader".into())),
            Err(UploadError::UnknownDestination(_))
        ));
    }

    #[test]
    fn an_empty_configured_id_is_treated_as_unset() {
        if list().map(|l| l.is_empty()).unwrap_or(true) {
            assert!(matches!(
                resolve_destination(Some(String::new())),
                Err(UploadError::NoDestination)
            ));
        }
    }
}
