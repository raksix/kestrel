//! ShareX-compatible filename pattern expansion.
//!
//! Implements the `%`-token vocabulary documented at
//! <https://getsharex.com> (source of truth: `CodeMenuEntryFilename.cs`),
//! including the `{n}` padding/repeat argument and `%rf{path}` file argument.
//!
//! Tokens are matched longest-first so that `%mon2` wins over `%mon` over `%mo`.

use chrono::{DateTime, Datelike, Local, Timelike};
use rand::Rng;
use std::path::Path;

/// Everything a pattern may need to expand. Callers fill in what they know;
/// unknown fields expand to an empty string rather than failing.
#[derive(Debug, Clone)]
pub struct NameContext {
    pub datetime: DateTime<Local>,
    pub window_title: Option<String>,
    pub process_name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub user_name: String,
    pub login_name: String,
    pub computer_name: String,
    /// Monotonic counter owned by the caller (persisted across runs).
    pub auto_increment: u64,
    pub locale: Locale,
    /// Kestrel extension: name of the captured application.
    pub app_name: Option<String>,
    /// Kestrel extension: first line of recognised text.
    pub ocr_first_line: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    English,
    Turkish,
}

impl Default for NameContext {
    fn default() -> Self {
        Self {
            datetime: Local::now(),
            window_title: None,
            process_name: None,
            width: None,
            height: None,
            user_name: whoami_user(),
            login_name: whoami_user(),
            computer_name: whoami_host(),
            auto_increment: 0,
            locale: Locale::default(),
            app_name: None,
            ocr_first_line: None,
        }
    }
}

fn whoami_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

fn whoami_host() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_default()
}

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_TR: [&str; 12] = [
    "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
    "Kasım", "Aralık",
];
const DAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const DAYS_TR: [&str; 7] = [
    "Pazartesi",
    "Salı",
    "Çarşamba",
    "Perşembe",
    "Cuma",
    "Cumartesi",
    "Pazar",
];

const ADJECTIVES: [&str; 24] = [
    "swift", "quiet", "bright", "clever", "brave", "calm", "eager", "gentle", "jolly", "keen",
    "lively", "mighty", "noble", "proud", "rapid", "sharp", "sleek", "solid", "steady", "sunny",
    "tidy", "vivid", "warm", "wise",
];
const ANIMALS: [&str; 24] = [
    "kestrel", "falcon", "otter", "lynx", "heron", "marten", "badger", "ibex", "osprey", "raven",
    "shrike", "vole", "wren", "bison", "civet", "dingo", "egret", "gecko", "hare", "jackal",
    "koala", "lemur", "mole", "newt",
];
const EMOJI: [&str; 16] = [
    "😀", "😎", "🚀", "🌟", "🔥", "🎯", "🦅", "🌊", "🍀", "⚡", "🎨", "🧭", "🛰", "🪐", "🧊", "🌙",
];

/// Characters that are unsafe in a filename on at least one supported platform.
const INVALID_FILENAME_CHARS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Ordered longest-first so prefix tokens never shadow longer ones.
const TOKENS: &[&str] = &[
    // 10+
    "radjective",
    // 7
    "ranimal",
    // 6
    "height",
    "remoji",
    // 5
    "width",
    // 4
    "unix",
    "guid",
    "mon2",
    // 3
    "mon",
    "iAa",
    "rna",
    "uln",
    // 2
    "yy",
    "mo",
    "w2",
    "wy",
    "mi",
    "ms",
    "pm",
    "ia",
    "ib",
    "ix",
    "rn",
    "ra",
    "rx",
    "rf",
    "un",
    "cn",
    "pn",
    "app",
    "ocr",
    // 1
    "y",
    "w",
    "d",
    "h",
    "s",
    "i",
    "t",
    "n",
];

/// Expand a ShareX-style pattern into a filename component (no extension).
///
/// Unknown `%foo` sequences are left untouched so that typos stay visible
/// instead of silently vanishing — this matches ShareX's behaviour.
pub fn expand(pattern: &str, ctx: &NameContext) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let rest_start = i + 1;
        let Some(token) = match_token(&chars, rest_start) else {
            out.push('%');
            i += 1;
            continue;
        };

        let after_token = rest_start + token.chars().count();
        let (arg, after_arg) = read_brace_arg(&chars, after_token);
        out.push_str(&expand_token(token, arg.as_deref(), ctx));
        i = after_arg;
    }

    out
}

/// Expand, then strip characters that cannot appear in a filename.
pub fn expand_sanitized(pattern: &str, ctx: &NameContext) -> String {
    let expanded = expand(pattern, ctx);
    let mut s: String = expanded
        .chars()
        .filter(|c| !INVALID_FILENAME_CHARS.contains(c) && !c.is_control())
        .collect();
    // Trailing dots and spaces are illegal on Windows.
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    s
}

