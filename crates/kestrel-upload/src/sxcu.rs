//! ShareX custom uploader (`.sxcu`) files.
//!
//! The on-disk shape is mirrored exactly, including ShareX's spellings and its
//! comma-separated `DestinationType` flags, so that files published for ShareX
//! import here without editing. Reference:
//! <https://getsharex.com/docs/custom-uploader>

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::syntax::{self, Context, Prompter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RequestMethod {
    Get,
    #[default]
    Post,
    Put,
    Patch,
    Delete,
}

impl RequestMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestMethod::Get => "GET",
            RequestMethod::Post => "POST",
            RequestMethod::Put => "PUT",
            RequestMethod::Patch => "PATCH",
            RequestMethod::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Body {
    #[default]
    None,
    MultipartFormData,
    FormURLEncoded,
    #[serde(rename = "JSON")]
    Json,
    #[serde(rename = "XML")]
    Xml,
    Binary,
}

impl Body {
    /// The `Content-Type` this body implies, unless the file overrides it.
    pub fn content_type(self) -> Option<&'static str> {
        match self {
            Body::None => None,
            // The boundary is appended by the HTTP client.
            Body::MultipartFormData => Some("multipart/form-data"),
            Body::FormURLEncoded => Some("application/x-www-form-urlencoded"),
            Body::Json => Some("application/json"),
            Body::Xml => Some("application/xml"),
            Body::Binary => Some("application/octet-stream"),
        }
    }
}

/// What an uploader can accept. ShareX stores these as a comma-separated
/// string, e.g. `"ImageUploader, TextUploader"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DestinationType {
    pub image: bool,
    pub text: bool,
    pub file: bool,
    pub url_shortener: bool,
    pub url_sharing: bool,
}

impl DestinationType {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn parse(raw: &str) -> Self {
        let mut kinds = Self::default();
        for part in raw.split(',') {
            match part.trim() {
                "ImageUploader" => kinds.image = true,
                "TextUploader" => kinds.text = true,
                "FileUploader" => kinds.file = true,
                "URLShortener" => kinds.url_shortener = true,
                "URLSharingService" => kinds.url_sharing = true,
                // "None" and anything unrecognised simply contribute nothing;
                // a future ShareX flag must not make the whole file unloadable.
                _ => {}
            }
        }
        kinds
    }

    fn to_raw(self) -> String {
        let mut parts = Vec::new();
        if self.image {
            parts.push("ImageUploader");
        }
        if self.text {
            parts.push("TextUploader");
        }
        if self.file {
            parts.push("FileUploader");
        }
        if self.url_shortener {
            parts.push("URLShortener");
        }
        if self.url_sharing {
            parts.push("URLSharingService");
        }
        if parts.is_empty() {
            "None".to_string()
        } else {
            parts.join(", ")
        }
    }
}

impl Serialize for DestinationType {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_raw())
    }
}

impl<'de> Deserialize<'de> for DestinationType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(DestinationType::parse(&raw))
    }
}

/// A parsed `.sxcu` file.
///
/// Field names are ShareX's, which is why they are PascalCase. `BTreeMap` keeps
/// parameters and headers in a stable order so a re-exported file diffs cleanly
/// against the original.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CustomUploader {
    pub version: String,
    pub name: String,
    pub destination_type: DestinationType,
    pub request_method: RequestMethod,
    #[serde(rename = "RequestURL")]
    pub request_url: String,
    pub parameters: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Body,
    pub arguments: BTreeMap<String, String>,
    pub file_form_name: String,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "ThumbnailURL")]
    pub thumbnail_url: String,
    #[serde(rename = "DeletionURL")]
    pub deletion_url: String,
    pub error_message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SxcuError {
    #[error("not a valid .sxcu file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the uploader has no request URL")]
    NoRequestUrl,
    #[error(transparent)]
    Syntax(#[from] syntax::SyntaxError),
}

pub type Result<T> = std::result::Result<T, SxcuError>;

/// A request ready to be executed, with every template already expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    pub method: RequestMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
    pub arguments: Vec<(String, String)>,
    pub file_form_name: String,
}

/// What a response was parsed into.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UploadResult {
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub deletion_url: Option<String>,
    /// Set when the service reported a failure through `ErrorMessage`.
    pub error: Option<String>,
}

