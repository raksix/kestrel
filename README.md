# Kestrel

Cross-platform capture, annotation and sharing — a ShareX-class tool for macOS, Windows and Linux.

> **Status: early development.** Capture, annotation and `.sxcu` uploading work.
> Screen recording, OCR and most of the standalone tools do not exist yet — see
> the roadmap. Usable for taking and marking up screenshots; not yet a
> replacement for ShareX.

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
- Undo and redo across annotations and framing alike
- Pin a capture above every other window, with ShareX's keys: drag to move,
  wheel to scale, modifier+wheel for opacity, right click to close

**Share**
- ShareX `.sxcu` custom uploaders work unmodified: all thirteen template
  functions, every body type, every request method
- Drag a `.sxcu` onto the window to import it
- Multipart, form-urlencoded, JSON, XML and binary uploads

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
| 4 | Screen recording, GIF, video tools | ⏳ |
| 5 | The remaining 24 tools, effect chain, OCR | ⏳ |
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

Requires [Rust](https://rustup.rs) 1.82+ and Node.js 22 (see `.nvmrc`).

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
crates/kestrel-editor    annotation model, history, and the export renderer
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