fn match_token(chars: &[char], start: usize) -> Option<&'static str> {
    TOKENS.iter().copied().find(|token| {
        let tl = token.chars().count();
        start + tl <= chars.len() && chars[start..start + tl].iter().copied().eq(token.chars())
    })
}

/// Read an optional `{...}` argument directly following a token.
/// Returns the argument (without braces) and the index just past it.
fn read_brace_arg(chars: &[char], at: usize) -> (Option<String>, usize) {
    if at >= chars.len() || chars[at] != '{' {
        return (None, at);
    }
    let mut j = at + 1;
    let mut buf = String::new();
    while j < chars.len() && chars[j] != '}' {
        buf.push(chars[j]);
        j += 1;
    }
    if j >= chars.len() {
        // Unterminated brace — treat as literal text, not an argument.
        return (None, at);
    }
    (Some(buf), j + 1)
}

fn expand_token(token: &str, arg: Option<&str>, ctx: &NameContext) -> String {
    let dt = &ctx.datetime;
    let n = arg.and_then(|a| a.parse::<usize>().ok());

    match token {
        // ── Window ──────────────────────────────────────────────
        "t" => ctx.window_title.clone().unwrap_or_default(),
        "pn" => ctx.process_name.clone().unwrap_or_default(),

        // ── Date and time ───────────────────────────────────────
        "y" => format!("{:04}", dt.year()),
        "yy" => format!("{:02}", dt.year() % 100),
        "mo" => format!("{:02}", dt.month()),
        "mon" => month_name(dt.month0() as usize, ctx.locale).to_string(),
        "mon2" => MONTHS_EN[dt.month0() as usize].to_string(),
        "w" => day_name(dt.weekday().num_days_from_monday() as usize, ctx.locale).to_string(),
        "w2" => DAYS_EN[dt.weekday().num_days_from_monday() as usize].to_string(),
        "wy" => format!("{}", dt.iso_week().week()),
        "d" => format!("{:02}", dt.day()),
        "h" => format!("{:02}", dt.hour()),
        "mi" => format!("{:02}", dt.minute()),
        "s" => format!("{:02}", dt.second()),
        "ms" => format!("{:03}", dt.timestamp_subsec_millis()),
        "pm" => if dt.hour() < 12 { "AM" } else { "PM" }.to_string(),
        "unix" => dt.timestamp().to_string(),

        // ── Incremental ─────────────────────────────────────────
        "i" => pad_left(&ctx.auto_increment.to_string(), n),
        "ia" => pad_left(&to_base(ctx.auto_increment, 36, false), n),
        "iAa" => pad_left(&to_base(ctx.auto_increment, 62, true), n),
        "ix" => pad_left(&to_base(ctx.auto_increment, 16, false), n),
        "ib" => {
            let base = n.unwrap_or(36).clamp(2, 62) as u64;
            to_base(ctx.auto_increment, base, true)
        }

        // ── Random ──────────────────────────────────────────────
        "rn" => random_from("0123456789", n.unwrap_or(1)),
        "ra" => random_from(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            n.unwrap_or(1),
        ),
        "rna" => random_from(
            "abcdefghijkmnpqrstuvwxyzACDEFGHJKLMNPQRSTUVWXYZ23456789",
            n.unwrap_or(1),
        ),
        "rx" => random_from("0123456789abcdef", n.unwrap_or(1)),
        "guid" => new_guid(),
        "radjective" => pick(&ADJECTIVES).to_string(),
        "ranimal" => pick(&ANIMALS).to_string(),
        "remoji" => (0..n.unwrap_or(1)).map(|_| *pick(&EMOJI)).collect(),
        "rf" => arg.map(random_line_from_file).unwrap_or_default(),

        // ── Image ───────────────────────────────────────────────
        "width" => ctx.width.map(|v| v.to_string()).unwrap_or_default(),
        "height" => ctx.height.map(|v| v.to_string()).unwrap_or_default(),

        // ── Computer ────────────────────────────────────────────
        "un" => ctx.user_name.clone(),
        "uln" => ctx.login_name.clone(),
        "cn" => ctx.computer_name.clone(),

        // ── Kestrel extensions ──────────────────────────────────
        "app" => ctx.app_name.clone().unwrap_or_default(),
        "ocr" => ctx.ocr_first_line.clone().unwrap_or_default(),

        // ── Other ───────────────────────────────────────────────
        "n" => "\n".to_string(),

        _ => String::new(),
    }
}