impl CustomUploader {
    pub fn parse(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// The name to show in the destination list.
    ///
    /// ShareX leaves `Name` empty when the request URL's domain is descriptive
    /// enough, and falls back to that domain.
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        domain_of(&self.request_url).unwrap_or_else(|| "Custom uploader".to_string())
    }

    /// Expand every template that goes into the request.
    pub fn prepare(&self, ctx: &Context, prompter: &dyn Prompter) -> Result<PreparedRequest> {
        if self.request_url.trim().is_empty() {
            return Err(SxcuError::NoRequestUrl);
        }

        let url = syntax::expand(&self.request_url, ctx, prompter)?;
        let query: Vec<(String, String)> = self
            .parameters
            .iter()
            .map(|(k, v)| Ok((k.clone(), syntax::expand(v, ctx, prompter)?)))
            .collect::<Result<_>>()?;

        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| Ok((k.clone(), syntax::expand(v, ctx, prompter)?)))
            .collect::<Result<_>>()?;

        // The body's implied content type is a default, not an override: a file
        // that sets Content-Type explicitly means it.
        if let Some(content_type) = self.body.content_type() {
            let already_set = headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            if !already_set && self.body != Body::MultipartFormData {
                headers.push(("Content-Type".to_string(), content_type.to_string()));
            }
        }

        let arguments: Vec<(String, String)> = self
            .arguments
            .iter()
            .map(|(k, v)| Ok((k.clone(), syntax::expand(v, ctx, prompter)?)))
            .collect::<Result<_>>()?;

        Ok(PreparedRequest {
            method: self.request_method,
            url: append_query(&url, &query),
            headers,
            body: self.body,
            arguments,
            file_form_name: self.file_form_name.clone(),
        })
    }

    /// Parse a response into the URLs the pipeline needs.
    pub fn parse_response(&self, ctx: &Context, prompter: &dyn Prompter) -> Result<UploadResult> {
        // The error message is checked first: a service that returns 200 with
        // an error body must not be treated as a success.
        let error = if self.error_message.trim().is_empty() {
            None
        } else {
            let message = syntax::expand(&self.error_message, ctx, prompter)?;
            (!message.trim().is_empty()).then_some(message)
        };

        // An empty URL template means the whole response body is the URL.
        let url = if self.url.trim().is_empty() {
            ctx.response.trim().to_string()
        } else {
            syntax::expand(&self.url, ctx, prompter)?
        };

        Ok(UploadResult {
            url,
            thumbnail_url: expand_optional(&self.thumbnail_url, ctx, prompter)?,
            deletion_url: expand_optional(&self.deletion_url, ctx, prompter)?,
            error,
        })
    }
}

fn expand_optional(
    template: &str,
    ctx: &Context,
    prompter: &dyn Prompter,
) -> Result<Option<String>> {
    if template.trim().is_empty() {
        return Ok(None);
    }
    let value = syntax::expand(template, ctx, prompter)?;
    Ok((!value.trim().is_empty()).then_some(value))
}

fn append_query(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_string();
    }
    let encoded: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect();
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{}", encoded.join("&"))
}

