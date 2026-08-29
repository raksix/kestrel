//! The listener that lets `kestrel <command>` drive a running app.
//!
//! The wire format and the reasoning behind the token live in
//! `kestrel_core::rpc`. This is the app half: bind a port, publish where it is,
//! and turn each request into the same call the UI would make.
//!
//! Two things this is careful about.
//!
//! The endpoint file is written with owner-only permissions, because it holds
//! the token and the token is what stops any local process from taking a
//! screenshot. On Unix that is enforced; on Windows the file inherits the
//! user's profile ACL, which is the same guarantee in practice but is worth
//! knowing is not set explicitly here.
//!
//! Captures are dispatched onto the app's own background path rather than run
//! on the connection thread. A region capture waits for the user to drag a
//! selection, and holding a socket open for that would make the command look
//! hung — so the reply says the capture *started*, which is the truth.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;

use kestrel_core::rpc::{self, Endpoint, Envelope, Request, Response};
use tauri::{AppHandle, Manager};

/// How long a connection may take to send its request.
///
/// Without this, a process that connects and says nothing holds a thread
/// forever.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The largest request accepted, so a malicious or broken client cannot make
/// the app allocate without bound.
const MAX_REQUEST: u64 = 64 * 1024;

pub fn endpoint_path() -> Option<PathBuf> {
    crate::settings::config_dir()
        .ok()
        .map(|dir| dir.join(rpc::ENDPOINT_FILE))
}

/// Start listening, and publish the endpoint.
///
/// Failure is logged rather than propagated: the app is perfectly usable
/// without the command line, and refusing to start over it would be a worse
/// trade than losing the feature.
pub fn serve(app: &AppHandle) {
    match bind(app) {
        Ok(port) => tracing::info!(port, "command-line interface listening"),
        Err(err) => tracing::warn!(%err, "could not start the command listener"),
    }
}

fn bind(app: &AppHandle) -> std::io::Result<u16> {
    // Loopback only. Binding to any interface would put a screenshot trigger on
    // the network, which is not a thing this app should ever do.
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    let token = generate_token();

    publish(&Endpoint {
        port,
        token: token.clone(),
        pid: std::process::id(),
    })?;

    let app = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let app = app.clone();
                    let token = token.clone();
                    // One thread per connection: a request can take as long as
                    // an upload, and serialising them would make a second
                    // command block behind the first.
                    std::thread::spawn(move || handle(&app, stream, &token));
                }
                Err(err) => tracing::warn!(%err, "command connection failed"),
            }
        }
    });

    Ok(port)
}

/// A random token, from the OS.
///
/// Derived from a `getrandom`-backed source rather than a timestamp: a token
/// anyone can guess from the clock is not a token.
fn generate_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("the OS random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Write the endpoint where the CLI will look for it, readable only by us.
fn publish(endpoint: &Endpoint) -> std::io::Result<()> {
    let path = endpoint_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&path, serde_json::to_vec(endpoint)?)?;

    // The token in this file is the only thing between a local process and a
    // screenshot, so it must not be world-readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Remove the endpoint file on shutdown, so the CLI reports "not running"
/// rather than failing to connect to a port nobody is on.
///
/// Only if it is still *ours*. Quitting one instance while another is starting
/// is an ordinary thing to do — during development it happens constantly — and
/// an unconditional delete here removes the file the new instance just wrote,
/// leaving a running app that every command reports as absent. That is what the
/// pid in the endpoint is for.
pub fn withdraw() {
    let Some(path) = endpoint_path() else {
        return;
    };

    let owned_by_us = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Endpoint>(&text).ok())
        .map(|endpoint| endpoint.pid == std::process::id())
        // A file we cannot read is not one we can claim, so leave it: a stale
        // file produces a clear message, a deleted live one does not.
        .unwrap_or(false);

    if owned_by_us {
        let _ = std::fs::remove_file(path);
    }
}

fn handle(app: &AppHandle, stream: TcpStream, token: &str) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(err) => {
            tracing::warn!(%err, "could not reply to a command");
            return;
        }
    };

    let mut line = String::new();
    let read = BufReader::new(Read::take(stream, MAX_REQUEST)).read_line(&mut line);

    let response = match read {
        Err(err) => Response::error(err),
        Ok(0) => Response::error("empty request"),
        Ok(_) => match serde_json::from_str::<Envelope>(&line) {
            Err(err) => Response::error(format!("malformed request: {err}")),
            Ok(envelope) if !rpc::token_matches(token, &envelope.token) => {
                // Deliberately vague, and logged rather than explained: telling
                // a caller *why* its token was wrong helps only an attacker.
                tracing::warn!("rejected a command with a bad token");
                Response::error("not authorised")
            }
            Ok(envelope) => dispatch(app, envelope.request),
        },
    };

    let mut body = serde_json::to_vec(&response).unwrap_or_else(|_| b"{}".to_vec());
    body.push(b'\n');
    let _ = writer.write_all(&body);
}

