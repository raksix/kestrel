//! Talking to a running Kestrel.
//!
//! The protocol and the reasoning behind the token are in `kestrel_core::rpc`.
//! This is the client half: find the endpoint file, send one request, print
//! what comes back.
//!
//! The failure everyone hits first is "the app is not running", so it gets a
//! real message rather than a connection error. The second is a stale endpoint
//! file left by a crash, which looks identical to a live one until you try to
//! connect — so that case is named too, instead of surfacing as ECONNREFUSED.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use kestrel_core::rpc::{Endpoint, Envelope, Request, Response, ENDPOINT_FILE};

/// How long to wait for the app to answer.
///
/// Generous, because a request can be an upload. Not unlimited, because a
/// command that never returns is worse than one that fails.
const TIMEOUT: Duration = Duration::from_secs(120);

pub type Result<T> = std::result::Result<T, String>;

/// Send one request and print the result.
pub fn send(request: Request, json: bool) -> Result<ExitCode> {
    let endpoint = read_endpoint()?;
    let response = exchange(&endpoint, &request)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&response).map_err(|e| e.to_string())?
        );
    } else {
        match &response {
            Response::Ok { message, path, url } => {
                // The URL and the path are the useful output, so they get a
                // line of their own that a shell can capture. The message is
                // context, not data.
                println!("{message}");
                if let Some(url) = url {
                    println!("{url}");
                }
                if let Some(path) = path {
                    println!("{path}");
                }
            }
            Response::Error { message } => eprintln!("kestrel: {message}"),
        }
    }

    Ok(if response.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn endpoint_path() -> Result<std::path::PathBuf> {
    // Must match `settings::config_dir()` in the app exactly, capital included:
    // on a case-sensitive filesystem "kestrel" and "Kestrel" are different
    // directories and every command would report a running app as absent.
    let dir = dirs::config_dir().ok_or("no config directory on this system")?;
    Ok(dir.join("Kestrel").join(ENDPOINT_FILE))
}

fn read_endpoint() -> Result<Endpoint> {
    let path = endpoint_path()?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Kestrel is not running. Start the app and try again.".to_string())?;

    serde_json::from_str(&text).map_err(|_| {
        format!(
            "the connection file at {} is unreadable — quit and restart Kestrel",
            path.display()
        )
    })
}

fn exchange(endpoint: &Endpoint, request: &Request) -> Result<Response> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|_| {
        // A file that exists pointing at a port that does not answer is a
        // crash, not a misconfiguration. Saying so saves someone reading a
        // connection error and looking for a firewall.
        "Kestrel is not running — a leftover connection file was found. \
         Start the app and try again."
            .to_string()
    })?;

    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;

    let envelope = Envelope {
        token: endpoint.token.clone(),
        request: request.clone(),
    };

    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut body = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    body.push(b'\n');
    writer.write_all(&body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| format!("no reply from Kestrel: {e}"))?;

    if line.trim().is_empty() {
        return Err("Kestrel closed the connection without replying".to_string());
    }
    serde_json::from_str(&line).map_err(|e| format!("unreadable reply: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_lives_beside_the_settings() {
        // If this drifted from where the app writes it, every app-driving
        // command would report "not running" on a running app.
        let path = endpoint_path().expect("a config directory");

        assert!(path.ends_with("Kestrel/rpc.json"), "{}", path.display());
    }

    #[test]
    fn a_missing_endpoint_says_the_app_is_not_running() {
        // The wording matters: this is the first failure everyone hits, and
        // "No such file or directory" sends people looking in the wrong place.
        let message = read_endpoint().err();

        // Only assert when the app genuinely is not running on this machine.
        if let Some(message) = message {
            assert!(message.contains("not running"), "{message}");
        }
    }

    #[test]
    fn the_timeout_is_long_enough_for_an_upload_but_not_forever() {
        assert!(TIMEOUT >= Duration::from_secs(30));
        assert!(TIMEOUT <= Duration::from_secs(600));
    }
}
