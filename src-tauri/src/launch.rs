//! What to do with a file or URL the operating system hands us.
//!
//! Three ways one arrives: on the command line, as a `kestrel://` URL, or as a
//! file the user double-clicked in a file manager. All three end up as the same
//! thing — a path or a URL to act on — so they are parsed into one type here
//! and handled in one place.
//!
//! # Handing over to a running instance
//!
//! Double-clicking a second `.sxcu` should import it into the app that is
//! already open, not start a second copy. Rather than adding a single-instance
//! plugin, this reuses the channel the `kestrel` command already talks over: if
//! a live instance answers, the request goes to it and this process exits.
//!
//! That means the check has to be cheap and it has to fail *fast* when nothing
//! is listening — a file association that hangs for two seconds before opening
//! is worse than one that occasionally starts a second window.

use std::path::{Path, PathBuf};

use kestrel_core::rpc::{Endpoint, Envelope, Request, Response};

/// Something the OS asked us to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// A `.sxcu` uploader or a `.sxie` effect preset.
    Import(PathBuf),
    /// An image, to annotate.
    Edit(PathBuf),
    /// Anything else with a path: upload it.
    Upload(PathBuf),
    /// Bring the window forward, which is what a bare launch means.
    Show,
}

/// Extensions that mean "import this", not "upload this".
const IMPORTABLE: [&str; 2] = ["sxcu", "sxie"];

/// Extensions we can annotate.
const EDITABLE: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

impl Intent {
    fn for_path(path: PathBuf) -> Self {
        match extension(&path).as_deref() {
            Some(ext) if IMPORTABLE.contains(&ext) => Intent::Import(path),
            Some(ext) if EDITABLE.contains(&ext) => Intent::Edit(path),
            _ => Intent::Upload(path),
        }
    }

