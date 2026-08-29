//! ShareX's custom-uploader template language.
//!
//! This is the highest-leverage piece of Kestrel. ShareX's `.sxcu` format is a
//! small language for describing any HTTP upload endpoint, and the community
//! has published hundreds of ready-made files. Implementing the language
//! exactly means those files work unmodified — which is worth far more than
//! any number of hand-written service integrations.
//!
//! Reference: <https://getsharex.com/docs/custom-uploader>
//!
//! The grammar is `{function:argument}` with `|` separating parameters, and
//! `\` escaping any of `{`, `}`, `|`, `\`. Functions nest: the argument of one
//! may itself contain another, which is how `{outputbox:Result|{json:a.b}}`
//! works.

use std::collections::HashMap;

use base64::Engine;

/// Everything a template may need. Anything absent expands to an empty string
/// rather than failing, matching ShareX: a template referring to a response
/// field the server did not send should produce a blank, not an error dialog.
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// Raw response body.
    pub response: String,
    /// Final URL after any redirects.
    pub response_url: String,
    pub headers: HashMap<String, String>,
    /// Text being uploaded, or the URL being shortened.
    pub input: String,
    pub filename: String,
}

impl Context {
    pub fn with_response(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            ..Default::default()
        }
    }
}

/// Something that has to ask the user mid-upload.
///
/// `{select:}`, `{inputbox:}` and `{outputbox:}` are interactive by design, so
/// expansion cannot be a pure function of the response. Callers supply a
/// prompter; tests supply a scripted one.
pub trait Prompter {
    /// Ask the user to pick one of `options`.
    fn select(&self, options: &[String]) -> Option<String>;
    /// Ask for free text, given a window title and a default.
    fn input(&self, title: Option<&str>, default: Option<&str>) -> Option<String>;
    /// Show a result. Returns nothing: `{outputbox:}` expands to an empty string.
    fn output(&self, title: Option<&str>, message: &str);
}

/// A prompter that answers nothing, for contexts with no user attached
/// (the CLI, tests of non-interactive templates).
pub struct NoPrompts;

impl Prompter for NoPrompts {
    fn select(&self, options: &[String]) -> Option<String> {
        // Falling back to the first option keeps a `{select:}` template usable
        // in an automated run instead of failing outright.
        options.first().cloned()
    }
    fn input(&self, _title: Option<&str>, default: Option<&str>) -> Option<String> {
        default.map(str::to_string)
    }
    fn output(&self, _title: Option<&str>, _message: &str) {}
}

#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    #[error("unterminated '{{' at byte {0}")]
    Unterminated(usize),
    #[error("invalid regular expression: {0}")]
    Regex(#[from] regex::Error),
}

pub type Result<T> = std::result::Result<T, SyntaxError>;

/// Expand a template against a response.
pub fn expand(template: &str, ctx: &Context, prompter: &dyn Prompter) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                // Only the four special characters are escapable; anything else
                // keeps its backslash, so Windows paths and regex classes in a
                // template survive.
                let next = chars[i + 1];
                if matches!(next, '{' | '}' | '|' | '\\') {
                    out.push(next);
                    i += 2;
                } else {
                    out.push('\\');
                    i += 1;
                }
            }
            '{' => {
                let (body, next) = read_group(&chars, i)?;
                out.push_str(&expand_function(&body, ctx, prompter)?);
                i = next;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Read a `{...}` group starting at `start`, honouring nesting and escapes.
fn read_group(chars: &[char], start: usize) -> Result<(String, usize)> {
    let mut depth = 0;
    let mut body = String::new();
    let mut i = start;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            body.push(c);
            body.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '{' {
            depth += 1;
            if depth > 1 {
                body.push(c);
            }
            i += 1;
            continue;
        }
        if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Ok((body, i + 1));
            }
            body.push(c);
            i += 1;
            continue;
        }
        body.push(c);
        i += 1;
    }
    Err(SyntaxError::Unterminated(start))
}

/// Split on unescaped `|`.
fn split_params(body: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = body.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                if matches!(next, '{' | '}' | '|' | '\\') {
                    parts.last_mut().expect("always non-empty").push(next);
                } else {
                    let last = parts.last_mut().expect("always non-empty");
                    last.push('\\');
                    last.push(next);
                }
            }
            continue;
        }
        if c == '|' {
            parts.push(String::new());
            continue;
        }
        parts.last_mut().expect("always non-empty").push(c);
    }
    parts
}