fn month_name(index0: usize, locale: Locale) -> &'static str {
    match locale {
        Locale::English => MONTHS_EN[index0],
        Locale::Turkish => MONTHS_TR[index0],
    }
}

fn day_name(index0: usize, locale: Locale) -> &'static str {
    match locale {
        Locale::English => DAYS_EN[index0],
        Locale::Turkish => DAYS_TR[index0],
    }
}

fn pad_left(s: &str, width: Option<usize>) -> String {
    match width {
        Some(w) if s.len() < w => format!("{}{}", "0".repeat(w - s.len()), s),
        _ => s.to_string(),
    }
}

/// Encode `value` in `base`. `mixed_case` picks the 62-symbol alphabet.
fn to_base(value: u64, base: u64, mixed_case: bool) -> String {
    const LOWER: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    const MIXED: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let alphabet = if mixed_case { MIXED } else { LOWER };
    let base = base.clamp(2, alphabet.len() as u64);

    if value == 0 {
        return "0".to_string();
    }
    let mut v = value;
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(alphabet[(v % base) as usize]);
        v /= base;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default()
}

fn random_from(alphabet: &str, count: usize) -> String {
    let chars: Vec<char> = alphabet.chars().collect();
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect()
}

fn pick<T>(items: &[T]) -> &T {
    let mut rng = rand::thread_rng();
    &items[rng.gen_range(0..items.len())]
}

fn new_guid() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn random_line_from_file(path: &str) -> String {
    let Ok(content) = std::fs::read_to_string(Path::new(path)) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }
    pick(&lines).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ctx() -> NameContext {
        NameContext {
            datetime: Local.with_ymd_and_hms(2026, 8, 29, 14, 32, 7).unwrap(),
            window_title: Some("Kestrel — docs".into()),
            process_name: Some("kestrel".into()),
            width: Some(1920),
            height: Some(1080),
            user_name: "furkan".into(),
            login_name: "furkan".into(),
            computer_name: "macbook".into(),
            auto_increment: 42,
            locale: Locale::English,
            app_name: Some("Safari".into()),
            ocr_first_line: None,
        }
    }

    #[test]
    fn expands_the_default_sharex_pattern() {
        assert_eq!(expand("%y-%mo-%d_%h-%mi-%s", &ctx()), "2026-08-29_14-32-07");
    }

    #[test]
    fn longest_token_wins_over_its_prefixes() {
        let c = ctx();
        assert_eq!(expand("%mo", &c), "08");
        assert_eq!(expand("%mon", &c), "August");
        assert_eq!(expand("%mon2", &c), "August");
        assert_eq!(expand("%y", &c), "2026");
        assert_eq!(expand("%yy", &c), "26");
    }

    #[test]
    fn increment_tokens_respect_base_and_padding() {
        let c = ctx();
        assert_eq!(expand("%i", &c), "42");
        assert_eq!(expand("%i{5}", &c), "00042");
        assert_eq!(expand("%ix", &c), "2a");
        assert_eq!(expand("%ia", &c), "16");
        assert_eq!(expand("%ib{16}", &c), "2A");
    }

    #[test]
    fn random_tokens_honour_repeat_count() {
        let c = ctx();
        assert_eq!(expand("%rn{8}", &c).len(), 8);
        assert_eq!(expand("%rx{4}", &c).len(), 4);
        assert!(expand("%rn{8}", &c).chars().all(|ch| ch.is_ascii_digit()));
        assert_eq!(expand("%guid", &c).len(), 36);
    }

    #[test]
    fn window_and_image_tokens() {
        let c = ctx();
        assert_eq!(expand("%t", &c), "Kestrel — docs");
        assert_eq!(expand("%pn", &c), "kestrel");
        assert_eq!(expand("%width x %height", &c), "1920 x 1080");
    }

    #[test]
    fn unknown_tokens_are_left_alone() {
        let c = ctx();
        assert_eq!(expand("%zz", &c), "%zz");
        assert_eq!(expand("100%", &c), "100%");
    }

    #[test]
    fn sanitize_strips_path_separators() {
        let mut c = ctx();
        c.window_title = Some("a/b:c*d?".into());
        assert_eq!(expand_sanitized("%t", &c), "abcd");
    }

    #[test]
    fn unterminated_brace_is_literal() {
        let c = ctx();
        assert_eq!(expand("%i{5", &c), "42{5");
    }

    #[test]
    fn turkish_locale_month_and_day_names() {
        let mut c = ctx();
        c.locale = Locale::Turkish;
        assert_eq!(expand("%mon", &c), "Ağustos");
        assert_eq!(expand("%mon2", &c), "August");
    }
}