    pub fn to_request(&self) -> Request {
        match self {
            Intent::Import(path) => Request::Import {
                path: path.to_string_lossy().into_owned(),
            },
            Intent::Edit(path) => Request::Edit {
                path: path.to_string_lossy().into_owned(),
            },
            Intent::Upload(path) => Request::Upload {
                path: path.to_string_lossy().into_owned(),
                destination: None,
            },
            Intent::Show => Request::Show,
        }
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// Read an intent out of one argument.
///
/// Returns `None` for anything that is not a path or a `kestrel://` URL —
/// flags, the program name, and the `-psn_…` argument macOS passes to bundles
/// launched from Finder.
pub fn parse_argument(argument: &str) -> Option<Intent> {
    if argument.starts_with('-') {
        return None;
    }

    if let Some(rest) = argument.strip_prefix("kestrel://") {
        return parse_url(rest);
    }

    // A bare path. Existence is checked because a mistyped argument should not
    // become an upload attempt that fails later with a confusing message.
    let path = PathBuf::from(argument);
    path.is_file().then(|| Intent::for_path(path))
}

/// `kestrel://import/<path>`, `kestrel://edit/<path>`, `kestrel://upload/<path>`
/// or `kestrel://show`.
///
/// Deliberately small. A URL scheme is an entry point anything on the machine
/// can invoke — a web page can navigate to one — so every verb here has to be
/// something that is safe to trigger without asking. Importing and editing show
/// the user what happened and change nothing outside Kestrel.
fn parse_url(rest: &str) -> Option<Intent> {
    let (verb, argument) = match rest.split_once('/') {
        Some((verb, argument)) => (verb, argument),
        None => (rest.trim_end_matches('/'), ""),
    };

    let path = || -> Option<PathBuf> {
        let decoded = percent_decode(argument);
        let path = PathBuf::from(decoded);
        path.is_file().then_some(path)
    };

    match verb {
        "show" | "" => Some(Intent::Show),
        "import" => path().map(Intent::Import),
        "edit" => path().map(Intent::Edit),
        // Upload is *not* offered over the URL scheme. Everything else here is
        // local and visible; this one would send a file off the machine, and a
        // link that does that without being asked is not something to ship.
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(byte) = hex_pair(bytes[i + 1], bytes[i + 2]) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    Some(digit(high)? << 4 | digit(low)?)
}

/// The first intent in a set of arguments, if any.
pub fn intent_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<Intent> {
    // `skip(1)` drops the program name. Only the first usable argument is
    // taken: opening five files at once would mean five editor windows, and
    // "the OS gave us a list" is not a reason to do that.
    args.into_iter()
        .skip(1)
        .find_map(|argument| parse_argument(&argument))
}

/// Try to hand `intent` to an instance that is already running.
///
/// `true` means it was delivered and this process has nothing left to do.
pub fn forward(intent: &Intent) -> bool {
    let Some(endpoint) = read_endpoint() else {
        return false;
    };

    match send(&endpoint, intent) {
        Some(response) if response.is_ok() => true,
        Some(Response::Error { message }) => {
            // The running instance refused it. Starting a second copy to try
            // the same thing again would just fail the same way.
            eprintln!("kestrel: {message}");
            true
        }
        _ => false,
    }
}

fn read_endpoint() -> Option<Endpoint> {
    let path = crate::rpc::endpoint_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn send(endpoint: &Endpoint, intent: &Intent) -> Option<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    // Short timeouts throughout. This runs before any window appears, so every
    // millisecond spent here is a millisecond of nothing happening after a
    // double-click.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;

    let envelope = Envelope {
        token: endpoint.token.clone(),
        request: intent.to_request(),
    };
    let mut body = serde_json::to_vec(&envelope).ok()?;
    body.push(b'\n');

    let mut writer = stream.try_clone().ok()?;
    writer.write_all(&body).ok()?;
    writer.flush().ok()?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kestrel-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"x").unwrap();
        path
    }

    #[test]
    fn an_uploader_file_is_imported_not_uploaded() {
        // Uploading a .sxcu to a host would be a comic misreading of a
        // double-click.
        let path = temp_file("service.sxcu");
        assert_eq!(
            parse_argument(path.to_str().unwrap()),
            Some(Intent::Import(path))
        );
    }

    #[test]
    fn an_effect_preset_is_imported() {
        let path = temp_file("preset.sxie");
        assert_eq!(
            parse_argument(path.to_str().unwrap()),
            Some(Intent::Import(path))
        );
    }

    #[test]
    fn an_image_opens_in_the_editor() {
        let path = temp_file("shot.png");
        assert_eq!(
            parse_argument(path.to_str().unwrap()),
            Some(Intent::Edit(path))
        );
    }

    #[test]
    fn an_extension_is_matched_whatever_its_case() {
        let path = temp_file("SHOT.PNG");
        assert!(matches!(
            parse_argument(path.to_str().unwrap()),
            Some(Intent::Edit(_))
        ));
    }

    #[test]
    fn anything_else_with_a_path_is_offered_for_upload() {
        let path = temp_file("notes.txt");
        assert_eq!(
            parse_argument(path.to_str().unwrap()),
            Some(Intent::Upload(path))
        );
    }

    #[test]
    fn a_path_that_does_not_exist_is_not_an_intent() {
        // A typo should not become an upload that fails later with a message
        // about the network.
        assert_eq!(parse_argument("/definitely/not/here.png"), None);
    }

    #[test]
    fn flags_are_ignored() {
        // macOS passes -psn_0_12345 to bundles launched from Finder.
        assert_eq!(parse_argument("-psn_0_123456"), None);
        assert_eq!(parse_argument("--verbose"), None);
    }

    #[test]
    fn the_program_name_is_skipped() {
        let path = temp_file("arg.png");
        let args = vec![
            "/Applications/Kestrel.app/Contents/MacOS/kestrel-app".to_string(),
            path.to_string_lossy().into_owned(),
        ];

        assert!(matches!(intent_from_args(args), Some(Intent::Edit(_))));
    }

    #[test]
    fn only_the_first_usable_argument_is_taken() {
        // Selecting five files and pressing return should not open five
        // editor windows.
        let first = temp_file("one.png");
        let second = temp_file("two.png");
        let args = vec![
            "kestrel-app".to_string(),
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];

        assert_eq!(intent_from_args(args), Some(Intent::Edit(first)));
    }

    #[test]
    fn a_bare_launch_has_no_intent() {
        assert_eq!(intent_from_args(vec!["kestrel-app".to_string()]), None);
    }

    #[test]
    fn the_url_scheme_can_show_the_window() {
        assert_eq!(parse_argument("kestrel://show"), Some(Intent::Show));
        assert_eq!(parse_argument("kestrel://show/"), Some(Intent::Show));
        assert_eq!(parse_argument("kestrel://"), Some(Intent::Show));
    }

    #[test]
    fn the_url_scheme_can_import_and_edit() {
        let uploader = temp_file("via-url.sxcu");
        let image = temp_file("via-url.png");

        assert_eq!(
            parse_argument(&format!("kestrel://import/{}", uploader.display())),
            Some(Intent::Import(uploader))
        );
        assert_eq!(
            parse_argument(&format!("kestrel://edit/{}", image.display())),
            Some(Intent::Edit(image))
        );
    }

    #[test]
    fn the_url_scheme_will_not_upload() {
        // A URL scheme is an entry point any web page can navigate to. Every
        // other verb is local and visible; this one would send a file off the
        // machine without being asked, so it is not offered.
        let path = temp_file("secret.png");
        assert_eq!(
            parse_argument(&format!("kestrel://upload/{}", path.display())),
            None
        );
    }

    #[test]
    fn an_unknown_url_verb_is_refused_rather_than_guessed() {
        assert_eq!(parse_argument("kestrel://delete/everything"), None);
        assert_eq!(parse_argument("kestrel://capture/region"), None);
    }

    #[test]
    fn a_url_path_that_does_not_exist_is_refused() {
        assert_eq!(parse_argument("kestrel://edit//not/a/file.png"), None);
    }

    #[test]
    fn percent_escapes_in_a_url_are_decoded() {
        let dir = std::env::temp_dir().join(format!("kestrel-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("with space.png");
        std::fs::write(&path, b"x").unwrap();

        let encoded = path.to_string_lossy().replace(' ', "%20");
        assert_eq!(
            parse_argument(&format!("kestrel://edit/{encoded}")),
            Some(Intent::Edit(path))
        );
    }

    #[test]
    fn a_malformed_escape_is_left_alone_rather_than_dropped() {
        // "%zz" is not an escape; eating it would silently change the path.
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("trailing%"), "trailing%");
    }

    #[test]
    fn every_intent_maps_to_a_request() {
        assert!(matches!(
            Intent::Import("/a.sxcu".into()).to_request(),
            Request::Import { .. }
        ));
        assert!(matches!(
            Intent::Edit("/a.png".into()).to_request(),
            Request::Edit { .. }
        ));
        assert!(matches!(
            Intent::Upload("/a.bin".into()).to_request(),
            Request::Upload { .. }
        ));
        assert_eq!(Intent::Show.to_request(), Request::Show);
    }
}