fn expand_function(body: &str, ctx: &Context, prompter: &dyn Prompter) -> Result<String> {
    let (name, argument) = match body.split_once(':') {
        Some((name, rest)) => (name, Some(rest)),
        None => (body, None),
    };

    // Arguments may themselves contain functions, so expand them first.
    let argument = match argument {
        Some(a) => Some(expand(a, ctx, prompter)?),
        None => None,
    };
    let params: Vec<String> = argument.as_deref().map(split_params).unwrap_or_default();
    let first = params.first().map(String::as_str).unwrap_or_default();

    Ok(match name {
        "response" => ctx.response.clone(),
        "responseurl" => ctx.response_url.clone(),
        "input" => ctx.input.clone(),
        "filename" => ctx.filename.clone(),

        "header" => ctx
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(first))
            .map(|(_, value)| value.clone())
            .unwrap_or_default(),

        "json" => json_path(&params, ctx),
        "regex" => regex_match(&params, ctx)?,
        "xml" => xml_path(&params, ctx),

        "base64" => base64::engine::general_purpose::STANDARD.encode(first.as_bytes()),

        "random" => {
            if params.is_empty() {
                String::new()
            } else {
                params[pseudo_random(params.len())].clone()
            }
        }

        "select" => prompter.select(&params).unwrap_or_default(),
        "inputbox" => {
            // Both parameters are optional: `{inputbox}`, `{inputbox:title}`
            // and `{inputbox:title|default}` are all valid.
            let title = params.first().filter(|s| !s.is_empty()).map(String::as_str);
            let default = params.get(1).map(String::as_str);
            prompter.input(title, default).unwrap_or_default()
        }
        "outputbox" => {
            // With one parameter it is the message; with two, title then
            // message. Either way it contributes nothing to the URL.
            let (title, message) = match params.len() {
                0 => (None, ""),
                1 => (None, params[0].as_str()),
                _ => (Some(params[0].as_str()), params[1].as_str()),
            };
            prompter.output(title, message);
            String::new()
        }

        // An unknown function is left verbatim so a typo stays visible rather
        // than silently turning into an empty URL.
        _ => format!("{{{body}}}"),
    })
}

