//! Directory indexing, as ShareX's "directory indexer".
//!
//! Walks a folder and writes a listing as HTML, plain text, JSON or XML. The
//! usual reason is to hand someone a readable manifest of what is in an archive
//! or a share.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Html,
    Text,
    Json,
    Xml,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub format: Format,
    /// How deep to descend. `None` means no limit.
    pub max_depth: Option<usize>,
    pub include_hidden: bool,
    pub show_sizes: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Html,
            max_depth: None,
            // Dotfiles are noise in a manifest meant for someone else.
            include_hidden: false,
            show_sizes: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
    /// Bytes. Zero for directories.
    pub size: u64,
    pub children: Vec<Entry>,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} is not a directory")]
    NotADirectory(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// Build the tree under `root`.
pub fn index(root: &Path, options: &Options) -> Result<Entry> {
    if !root.is_dir() {
        return Err(IndexError::NotADirectory(root.display().to_string()));
    }
    Ok(walk(root, options, 0))
}

fn walk(path: &Path, options: &Options, depth: usize) -> Entry {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    if !path.is_dir() {
        return Entry {
            name,
            is_directory: false,
            size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            children: Vec::new(),
        };
    }

    let too_deep = options.max_depth.is_some_and(|max| depth >= max);
    let mut children = Vec::new();

    if !too_deep {
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut paths: Vec<_> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    options.include_hidden
                        || !p
                            .file_name()
                            .map(|n| n.to_string_lossy().starts_with('.'))
                            .unwrap_or(false)
                })
                .collect();

            // Directories first, then alphabetical — the order a file manager
            // uses, and stable between runs regardless of what the filesystem
            // hands back.
            paths.sort_by(|a, b| {
                b.is_dir()
                    .cmp(&a.is_dir())
                    .then_with(|| a.file_name().cmp(&b.file_name()))
            });

            children = paths
                .iter()
                .map(|child| walk(child, options, depth + 1))
                .collect();
        }
    }

    Entry {
        name,
        is_directory: true,
        size: 0,
        children,
    }
}

