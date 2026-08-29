//! End-to-end tests against the built binary.
//!
//! The CLI's contract is not the return value of a function — it is what lands
//! on stdout, what lands on stderr, and the exit code. Testing the functions
//! directly would miss all three, and those are what a shell script depends on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> PathBuf {
    // The integration test binary sits next to the one under test.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("kestrel")
}

fn dir(name: &str) -> PathBuf {
    // Per-test names: a shared directory means parallel tests delete each
    // other's fixtures.
    let dir = std::env::temp_dir().join(format!("kestrel-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A small image with a known pixel, so colour and comparison have something
/// definite to find.
fn fixture(dir: &Path, name: &str, colour: [u8; 4]) -> PathBuf {
    let path = dir.join(name);
    image::RgbaImage::from_pixel(8, 8, image::Rgba(colour))
        .save(&path)
        .expect("write fixture");
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("cli runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn hashing_prints_every_algorithm() {
    let dir = dir("hash");
    let file = dir.join("data.bin");
    std::fs::write(&file, b"kestrel").unwrap();

    let output = run(&["hash", file.to_str().unwrap()]);
    let text = stdout(&output);

    assert!(output.status.success());
    for algorithm in ["Md5", "Sha1", "Sha256", "Sha512"] {
        assert!(text.contains(algorithm), "{text}");
    }
}

#[test]
fn a_matching_hash_exits_zero_and_a_mismatch_exits_non_zero() {
    // This is what makes `kestrel hash file --expect abc && deploy` mean
    // something; printing "no match" and exiting 0 would silently pass.
    let dir = dir("expect");
    let file = dir.join("data.bin");
    std::fs::write(&file, b"kestrel").unwrap();

    let listed = stdout(&run(&["hash", file.to_str().unwrap()]));
    let sha256 = listed
        .lines()
        .find(|line| line.starts_with("Sha256"))
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("a sha256 line")
        .to_string();

    let matched = run(&["hash", file.to_str().unwrap(), "--expect", &sha256]);
    let mismatched = run(&["hash", file.to_str().unwrap(), "--expect", "deadbeef"]);

    assert!(matched.status.success(), "{}", stdout(&matched));
    assert!(!mismatched.status.success());
}

#[test]
fn a_pasted_hash_with_whitespace_and_capitals_still_matches() {
    // A published checksum is usually copied with surrounding whitespace, and
    // often in the other case.
    let dir = dir("paste");
    let file = dir.join("data.bin");
    std::fs::write(&file, b"kestrel").unwrap();

    let listed = stdout(&run(&["hash", file.to_str().unwrap()]));
    let sha256 = listed
        .lines()
        .find(|line| line.starts_with("Sha256"))
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("a sha256 line")
        .to_uppercase();

    let output = run(&[
        "hash",
        file.to_str().unwrap(),
        "--expect",
        &format!("  {sha256}  "),
    ]);

    assert!(output.status.success());
}

#[test]
fn identical_images_exit_zero_and_different_ones_do_not() {
    let dir = dir("compare");
    let a = fixture(&dir, "a.png", [10, 20, 30, 255]);
    let b = fixture(&dir, "b.png", [10, 20, 30, 255]);
    let c = fixture(&dir, "c.png", [200, 20, 30, 255]);

    let same = run(&["compare", a.to_str().unwrap(), b.to_str().unwrap()]);
    let different = run(&["compare", a.to_str().unwrap(), c.to_str().unwrap()]);

    assert!(same.status.success());
    assert!(stdout(&same).contains("identical"));
    assert!(!different.status.success());
}

#[test]
fn a_diff_image_is_written_when_asked_for() {
    let dir = dir("diff");
    let a = fixture(&dir, "a.png", [10, 20, 30, 255]);
    let b = fixture(&dir, "b.png", [200, 20, 30, 255]);
    let diff = dir.join("diff.png");

    run(&[
        "compare",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--diff",
        diff.to_str().unwrap(),
    ]);

    assert!(diff.is_file());
    assert_eq!(image::open(&diff).unwrap().width(), 8);
}

#[test]
fn a_colour_is_reported_in_every_notation() {
    let dir = dir("color");
    let file = fixture(&dir, "red.png", [255, 0, 0, 255]);

    let text = stdout(&run(&["color", file.to_str().unwrap(), "1", "1"]));

    assert!(text.contains("#FF0000"), "{text}");
    for notation in ["rgb", "hsl", "hsv", "cmyk"] {
        assert!(text.contains(notation), "{text}");
    }
}

#[test]
fn a_point_outside_the_image_is_an_error_on_stderr() {
    // Reporting a different pixel's colour would be worse than failing.
    let dir = dir("outside");
    let file = fixture(&dir, "small.png", [0, 0, 0, 255]);

    let output = run(&["color", file.to_str().unwrap(), "99", "99"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("outside"), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "errors must not go to stdout");
}

#[test]
fn json_output_is_parseable_on_its_own() {
    // The whole point of --json is piping it somewhere; a stray log line would
    // break that.
    let dir = dir("json");
    let file = fixture(&dir, "img.png", [1, 2, 3, 255]);

    let output = run(&["analyze", file.to_str().unwrap(), "--json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout should be valid json alone");

    assert_eq!(parsed["width"], 8);
    assert_eq!(parsed["height"], 8);
}

#[test]
fn a_missing_file_fails_with_the_path_in_the_message() {
    let output = run(&["analyze", "/definitely/not/here.png"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("/definitely/not/here.png"),
        "the message should name the file: {}",
        stderr(&output)
    );
}

#[test]
fn an_image_with_no_qr_code_is_not_an_error() {
    // "Nothing found" is a perfectly good answer to "what codes are in this".
    let dir = dir("qr");
    let file = fixture(&dir, "blank.png", [255, 255, 255, 255]);

    let output = run(&["qr", file.to_str().unwrap()]);

    assert!(output.status.success());
    assert!(stdout(&output).contains("no codes"));
}

#[test]
fn a_filename_pattern_expands_to_something_usable_as_a_filename() {
    let output = run(&["name", "%y-%mo-%d_%pn"]);
    let text = stdout(&output).trim().to_string();

    assert!(output.status.success());
    assert!(!text.contains('%'), "every token should expand: {text}");
    assert!(
        !text.contains('/') && !text.contains('\\'),
        "the result is sanitised for use as a filename: {text}"
    );
}

#[test]
fn indexing_a_directory_lists_what_is_in_it() {
    let dir = dir("index");
    std::fs::write(dir.join("one.txt"), b"a").unwrap();
    std::fs::write(dir.join("two.txt"), b"b").unwrap();

    let output = run(&["index", dir.to_str().unwrap(), "--format", "text"]);
    let text = stdout(&output);

    assert!(output.status.success());
    assert!(text.contains("one.txt"), "{text}");
    assert!(text.contains("two.txt"), "{text}");
}

#[test]
fn indexing_something_that_is_not_a_directory_fails() {
    let dir = dir("notdir");
    let file = dir.join("file.txt");
    std::fs::write(&file, b"a").unwrap();

    let output = run(&["index", file.to_str().unwrap()]);

    assert!(!output.status.success());
}

#[test]
fn an_sxie_preset_reports_what_it_could_not_import() {
    // The .sxie schema is inferred, so a preset can legitimately contain
    // effects Kestrel has no equivalent for. Saying so is the point.
    let dir = dir("sxie");
    let file = dir.join("preset.sxie");
    std::fs::write(
        &file,
        r#"{"Name":"Test","Effects":[
            {"$type":"ShareX.ImageEffectsLib.Grayscale, ShareX.ImageEffectsLib"},
            {"$type":"ShareX.ImageEffectsLib.Particles, ShareX.ImageEffectsLib"}
        ]}"#,
    )
    .unwrap();

    let output = run(&["sxie", file.to_str().unwrap()]);

    assert!(output.status.success());
    assert!(stdout(&output).contains("Grayscale"));
    assert!(
        stderr(&output).contains("Particles"),
        "the warning belongs on stderr so a piped list stays clean: {}",
        stderr(&output)
    );
}

#[test]
fn an_invalid_sxcu_fails_rather_than_reporting_an_empty_uploader() {
    let dir = dir("sxcu");
    let file = dir.join("broken.sxcu");
    std::fs::write(&file, "{ not json").unwrap();

    assert!(!run(&["sxcu", file.to_str().unwrap()]).status.success());
}

#[test]
fn no_subcommand_prints_usage_and_fails() {
    // Exiting zero on "you did not tell me what to do" would make a typo in a
    // script look like a success.
    let output = run(&[]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("Usage"), "{}", stderr(&output));
}