/// `{json:path}` or `{json:input|path}`.
fn json_path(params: &[String], ctx: &Context) -> String {
    let (source, path) = source_and_query(params, ctx);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&source) else {
        return String::new();
    };
    // ShareX accepts paths with or without the leading `$`.
    let normalised = if path.starts_with('$') {
        path.to_string()
    } else {
        format!("$.{path}")
    };
    let Ok(query) = serde_json_path::JsonPath::parse(&normalised) else {
        return String::new();
    };

    match query.query(&value).first() {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// `{regex:pattern}`, `{regex:pattern|group}` or `{regex:input|pattern|group}`.
fn regex_match(params: &[String], ctx: &Context) -> Result<String> {
    // Disambiguating the two- and three-parameter forms is the awkward part:
    // ShareX decides by whether the first parameter parses as a pattern that
    // matches. We use the simpler, predictable rule — three parameters means
    // an explicit input.
    let (source, pattern, group) = match params.len() {
        0 => return Ok(String::new()),
        1 => (ctx.response.clone(), params[0].clone(), None),
        2 => (
            ctx.response.clone(),
            params[0].clone(),
            Some(params[1].clone()),
        ),
        _ => (
            params[0].clone(),
            params[1].clone(),
            Some(params[2].clone()),
        ),
    };

    let re = regex::Regex::new(&pattern)?;
    let Some(captures) = re.captures(&source) else {
        return Ok(String::new());
    };

    Ok(match group {
        None => captures
            .get(0)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default(),
        Some(group) => match group.parse::<usize>() {
            Ok(index) => captures
                .get(index)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            Err(_) => captures
                .name(&group)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
        },
    })
}

/// `{xml:xpath}` or `{xml:input|xpath}`.
fn xml_path(params: &[String], ctx: &Context) -> String {
    let (source, path) = source_and_query(params, ctx);

    let Ok(package) = sxd_document::parser::parse(&source) else {
        return String::new();
    };
    let document = package.as_document();
    let Ok(value) = sxd_xpath::evaluate_xpath(&document, &path) else {
        return String::new();
    };
    value.string()
}

/// Both `{json:}` and `{xml:}` accept an optional leading input parameter.
fn source_and_query(params: &[String], ctx: &Context) -> (String, String) {
    match params.len() {
        0 => (ctx.response.clone(), String::new()),
        1 => (ctx.response.clone(), params[0].clone()),
        _ => (params[0].clone(), params[1].clone()),
    }
}

/// A tiny, dependency-free source of randomness for `{random:}`.
///
/// The choice only has to vary between uploads — it is picking a CDN hostname,
/// not generating a key — so the clock is enough and avoids pulling `rand` in.
fn pseudo_random(len: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % len.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        let mut headers = HashMap::new();
        headers.insert(
            "Location".to_string(),
            "https://example.com/abc".to_string(),
        );
        Context {
            response: r#"{"status":200,"data":{"link":"https://example.com/image.png"},"files":[{"name":"image.png","url":"https://example.com/a.png"}]}"#.to_string(),
            response_url: "https://example.com/final".to_string(),
            headers,
            input: "merhaba".to_string(),
            filename: "shot.png".to_string(),
        }
    }

    fn run(template: &str) -> String {
        expand(template, &ctx(), &NoPrompts).expect("template should expand")
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(run("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn response_and_url_and_input_and_filename() {
        assert_eq!(run("{responseurl}"), "https://example.com/final");
        assert_eq!(run("{input}"), "merhaba");
        assert_eq!(run("{filename}"), "shot.png");
        assert!(run("{response}").starts_with('{'));
    }

    #[test]
    fn headers_are_matched_case_insensitively() {
        // Servers are inconsistent about header casing and templates are
        // written by hand, so this must not be case-sensitive.
        assert_eq!(run("{header:location}"), "https://example.com/abc");
        assert_eq!(run("{header:LOCATION}"), "https://example.com/abc");
        assert_eq!(run("{header:missing}"), "");
    }

    #[test]
    fn json_paths_from_the_sharex_documentation() {
        assert_eq!(run("{json:data.link}"), "https://example.com/image.png");
        assert_eq!(run("{json:files[0].url}"), "https://example.com/a.png");
    }

    #[test]
    fn a_json_path_that_matches_nothing_yields_empty() {
        assert_eq!(run("{json:nope.missing}"), "");
    }

    #[test]
    fn a_non_json_response_yields_empty_rather_than_failing() {
        let ctx = Context::with_response("<html>not json</html>");
        assert_eq!(expand("{json:a.b}", &ctx, &NoPrompts).unwrap(), "");
    }

    #[test]
    fn json_numbers_stringify() {
        assert_eq!(run("{json:status}"), "200");
    }

    #[test]
    fn a_url_can_be_built_from_a_domain_and_a_parsed_id() {
        let ctx = Context::with_response(r#"{"id":"xyz789"}"#);
        assert_eq!(
            expand("https://i.example.com/{json:id}.png", &ctx, &NoPrompts).unwrap(),
            "https://i.example.com/xyz789.png"
        );
    }

    #[test]
    fn regex_with_a_group_index() {
        let ctx = Context::with_response(r#"<a href="https://example.com/x.png">"#);
        assert_eq!(
            expand(r#"{regex:href="(.+)"|1}"#, &ctx, &NoPrompts).unwrap(),
            "https://example.com/x.png"
        );
    }

    #[test]
    fn regex_with_a_named_group() {
        let ctx = Context::with_response(r#"<a href="https://example.com/y.png">"#);
        assert_eq!(
            expand(r#"{regex:href="(?<url>.+)"|url}"#, &ctx, &NoPrompts).unwrap(),
            "https://example.com/y.png"
        );
    }

    #[test]
    fn regex_with_no_group_returns_the_whole_match() {
        let ctx = Context::with_response("id=4815162342;");
        assert_eq!(
            expand("{regex:id=[0-9]+}", &ctx, &NoPrompts).unwrap(),
            "id=4815162342"
        );
    }

    #[test]
    fn a_regex_that_does_not_match_yields_empty() {
        let ctx = Context::with_response("nothing here");
        assert_eq!(expand("{regex:zzz(.+)|1}", &ctx, &NoPrompts).unwrap(), "");
    }

    #[test]
    fn an_invalid_regex_is_an_error_not_a_silent_blank() {
        // A broken pattern is a mistake in the uploader definition, and the
        // author needs to be told rather than shipped an empty URL.
        let ctx = Context::with_response("x");
        assert!(matches!(
            expand("{regex:([unclosed}", &ctx, &NoPrompts),
            Err(SyntaxError::Regex(_))
        ));
    }

    #[test]
    fn xpath_selects_a_node_value() {
        let ctx = Context::with_response(
            "<files><file><url>https://example.com/z.png</url></file></files>",
        );
        assert_eq!(
            expand("{xml:/files/file[1]/url}", &ctx, &NoPrompts).unwrap(),
            "https://example.com/z.png"
        );
    }

    #[test]
    fn malformed_xml_yields_empty() {
        let ctx = Context::with_response("<files><unclosed>");
        assert_eq!(expand("{xml:/files}", &ctx, &NoPrompts).unwrap(), "");
    }

    #[test]
    fn base64_encodes_its_argument() {
        assert_eq!(run("{base64:user:pass}"), "dXNlcjpwYXNz");
    }

    #[test]
    fn base64_is_how_basic_auth_headers_are_written() {
        assert_eq!(run("Basic {base64:user:pass}"), "Basic dXNlcjpwYXNz");
    }

    #[test]
    fn random_picks_one_of_its_options() {
        let options = ["a", "b", "c"];
        let picked = run("{random:a|b|c}");
        assert!(options.contains(&picked.as_str()));
    }

    #[test]
    fn escaped_braces_and_pipes_are_literal() {
        assert_eq!(run(r"\{not a function\}"), "{not a function}");
        assert_eq!(run(r"a\|b"), "a|b");
        assert_eq!(run(r"back\\slash"), r"back\slash");
    }

    #[test]
    fn an_unknown_function_is_left_visible() {
        // A typo must not quietly become an empty URL.
        assert_eq!(run("{nosuchthing:x}"), "{nosuchthing:x}");
    }

    #[test]
    fn an_unterminated_brace_is_an_error() {
        let ctx = ctx();
        assert!(matches!(
            expand("{json:a.b", &ctx, &NoPrompts),
            Err(SyntaxError::Unterminated(0))
        ));
    }

    #[test]
    fn functions_nest_inside_arguments() {
        // The documented `{outputbox:Result|{json:...}}` shape.
        struct Recorder(std::cell::RefCell<Vec<String>>);
        impl Prompter for Recorder {
            fn select(&self, options: &[String]) -> Option<String> {
                options.first().cloned()
            }
            fn input(&self, _: Option<&str>, default: Option<&str>) -> Option<String> {
                default.map(str::to_string)
            }
            fn output(&self, _: Option<&str>, message: &str) {
                self.0.borrow_mut().push(message.to_string());
            }
        }

        let recorder = Recorder(Default::default());
        let out = expand("{outputbox:Result|{json:data.link}}", &ctx(), &recorder).unwrap();

        assert_eq!(out, "", "an output box contributes nothing to the URL");
        assert_eq!(
            recorder.0.borrow().as_slice(),
            ["https://example.com/image.png"]
        );
    }

    #[test]
    fn an_inputbox_uses_its_default_when_nobody_answers() {
        assert_eq!(run("{inputbox:Subdomain|i}"), "i");
        assert_eq!(run("{inputbox}"), "");
    }

    #[test]
    fn a_select_falls_back_to_its_first_option_when_unattended() {
        assert_eq!(run("{select:one|two|three}"), "one");
    }

    #[test]
    fn a_template_can_mix_several_functions() {
        let ctx = Context {
            response: r#"{"files":[{"url":"pic.png"}]}"#.to_string(),
            ..Default::default()
        };
        assert_eq!(
            expand(
                "https://{select:cdn1|cdn2}.example.com/{json:files[0].url}",
                &ctx,
                &NoPrompts
            )
            .unwrap(),
            "https://cdn1.example.com/pic.png"
        );
    }

    #[test]
    fn an_explicit_input_overrides_the_response() {
        let ctx = Context::with_response(r#"{"a":"from response"}"#);
        assert_eq!(
            expand(r#"{json:\{"a":"explicit"\}|a}"#, &ctx, &NoPrompts).unwrap(),
            "explicit"
        );
    }
}