/// Render the tree in the chosen format.
pub fn render(entry: &Entry, options: &Options) -> Result<String> {
    Ok(match options.format {
        Format::Json => serde_json::to_string_pretty(entry)?,
        Format::Text => {
            let mut out = String::new();
            render_text(entry, options, 0, &mut out);
            out
        }
        Format::Xml => {
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            render_xml(entry, options, 0, &mut out);
            out
        }
        Format::Html => {
            let mut out = String::from(
                "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
                 <title>Index</title>\n<style>\
                 body{font-family:system-ui,sans-serif;margin:2rem;line-height:1.5}\
                 ul{list-style:none;padding-left:1.2rem}\
                 .dir{font-weight:600}.size{color:#777;font-size:.85em;margin-left:.5em}\
                 </style>\n</head>\n<body>\n",
            );
            render_html(entry, options, &mut out);
            out.push_str("</body>\n</html>\n");
            out
        }
    })
}

fn render_text(entry: &Entry, options: &Options, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push_str(&entry.name);
    if entry.is_directory {
        out.push('/');
    } else if options.show_sizes {
        out.push_str(&format!("  ({})", human_size(entry.size)));
    }
    out.push('\n');

    for child in &entry.children {
        render_text(child, options, depth + 1, out);
    }
}

fn render_xml(entry: &Entry, options: &Options, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let name = escape_xml(&entry.name);

    if entry.is_directory {
        out.push_str(&format!("{indent}<directory name=\"{name}\">\n"));
        for child in &entry.children {
            render_xml(child, options, depth + 1, out);
        }
        out.push_str(&format!("{indent}</directory>\n"));
    } else if options.show_sizes {
        out.push_str(&format!(
            "{indent}<file name=\"{name}\" size=\"{}\" />\n",
            entry.size
        ));
    } else {
        out.push_str(&format!("{indent}<file name=\"{name}\" />\n"));
    }
}

fn render_html(entry: &Entry, options: &Options, out: &mut String) {
    out.push_str("<ul>\n<li>");
    let name = escape_html(&entry.name);

    if entry.is_directory {
        out.push_str(&format!("<span class=\"dir\">{name}/</span>"));
        for child in &entry.children {
            render_html(child, options, out);
        }
    } else {
        out.push_str(&name);
        if options.show_sizes {
            out.push_str(&format!(
                "<span class=\"size\">{}</span>",
                escape_html(&human_size(entry.size))
            ));
        }
    }
    out.push_str("</li>\n</ul>\n");
}

/// A file name is untrusted input: it can contain `<`, `&`, or a quote, and an
/// unescaped one would break the document or inject markup into it.
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_xml(value: &str) -> String {
    escape_html(value).replace('\'', "&apos;")
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own directory. Sharing one meant the tests deleted
    /// each other's fixtures halfway through, since they run in parallel.
    fn fixture(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("kestrel-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        std::fs::write(root.join(".hidden"), b"x").unwrap();
        std::fs::write(root.join("sub/b.txt"), vec![0u8; 2048]).unwrap();
        std::fs::write(root.join("sub/deeper/c.txt"), b"c").unwrap();
        root
    }

    fn names(entry: &Entry) -> Vec<String> {
        let mut found = vec![entry.name.clone()];
        for child in &entry.children {
            found.extend(names(child));
        }
        found
    }

    #[test]
    fn the_tree_covers_every_visible_file() {
        let root = fixture("covers");
        let tree = index(&root, &Options::default()).unwrap();
        let found = names(&tree);

        assert!(found.contains(&"a.txt".to_string()));
        assert!(found.contains(&"b.txt".to_string()));
        assert!(found.contains(&"c.txt".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hidden_files_are_left_out_unless_asked_for() {
        let root = fixture("hidden");

        let without = index(&root, &Options::default()).unwrap();
        assert!(!names(&without).contains(&".hidden".to_string()));

        let with = index(
            &root,
            &Options {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(names(&with).contains(&".hidden".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn depth_can_be_limited() {
        let root = fixture("depth");
        let shallow = index(
            &root,
            &Options {
                max_depth: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

        let found = names(&shallow);
        assert!(found.contains(&"sub".to_string()));
        assert!(!found.contains(&"c.txt".to_string()), "depth 2 is excluded");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn directories_come_first_and_the_order_is_stable() {
        // The filesystem returns entries in whatever order it likes; a manifest
        // that reorders between runs is useless for comparing two of them.
        let root = fixture("order");
        let first = names(&index(&root, &Options::default()).unwrap());
        for _ in 0..3 {
            assert_eq!(names(&index(&root, &Options::default()).unwrap()), first);
        }
        assert_eq!(first.get(1).map(String::as_str), Some("sub"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_is_not_a_valid_root() {
        let root = fixture("notdir");
        let file = root.join("a.txt");
        assert!(matches!(
            index(&file, &Options::default()),
            Err(IndexError::NotADirectory(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_format_renders_the_contents() {
        let root = fixture("formats");
        let tree = index(&root, &Options::default()).unwrap();

        for format in [Format::Html, Format::Text, Format::Json, Format::Xml] {
            let options = Options {
                format,
                ..Default::default()
            };
            let output = render(&tree, &options).unwrap();
            assert!(output.contains("a.txt"), "{format:?} lost a file");
            assert!(output.contains("b.txt"), "{format:?} lost a nested file");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn json_output_parses_back() {
        let root = fixture("json");
        let tree = index(&root, &Options::default()).unwrap();
        let json = render(
            &tree,
            &Options {
                format: Format::Json,
                ..Default::default()
            },
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert!(parsed.get("children").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_name_containing_markup_cannot_break_the_document() {
        // File names are untrusted input, and an unescaped one would either
        // corrupt the output or inject markup into it.
        let entry = Entry {
            name: "<script>alert('x')</script> & \"quoted\".txt".into(),
            is_directory: false,
            size: 10,
            children: Vec::new(),
        };

        let html = render(
            &entry,
            &Options {
                format: Format::Html,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));

        let xml = render(
            &entry,
            &Options {
                format: Format::Xml,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!xml.contains("<script>"));
        assert!(xml.contains("&quot;"));
    }

    #[test]
    fn sizes_are_shown_in_units_people_read() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn sizes_can_be_left_out() {
        let entry = Entry {
            name: "a.txt".into(),
            is_directory: false,
            size: 2048,
            children: Vec::new(),
        };
        let options = Options {
            format: Format::Text,
            show_sizes: false,
            ..Default::default()
        };
        assert!(!render(&entry, &options).unwrap().contains("KB"));
    }
}
