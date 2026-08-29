//! The wire protocol between the `kestrel` command and a running app.
//!
//! Both halves depend on this, and neither is allowed to depend on the other,
//! so the types live here with the rest of the domain model.
//!
//! # Transport
//!
//! A TCP listener bound to `127.0.0.1:0`, with the chosen port written to a
//! file. Not a Unix socket, because Windows has no equivalent in `std` and
//! adding a named-pipe crate to get one buys nothing here; not a fixed port,
//! because two accounts on one machine would collide.
//!
//! # Why there is a token
//!
//! A localhost port that takes a screenshot on request is a real attack
//! surface: on a shared machine *any* local process could ask for one, and on
//! any machine a page in a browser could try. So every request carries a secret
//! read from a file only the user can read, and a request without the right one
//! is refused before it is even parsed as a command.
//!
//! This is not a substitute for OS-level isolation — a process running as the
//! same user can read the file. It stops the cases that matter in practice:
//! other users, and anything that can reach the port but not the filesystem.

use serde::{Deserialize, Serialize};

/// The file, inside the app's config directory, that says where to connect.
pub const ENDPOINT_FILE: &str = "rpc.json";

/// Where a running app is listening, and the secret needed to talk to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    /// The process that wrote this, so a stale file left by a crash can be
    /// told apart from a live one without connecting.
    pub pid: u32,
}

/// One instruction to a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Take a capture using one of the built-in methods.
    Capture { method: crate::model::CaptureMethod },
    /// Run a configured workflow by id or by name.
    RunWorkflow { workflow: String },
    /// Upload a file that already exists on disk.
    Upload {
        path: String,
        destination: Option<String>,
    },
    /// Open a file in the annotation editor.
    Edit { path: String },
    /// Pin an image above every other window.
    Pin { path: String },
    /// Import a `.sxcu` uploader or a `.sxie` effect preset.
    Import { path: String },
    /// Bring the main window forward.
    Show,
    /// Check that the app is there. Used to tell a live endpoint from a stale
    /// file without causing a side effect.
    Ping,
}

/// What the app sends back. One response per request, then the connection
/// closes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// Done. `message` is for a human; `path` and `url` are set when the
    /// request produced one, so a script can use the result.
    Ok {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// The request was understood and refused, or it failed.
    Error { message: String },
}

impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Response::Ok {
            message: message.into(),
            path: None,
            url: None,
        }
    }

    pub fn with_path(message: impl Into<String>, path: Option<String>) -> Self {
        Response::Ok {
            message: message.into(),
            path,
            url: None,
        }
    }

    pub fn error(message: impl std::fmt::Display) -> Self {
        Response::Error {
            message: message.to_string(),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Response::Ok { .. })
    }
}

/// One request as it goes over the wire: the token, then the instruction.
///
/// The token is a sibling of the request rather than part of it so that an
/// unauthorised caller is rejected without the command ever being interpreted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub token: String,
    pub request: Request,
}

/// Compare two tokens without leaking how much of the prefix matched.
///
/// A naive `==` on strings returns as soon as it finds a difference, and the
/// difference in timing is measurable over enough attempts. This is cheap
/// insurance on a comparison that runs once per request.
pub fn token_matches(expected: &str, given: &str) -> bool {
    if expected.len() != given.len() {
        return false;
    }
    expected
        .bytes()
        .zip(given.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CaptureMethod;

    #[test]
    fn a_request_survives_a_json_round_trip() {
        let envelope = Envelope {
            token: "secret".into(),
            request: Request::Capture {
                method: CaptureMethod::Region,
            },
        };
        let json = serde_json::to_string(&envelope).unwrap();

        assert_eq!(serde_json::from_str::<Envelope>(&json).unwrap(), envelope);
    }

    #[test]
    fn every_request_variant_round_trips() {
        // The two halves are separate binaries that can be built at different
        // times, so the wire format has to be exercised rather than assumed.
        let requests = [
            Request::Capture {
                method: CaptureMethod::Fullscreen,
            },
            Request::RunWorkflow {
                workflow: "region".into(),
            },
            Request::Upload {
                path: "/a.png".into(),
                destination: Some("imgur".into()),
            },
            Request::Edit {
                path: "/a.png".into(),
            },
            Request::Pin {
                path: "/a.png".into(),
            },
            Request::Import {
                path: "/a.sxcu".into(),
            },
            Request::Show,
            Request::Ping,
        ];

        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            assert_eq!(
                serde_json::from_str::<Request>(&json).unwrap(),
                request,
                "{json}"
            );
        }
    }

    #[test]
    fn a_response_without_a_path_or_url_omits_them() {
        // The CLI prints whichever of these is present, so a null would show up
        // as the word "null" in a shell pipeline.
        let json = serde_json::to_string(&Response::ok("done")).unwrap();

        assert!(!json.contains("path"), "{json}");
        assert!(!json.contains("url"), "{json}");
    }

    #[test]
    fn a_response_carries_a_path_when_there_is_one() {
        let response = Response::with_path("saved", Some("/shot.png".into()));
        let json = serde_json::to_string(&response).unwrap();

        assert!(response.is_ok());
        assert!(json.contains("/shot.png"), "{json}");
    }

    #[test]
    fn an_error_response_is_not_ok() {
        assert!(!Response::error("nope").is_ok());
    }

    #[test]
    fn matching_tokens_compare_equal() {
        assert!(token_matches("abc123", "abc123"));
    }

    #[test]
    fn a_wrong_token_is_rejected_however_it_is_wrong() {
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "xbc123"));
        assert!(!token_matches("abc123", "abc12"));
        assert!(!token_matches("abc123", "abc1234"));
        assert!(!token_matches("abc123", ""));
    }

    #[test]
    fn an_empty_expected_token_does_not_accept_everything() {
        // If the endpoint file were somehow written without a token, refusing
        // everything is the safe failure; accepting everything is not.
        assert!(!token_matches("", "anything"));
        assert!(token_matches("", ""));
    }

    #[test]
    fn an_endpoint_survives_a_json_round_trip() {
        let endpoint = Endpoint {
            port: 51234,
            token: "t".into(),
            pid: 42,
        };
        let json = serde_json::to_string(&endpoint).unwrap();

        assert_eq!(serde_json::from_str::<Endpoint>(&json).unwrap(), endpoint);
    }
}