/// Percent-encode everything outside the unreserved set (RFC 3986).
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn domain_of(url: &str) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme.split(['/', '?', '#']).next()?;
    let host = host.split('@').next_back()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::NoPrompts;

    /// The example from the ShareX documentation, verbatim.
    const DOC_EXAMPLE: &str = r#"{
      "Version": "17.0.0",
      "Name": "Example",
      "DestinationType": "ImageUploader, TextUploader, FileUploader",
      "RequestMethod": "POST",
      "RequestURL": "https://example.com/upload.php",
      "Parameters": { "Parameter1": "Value1" },
      "Headers": { "Header1": "Value1" },
      "Body": "MultipartFormData",
      "Arguments": { "Argument1": "Value1" },
      "FileFormName": "file",
      "URL": "{json:url}",
      "ThumbnailURL": "{json:thumbnail_url}",
      "DeletionURL": "{json:deletion_url}",
      "ErrorMessage": "{json:error}"
    }"#;

    #[test]
    fn the_documented_example_parses() {
        let uploader = CustomUploader::parse(DOC_EXAMPLE).expect("should parse");

        assert_eq!(uploader.name, "Example");
        assert_eq!(uploader.request_method, RequestMethod::Post);
        assert_eq!(uploader.body, Body::MultipartFormData);
        assert_eq!(uploader.file_form_name, "file");
        assert!(uploader.destination_type.image);
        assert!(uploader.destination_type.text);
        assert!(uploader.destination_type.file);
        assert!(!uploader.destination_type.url_shortener);
    }

    #[test]
    fn destination_flags_round_trip() {
        let uploader = CustomUploader::parse(DOC_EXAMPLE).unwrap();
        let json = serde_json::to_string(&uploader).unwrap();
        let back = CustomUploader::parse(&json).unwrap();

        assert_eq!(back, uploader);
        assert!(json.contains("ImageUploader, TextUploader, FileUploader"));
    }

    #[test]
    fn an_unknown_destination_flag_does_not_break_the_file() {
        // A newer ShareX may add flags; the rest of the file must still load.
        let json =
            r#"{"DestinationType":"ImageUploader, SomethingNew","RequestURL":"https://a.b"}"#;
        let uploader = CustomUploader::parse(json).unwrap();

        assert!(uploader.destination_type.image);
    }

    #[test]
    fn a_minimal_file_parses_with_defaults() {
        let uploader = CustomUploader::parse(r#"{"RequestURL":"https://a.b/u"}"#).unwrap();

        assert_eq!(uploader.request_method, RequestMethod::Post);
        assert_eq!(uploader.body, Body::None);
        assert!(uploader.destination_type.is_empty());
    }

    #[test]
    fn the_name_falls_back_to_the_request_domain() {
        let uploader =
            CustomUploader::parse(r#"{"RequestURL":"https://example.com/upload.php"}"#).unwrap();
        assert_eq!(uploader.display_name(), "example.com");

        let named =
            CustomUploader::parse(r#"{"Name":"My host","RequestURL":"https://example.com/u"}"#)
                .unwrap();
        assert_eq!(named.display_name(), "My host");
    }

    #[test]
    fn parameters_become_a_query_string() {
        let json = r#"{
          "RequestURL": "https://example.com/upload.php",
          "Parameters": { "api_key": "eUM14R4g4pMS", "private": "true" }
        }"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        assert!(prepared.url.starts_with("https://example.com/upload.php?"));
        assert!(prepared.url.contains("api_key=eUM14R4g4pMS"));
        assert!(prepared.url.contains("private=true"));
    }

    #[test]
    fn query_values_are_percent_encoded() {
        let json = r#"{"RequestURL":"https://e.com/u","Parameters":{"q":"a b&c=d"}}"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        assert!(prepared.url.contains("q=a%20b%26c%3Dd"), "{}", prepared.url);
    }

    #[test]
    fn a_url_that_already_has_a_query_gets_an_ampersand() {
        let json = r#"{"RequestURL":"https://e.com/u?v=1","Parameters":{"w":"2"}}"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        assert_eq!(prepared.url, "https://e.com/u?v=1&w=2");
    }

    #[test]
    fn templates_in_headers_and_arguments_are_expanded() {
        let json = r#"{
          "RequestURL": "https://e.com/u",
          "Headers": { "Authorization": "Basic {base64:user:pass}" },
          "Body": "JSON",
          "Arguments": { "text": "{input}" }
        }"#;
        let ctx = Context {
            input: "merhaba".into(),
            ..Default::default()
        };
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&ctx, &NoPrompts)
            .unwrap();

        assert!(prepared
            .headers
            .contains(&("Authorization".into(), "Basic dXNlcjpwYXNz".into())));
        assert_eq!(
            prepared.arguments,
            [("text".to_string(), "merhaba".to_string())]
        );
    }

    #[test]
    fn a_body_type_supplies_its_content_type() {
        let json = r#"{"RequestURL":"https://e.com/u","Body":"JSON"}"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        assert!(prepared
            .headers
            .contains(&("Content-Type".into(), "application/json".into())));
    }

    #[test]
    fn an_explicit_content_type_is_not_overridden() {
        let json = r#"{
          "RequestURL": "https://e.com/u",
          "Body": "JSON",
          "Headers": { "Content-Type": "application/vnd.custom+json" }
        }"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        let content_types: Vec<&String> = prepared
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(content_types, ["application/vnd.custom+json"]);
    }

    #[test]
    fn multipart_leaves_content_type_to_the_http_client() {
        // The boundary parameter is generated at send time, so setting the
        // header here would produce a request no server can parse.
        let json = r#"{"RequestURL":"https://e.com/u","Body":"MultipartFormData"}"#;
        let prepared = CustomUploader::parse(json)
            .unwrap()
            .prepare(&Context::default(), &NoPrompts)
            .unwrap();

        assert!(!prepared
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type")));
    }

    #[test]
    fn a_response_is_parsed_into_urls() {
        let uploader = CustomUploader::parse(DOC_EXAMPLE).unwrap();
        let ctx = Context::with_response(
            r#"{"url":"https://e.com/a.png","thumbnail_url":"https://e.com/t.png","deletion_url":"https://e.com/d"}"#,
        );

        let result = uploader.parse_response(&ctx, &NoPrompts).unwrap();

        assert_eq!(result.url, "https://e.com/a.png");
        assert_eq!(result.thumbnail_url.as_deref(), Some("https://e.com/t.png"));
        assert_eq!(result.deletion_url.as_deref(), Some("https://e.com/d"));
        assert_eq!(result.error, None);
    }

    #[test]
    fn an_empty_url_template_means_the_body_is_the_url() {
        let json = r#"{"RequestURL":"https://e.com/u"}"#;
        let ctx = Context::with_response("  https://e.com/direct.png\n");

        let result = CustomUploader::parse(json)
            .unwrap()
            .parse_response(&ctx, &NoPrompts)
            .unwrap();

        assert_eq!(result.url, "https://e.com/direct.png");
    }

    #[test]
    fn an_error_in_the_body_is_reported_even_on_a_successful_status() {
        // Plenty of services answer 200 with an error payload; treating that as
        // a success would hand the user a broken link.
        let uploader = CustomUploader::parse(DOC_EXAMPLE).unwrap();
        let ctx = Context::with_response(r#"{"error":"quota exceeded"}"#);

        let result = uploader.parse_response(&ctx, &NoPrompts).unwrap();

        assert_eq!(result.error.as_deref(), Some("quota exceeded"));
    }

    #[test]
    fn absent_optional_urls_are_none_not_empty_strings() {
        let uploader = CustomUploader::parse(DOC_EXAMPLE).unwrap();
        let ctx = Context::with_response(r#"{"url":"https://e.com/a.png"}"#);

        let result = uploader.parse_response(&ctx, &NoPrompts).unwrap();

        assert_eq!(result.thumbnail_url, None);
        assert_eq!(result.deletion_url, None);
    }

    #[test]
    fn an_uploader_without_a_request_url_is_rejected() {
        let uploader = CustomUploader::parse(r#"{"Name":"broken"}"#).unwrap();
        assert!(matches!(
            uploader.prepare(&Context::default(), &NoPrompts),
            Err(SxcuError::NoRequestUrl)
        ));
    }

    #[test]
    fn every_request_method_parses() {
        for (raw, expected) in [
            ("GET", RequestMethod::Get),
            ("POST", RequestMethod::Post),
            ("PUT", RequestMethod::Put),
            ("PATCH", RequestMethod::Patch),
            ("DELETE", RequestMethod::Delete),
        ] {
            let json = format!(r#"{{"RequestURL":"https://e.com","RequestMethod":"{raw}"}}"#);
            let uploader = CustomUploader::parse(&json).unwrap();
            assert_eq!(uploader.request_method, expected);
            assert_eq!(uploader.request_method.as_str(), raw);
        }
    }

    #[test]
    fn every_body_type_parses() {
        for raw in [
            "None",
            "MultipartFormData",
            "FormURLEncoded",
            "JSON",
            "XML",
            "Binary",
        ] {
            let json = format!(r#"{{"RequestURL":"https://e.com","Body":"{raw}"}}"#);
            assert!(
                CustomUploader::parse(&json).is_ok(),
                "{raw} should be a valid body type"
            );
        }
    }
}
