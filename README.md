# Kestrel

Cross-platform capture, annotation and sharing — a ShareX-class tool for macOS, Windows and Linux.

> **Status: early development (phase 1).** The capture core, filename engine and
> app shell work. The annotation editor, uploaders and recording are on the
> roadmap below. Not ready for daily use yet.

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

- Full-screen, per-display and window capture on macOS, Windows, X11 and Wayland
- **Region selection overlay** — drag to select, or click a window to snap to it,
  with live size readout, crosshair and keyboard nudging
- **Window / display picker** with live thumbnails, keyboard navigable
- Multi-display capture with correct mixed-DPI compositing
- **Editable global shortcuts**, with conflict detection and a report of which
  bindings the OS actually accepted
- ShareX-compatible filename patterns (all 37 tokens) with a live preview
- Screen-recording permission detection and guided setup on macOS
- System tray menu, save to disk, copy to clipboard
- Settings persisted as readable JSON in the platform config directory

Selections are cropped from a snapshot taken *before* the overlay appears, so the
overlay's own dimming can never end up in the capture.

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 0 | Project shell, design system, CI, settings, tray | ✅ |
| 1 | Capture backends, region overlay, shortcuts | ✅ |
| 2 | Annotation editor, pin to screen, post-capture card | ⏳ |
| 3 | Uploaders, `.sxcu` engine, workflows, history | ⏳ |
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

Run the Rust test suite:

```bash
cargo test
```

Linux also needs the usual Tauri system dependencies (`libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`).

On macOS the app needs the **Screen Recording** permission. Without it macOS does
not return an error — it silently hands back a wallpaper image and an empty
window list. Kestrel detects this and says so instead of appearing broken.

## Architecture

```
crates/kestrel-core      domain model, workflows, filename tokens (no UI, no Tauri)
crates/kestrel-capture   platform capture backends behind one trait
src-tauri                thin desktop shell: tray, shortcuts, overlay, IPC
src                      React UI: main window, selection overlay, picker
```

`kestrel-core` never imports a UI framework, so the CLI and the test suite use
the same logic the app does.

## Contributing

Issues and pull requests are welcome. Adding an uploader is intentionally the
easiest contribution: implement one trait, add a request/response fixture.

## License

[GPL-3.0](LICENSE), the same spirit as ShareX.

Kestrel is an independent project. It is not affiliated with or endorsed by the
ShareX project; "ShareX" is referenced only to describe compatibility.