fn dispatch(app: &AppHandle, request: Request) -> Response {
    match request {
        Request::Ping => Response::ok("kestrel is running"),

        Request::Show => {
            crate::show_main_window(app);
            Response::ok("shown")
        }

        Request::Capture { method } => {
            // Dispatched onto the background path, not run here. A region
            // capture waits for a drag, and holding the socket open for that
            // would make the command look hung.
            crate::run_in_background(app, method);
            Response::ok(format!("{method:?} capture started"))
        }

        Request::RunWorkflow { workflow } => match find_workflow(app, &workflow) {
            Some(found) => {
                crate::run_in_background(app, found.method);
                Response::ok(format!("running {}", found.name))
            }
            None => Response::error(format!("no workflow called {workflow:?}")),
        },

        Request::Edit { path } => match image::open(&path) {
            Ok(image) => match crate::editor::open(app, image.to_rgba8()) {
                Ok(_) => Response::with_path("opened in the editor", Some(path)),
                Err(err) => Response::error(err),
            },
            Err(err) => Response::error(format!("{path}: {err}")),
        },

        Request::Pin { path } => match image::open(&path) {
            Ok(image) => match crate::pin::pin(app, image.to_rgba8()) {
                Ok(_) => Response::with_path("pinned", Some(path)),
                Err(err) => Response::error(err),
            },
            Err(err) => Response::error(format!("{path}: {err}")),
        },

        Request::Import { path } => import(app, &path),

        Request::Upload { path, destination } => upload(app, path, destination),
    }
}

/// Match a workflow by id first, then by name.
///
/// Ids are stable and names are what a person remembers, so both work; ids win
/// because they are the unambiguous form.
fn find_workflow(app: &AppHandle, wanted: &str) -> Option<kestrel_core::model::Workflow> {
    let workflows = app
        .state::<crate::settings::SettingsState>()
        .snapshot()
        .workflows;

    workflows
        .iter()
        .find(|workflow| workflow.id == wanted)
        .or_else(|| {
            workflows
                .iter()
                .find(|workflow| workflow.name.eq_ignore_ascii_case(wanted))
        })
        .cloned()
}

/// Import a `.sxcu` uploader or a `.sxie` effect preset, whichever it is.
fn import(app: &AppHandle, path: &str) -> Response {
    let _ = app;
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("sxcu") => match crate::uploads::import(std::path::Path::new(path)) {
            Ok(destination) => Response::ok(format!("imported {}", destination.name)),
            Err(err) => Response::error(err),
        },
        Some("sxie") => match std::fs::read_to_string(path) {
            Ok(text) => match kestrel_editor::import_sxie(&text) {
                Ok(imported) if imported.is_complete() => {
                    Response::ok(format!("{} effect(s) read", imported.chain.len()))
                }
                Ok(imported) => Response::error(format!(
                    "{} effect(s) read, but these have no equivalent: {}",
                    imported.chain.len(),
                    imported.unsupported.join(", ")
                )),
                Err(err) => Response::error(err),
            },
            Err(err) => Response::error(format!("{path}: {err}")),
        },
        _ => Response::error(format!("{path} is not a .sxcu or .sxie file")),
    }
}

/// Upload a file and wait for the result.
///
/// Unlike a capture, this one blocks: the useful output is the URL, and a
/// command that returned before producing it would be pointless in a script.
fn upload(app: &AppHandle, path: String, destination: Option<String>) -> Response {
    let app = app.clone();
    let handle = std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            crate::uploads::upload_path_to(
                &app,
                std::path::Path::new(&path),
                destination,
                &crate::commands::UNATTENDED,
            )
            .await
        })
    });

    match handle.join() {
        Ok(Ok(uploaded)) => Response::Ok {
            message: format!("uploaded to {}", uploaded.destination),
            path: None,
            url: Some(uploaded.url),
        },
        Ok(Err(err)) => Response::error(err),
        Err(_) => Response::error("the upload thread panicked"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_long_and_different_every_time() {
        // A predictable token is not a token. Length is checked too, because a
        // truncated one would still look plausible in the file.
        let first = generate_token();
        let second = generate_token();

        assert_eq!(first.len(), 48, "24 bytes as hex");
        assert_ne!(first, second);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn a_request_longer_than_the_limit_is_cut_off_rather_than_buffered() {
        // The point of the cap is to bound what a broken or hostile client can
        // make the app allocate, so check the truncation rather than the number.
        let flood = "a".repeat(MAX_REQUEST as usize * 2);
        let mut read = String::new();
        Read::take(flood.as_bytes(), MAX_REQUEST)
            .read_to_string(&mut read)
            .unwrap();

        assert_eq!(read.len(), MAX_REQUEST as usize);
    }
}
