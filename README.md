# Kestrel

Cross-platform capture, annotation and sharing — a ShareX-class tool for macOS, Windows and Linux.

> **Status: early development.**
> Capture, annotation, image effects, recording, OCR and `.sxcu` uploading
> work. Most of the standalone tools and the phase 6 integrations do not exist
> yet — see the roadmap.

## Why

[ShareX](https://github.com/ShareX/ShareX) is the most capable screenshot tool ever
built, and it is Windows-only — it is ~250k lines of C#/WinForms wired directly
into GDI+, Win32 hooks and the registry. There is no portable core to lift out.

Kestrel is not a port of that code. It is a port of its *behaviour*: the same
after-capture task chain, the same workflow model, the same `%y-%mo-%d` filename
tokens, and the same `.sxcu` custom-uploader format — reimplemented in Rust so it
runs everywhere.

Concretely, that last point matters more than it sounds. ShareX's custom uploader
system is a small language for describing any HTTP upload endpoint, and the
community has published hundreds of ready-made `.sxcu` files. Kestrel implements
that language exactly, so those files work unmodified.

## What it does today

**Capture**
- Full-screen, per-display and window capture on macOS, Windows, X11 and Wayland
- Region overlay: drag to select, or click a window to snap to it, with a live
  size readout, crosshair and keyboard nudging
- Draw on the overlay before capturing — rectangle, ellipse, arrow, line,
  freehand, step numbers, highlight, blur and pixelate
- Window and display picker with live thumbnails, keyboard navigable
- Multi-display capture with correct mixed-DPI compositing

**Annotate**
- Editor with twelve tools and ShareX's keyboard letters, non-destructive so a
  document can be reopened and re-edited
- Text with a system font and an outline, multi-line, typed into a real
  textarea so input methods and screen readers keep working
- Crop, padding, rounded corners, drop shadow, and solid or gradient
  backgrounds — ShareX's crop tool and image beautifier
- An ordered image effect chain: resize, rotate, flip, auto-crop, brightness,
  contrast, gamma, saturation, opacity, greyscale, sepia, invert, blur,
  sharpen, pixelate and border. The order is part of the picture, so the list
  is reorderable, and the chain always applies to the untouched original —
  removing an effect really undoes it
- ShareX `.sxie` effect presets import — best-effort, because unlike `.sxcu`
  that format is not documented, so the importer names anything it could not
  map rather than applying a partial preset silently
- Undo and redo across annotations and framing alike
- Pin a capture above every other window, with ShareX's keys: drag to move,
  wheel to scale, modifier+wheel for opacity, right click to close

**Record**
- Screen recording to H.264, HEVC, VP9 or AV1, and animated GIF with a
  per-clip palette
- Pause, resume and cancel, with a live duration that excludes paused time
- Convert a video to MP4, WebM, MKV, GIF or MP3, with optional rescaling and
  frame-rate change — the result is written beside the source, never over it
- Pull a single frame out of a video as a thumbnail
- Needs ffmpeg; if it is missing the app says so and gives the install command
  for the platform

**Share**
- ShareX `.sxcu` custom uploaders work unmodified: all thirteen template
  functions, every body type, every request method
- Drag a `.sxcu` onto the window to import it
- Multipart, form-urlencoded, JSON, XML and binary uploads

**Tools**
- QR codes: generate one, or read every code out of a capture with its position
- Hash checking with MD5, SHA-1, SHA-256 and SHA-512 in a single pass, and a
  paste-tolerant comparison
- Image analysis: size, colour count, transparency and dominant colours
- Metadata viewer that flags what identifies a person, place or device, and a
  stripper that writes a clean copy rather than touching the original
- Directory indexing to HTML, text, JSON or XML
- Colour picker: read a pixel from a capture — or an area average, for
  anti-aliased text — in hex, RGB, HSL, HSV and CMYK at once
- Image comparer: how much differs, where it differs, and a diff picture with
  the original still visible underneath
- OCR: read the text out of a capture and search for it later. Recognition
  runs locally, so nothing is sent anywhere. The ~20 MB models are downloaded
  on first use, and only after you say so

**Organise**
- Capture history in SQLite, searchable by filename, window title, URL and
  recognised text
- Library grouped by day, with copy URL, open, copy path and remove
- Editable global shortcuts with conflict detection and a report of which
  bindings the OS actually accepted
- ShareX-compatible filename patterns (all 37 tokens) with a live preview
- Settings persisted as readable JSON in the platform config directory

Selections are cropped from a snapshot taken *before* the overlay appears, so
the overlay's own dimming can never end up in the capture. The exported file is
rendered in Rust, not the webview, so it looks the same on every platform and is
not capped at screen resolution.

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 0 | Project shell, design system, CI, settings, tray | ✅ |
| 1 | Capture backends, region overlay, shortcuts | ✅ |
| 2 | Annotation editor, framing, pin to screen, post-capture card | ✅ |
| 3 | Uploaders, `.sxcu` engine, history, destinations | 🚧 workflow editor left |
| 4 | Screen recording, GIF, video tools | 🚧 audio capture left |
| 5 | The remaining tools, effect chain, OCR | 🚧 effects and OCR done, eleven tools done |
| 6 | CLI, integrations, scrolling capture, 1.0 | ⏳ |

Full plans: [`docs/00-PLAN.md`](docs/00-PLAN.md) ·
[`docs/01-FEATURE-PARITY.md`](docs/01-FEATURE-PARITY.md) ·
[`docs/02-DESIGN.md`](docs/02-DESIGN.md) ·
[`docs/03-SHAREX-FEATURES.md`](docs/03-SHAREX-FEATURES.md) — the itemised ShareX
feature backlog, every capability with status and target phase

## Platform support

Kestrel is honest about what each platform allows. The app reads its own
capabilities at runtime and disables — with an explanation — anything the
session cannot do.

| | macOS | Windows | Linux/X11 | Linux/Wayland |
|---|:-:|:-:|:-:|:-:|
| Screen capture | ✅ | ✅ | ✅ | ✅ (portal) |
| Window capture | ✅ | ✅ | ✅ | ❌ |
| Global shortcuts | ✅ | ✅ | ✅ | ⚠️ DE-dependent |
| Scrolling capture | ⚠️ | ✅ | ⚠️ | ❌ |

Wayland's restrictions are deliberate parts of the protocol, not bugs we can
work around.

## Building

Requires [Rust](https://rustup.rs) 1.89+ and Node.js 22 (see `.nvmrc`).

```bash
npm install
npm run tauri dev
```

Run the test suite:

```bash
cargo test --workspace
npm run build
```

The dev profile builds dependencies at `opt-level = 3` and disables incremental
compilation. Without the first, a single Retina screenshot takes seconds to
process; without the second, incremental and optimisation together produce
stale symbols and an intermittent link failure.

Linux also needs the usual Tauri system dependencies (`libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`).

On macOS the app needs the **Screen Recording** permission. Without it macOS does
not return an error — it silently hands back a wallpaper image and an empty
window list. Kestrel detects this and says so instead of appearing broken.

## Architecture

```
crates/kestrel-core      domain model, workflows, filename tokens (no UI, no Tauri)
crates/kestrel-capture   platform capture backends behind one trait
crates/kestrel-editor    annotation model, effects, history, export renderer
crates/kestrel-record    ffmpeg discovery and the recording frame pump
crates/kestrel-tools     QR, hashing, analysis, metadata, indexing, OCR
crates/kestrel-upload    .sxcu template engine, file format, HTTP transport
src-tauri                desktop shell: tray, shortcuts, windows, history, IPC
src                      React UI: main window, overlay, picker, editor
```

`kestrel-core` never imports a UI framework, so the CLI and the test suite use
the same logic the app does.

## Contributing

Issues and pull requests are welcome. The easiest useful contribution is not
Rust at all: a `.sxcu` file for a service Kestrel does not cover yet already
works, because the format is implemented exactly.

## License

[GPL-3.0](LICENSE), the same spirit as ShareX.

Kestrel is an independent project. It is not affiliated with or endorsed by the
ShareX project; "ShareX" is referenced only to describe compatibility.
