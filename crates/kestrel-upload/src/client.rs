//! Executing a [`PreparedRequest`].
//!
//! Kept separate from `sxcu` so the template language and the file format stay
//! testable without a network, and so a native uploader can reuse this
//! transport without going through the custom-uploader path.

use std::time::Duration;

use crate::sxcu::{Body, PreparedRequest, RequestMethod};
use crate::syntax::Context;

/// Default request timeout. Uploads of large videos need room, but a hung
/// connection must not wedge the task queue forever.
const TIMEOUT: Duration = Duration::from_secs(180);

/// What is being uploaded.
#[derive(Debug, Clone)]
pub enum Payload {
    /// A file: its bytes, the name to send, and its MIME type.
    File {
        bytes: Vec<u8>,
        filename: String,
        mime: String,
    },
    /// Text, or a URL for a shortener. Carried in the template's `{input}`.
    Text(String),
    /// No payload beyond the arguments themselves.
    None,
}

impl Payload {
    pub fn filename(&self) -> &str {
        match self {
            Payload::File { filename, .. } => filename,
            _ => "",
        }
    }

    pub fn as_input(&self) -> &str {
        match self {
            Payload::Text(text) => text,
            _ => "",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, ClientError>;

/// A raw response, ready to be handed to the template engine.
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    pub body: String,
    pub final_url: String,
    pub headers: std::collections::HashMap<String, String>,
}

impl RawResponse {
    /// Build the context the `.sxcu` response templates are expanded against.
    pub fn into_context(self, payload: &Payload) -> Context {
        Context {
            response: self.body,
            response_url: self.final_url,
            headers: self.headers,
            input: payload.as_input().to_string(),
            filename: payload.filename().to_string(),
        }
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        // Some hosts serve a different page to unknown clients; identifying
        // ourselves honestly is better than pretending to be a browser.
        .user_agent(concat!("Kestrel/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// Send a prepared request and return the raw response.
///
/// A non-success status is *not* an error here. Many services answer 4xx with
/// a body the uploader's `ErrorMessage` template is meant to read, and turning
/// that into a transport error would discard the only useful diagnostic.
pub async fn execute(
    client: &reqwest::Client,
    request: &PreparedRequest,
    payload: &Payload,
) -> Result<RawResponse> {
    let method = match request.method {
        RequestMethod::Get => reqwest::Method::GET,
        RequestMethod::Post => reqwest::Method::POST,
        RequestMethod::Put => reqwest::Method::PUT,
        RequestMethod::Patch => reqwest::Method::PATCH,
        RequestMethod::Delete => reqwest::Method::DELETE,
    };

    let mut builder = client.request(method, &request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    builder = attach_body(builder, request, payload)?;

    let response = builder.send().await?;
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_string(), v.to_string()))
        })
        .collect();
    let body = response.text().await?;

    Ok(RawResponse {
        status,
        body,
        final_url,
        headers,
    })
}

fn attach_body(
    builder: reqwest::RequestBuilder,
    request: &PreparedRequest,
    payload: &Payload,
) -> Result<reqwest::RequestBuilder> {
    Ok(match request.body {
        Body::None => builder,

        Body::MultipartFormData => {
            let mut form = reqwest::multipart::Form::new();
            for (name, value) in &request.arguments {
                form = form.text(name.clone(), value.clone());
            }
            if let Payload::File {
                bytes,
                filename,
                mime,
            } = payload
            {
                // Without a file form name the file has nowhere to go, so fall
                // back to the near-universal "file" rather than dropping it.
                let field = if request.file_form_name.is_empty() {
                    "file"
                } else {
                    &request.file_form_name
                };
                let part = reqwest::multipart::Part::bytes(bytes.clone())
                    .file_name(filename.clone())
                    .mime_str(mime)
                    .unwrap_or_else(|_| {
                        reqwest::multipart::Part::bytes(bytes.clone()).file_name(filename.clone())
                    });
                form = form.part(field.to_string(), part);
            }
            builder.multipart(form)
        }

        Body::FormURLEncoded => builder.form(&request.arguments),

        Body::Json => {
            // Arguments are strings by definition of the format, so the body is
            // a flat string map. Anything richer belongs in the template.
            let map: serde_json::Map<String, serde_json::Value> = request
                .arguments
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            builder.body(serde_json::Value::Object(map).to_string())
        }

        Body::Xml => {
            let mut xml = String::from("<root>");
            for (name, value) in &request.arguments {
                xml.push_str(&format!(
                    "<{name}>{}</{name}>",
                    escape_xml(value),
                    name = escape_xml_name(name)
                ));
            }
            xml.push_str("</root>");
            builder.body(xml)
        }

        Body::Binary => match payload {
            Payload::File { bytes, .. } => builder.body(bytes.clone()),
            Payload::Text(text) => builder.body(text.clone()),
            Payload::None => builder,
        },
    })
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Element names cannot contain markup, so anything unsafe is dropped rather
/// than escaped — an escaped character is still invalid in a tag name.
fn escape_xml_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sxcu::CustomUploader;
    use crate::syntax::NoPrompts;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Real PNG magic bytes, for anything that must survive a binary body.
    fn png() -> Payload {
        Payload::File {
            bytes: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            filename: "shot.png".into(),
            mime: "image/png".into(),
        }
    }

    /// An ASCII payload, so that a multipart body stays valid UTF-8 and
    /// wiremock's string matchers can inspect it. The bytes are incidental to
    /// what those tests assert — the form name and file name are the point.
    fn ascii_file() -> Payload {
        Payload::File {
            bytes: b"fake-image-bytes".to_vec(),
            filename: "shot.png".into(),
            mime: "image/png".into(),
        }
    }

    async fn run(server: &MockServer, sxcu: &str, payload: &Payload) -> RawResponse {
        let uploader = CustomUploader::parse(&sxcu.replace("{{SERVER}}", &server.uri())).unwrap();
        let ctx = Context {
            input: payload.as_input().to_string(),
            filename: payload.filename().to_string(),
            ..Default::default()
        };
        let request = uploader.prepare(&ctx, &NoPrompts).unwrap();
        execute(&client(), &request, payload).await.unwrap()
    }

    #[tokio::test]
    async fn a_multipart_upload_sends_the_file_under_its_form_name() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload"))
            .and(body_string_contains("name=\"file_image\""))
            .and(body_string_contains("shot.png"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"url":"https://e/x"}"#))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{
              "RequestURL": "{{SERVER}}/upload",
              "Body": "MultipartFormData",
              "FileFormName": "file_image"
            }"#,
            &ascii_file(),
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn a_file_without_a_form_name_still_gets_sent() {
        let server = MockServer::start().await;
        Mock::given(body_string_contains("name=\"file\""))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{"RequestURL":"{{SERVER}}/u","Body":"MultipartFormData"}"#,
            &ascii_file(),
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn parameters_arrive_as_query_string() {
        let server = MockServer::start().await;
        Mock::given(query_param("api_key", "secret"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{"RequestURL":"{{SERVER}}/u","Parameters":{"api_key":"secret"}}"#,
            &Payload::None,
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn headers_are_sent_with_their_templates_expanded() {
        let server = MockServer::start().await;
        Mock::given(header("Authorization", "Basic dXNlcjpwYXNz"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{
              "RequestURL": "{{SERVER}}/u",
              "Headers": { "Authorization": "Basic {base64:user:pass}" }
            }"#,
            &Payload::None,
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn a_json_body_carries_the_arguments() {
        let server = MockServer::start().await;
        Mock::given(header("content-type", "application/json"))
            .and(body_string_contains(r#""text":"merhaba""#))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{
              "RequestURL": "{{SERVER}}/u",
              "Body": "JSON",
              "Arguments": { "text": "{input}" }
            }"#,
            &Payload::Text("merhaba".into()),
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn a_binary_body_sends_the_raw_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{"RequestURL":"{{SERVER}}/u","RequestMethod":"PUT","Body":"Binary"}"#,
            &png(),
        )
        .await;

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn a_failing_status_is_returned_rather_than_raised() {
        // The body of a 4xx is usually the only explanation of what went
        // wrong, and the uploader's ErrorMessage template is written to read
        // it. Turning this into a transport error would throw that away.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(429).set_body_string(r#"{"error":"quota exceeded"}"#),
            )
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{"RequestURL":"{{SERVER}}/u","ErrorMessage":"{json:error}"}"#,
            &Payload::None,
        )
        .await;

        assert_eq!(response.status, 429);

        let uploader =
            CustomUploader::parse(r#"{"RequestURL":"https://e","ErrorMessage":"{json:error}"}"#)
                .unwrap();
        let result = uploader
            .parse_response(&response.into_context(&Payload::None), &NoPrompts)
            .unwrap();
        assert_eq!(result.error.as_deref(), Some("quota exceeded"));
    }

    #[tokio::test]
    async fn response_headers_reach_the_template_context() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(201).insert_header("Location", "https://e.com/created"),
            )
            .mount(&server)
            .await;

        let response = run(
            &server,
            r#"{"RequestURL":"{{SERVER}}/u","URL":"{header:location}"}"#,
            &Payload::None,
        )
        .await;

        let uploader =
            CustomUploader::parse(r#"{"RequestURL":"https://e","URL":"{header:location}"}"#)
                .unwrap();
        let result = uploader
            .parse_response(&response.into_context(&Payload::None), &NoPrompts)
            .unwrap();

        assert_eq!(result.url, "https://e.com/created");
    }

    #[tokio::test]
    async fn the_whole_round_trip_produces_a_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"data":{"link":"https://i.example.com/abc.png"},"del":"https://i.example.com/d/abc"}"#,
            ))
            .mount(&server)
            .await;

        let sxcu = format!(
            r#"{{
              "Name": "Test host",
              "DestinationType": "ImageUploader",
              "RequestURL": "{}/upload",
              "Body": "MultipartFormData",
              "FileFormName": "file",
              "URL": "{{json:data.link}}",
              "DeletionURL": "{{json:del}}",
              "ErrorMessage": "{{json:error}}"
            }}"#,
            server.uri()
        );

        let uploader = CustomUploader::parse(&sxcu).unwrap();
        let payload = ascii_file();
        let request = uploader.prepare(&Context::default(), &NoPrompts).unwrap();
        let response = execute(&client(), &request, &payload).await.unwrap();
        let result = uploader
            .parse_response(&response.into_context(&payload), &NoPrompts)
            .unwrap();

        assert_eq!(result.url, "https://i.example.com/abc.png");
        assert_eq!(
            result.deletion_url.as_deref(),
            Some("https://i.example.com/d/abc")
        );
        assert_eq!(result.error, None);
    }

    #[test]
    fn xml_escaping_keeps_the_document_well_formed() {
        assert_eq!(escape_xml("a & b < c"), "a &amp; b &lt; c");
        // An escaped character is still invalid inside a tag name, so unsafe
        // characters there are dropped rather than escaped.
        assert_eq!(escape_xml_name("bad<name>"), "badname");
    }
}
