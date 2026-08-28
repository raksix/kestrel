# Contributing to Kestrel

Thanks for looking. Kestrel is early — the architecture is settled, most of the
surface is not, so this is a good time to shape it.

## Getting set up

```bash
npm install
npm run tauri dev
```

Rust 1.82+ and Node 20+. On Linux you also need `libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `libxdo-dev` and `patchelf`.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build
```

CI runs exactly this on macOS, Windows and Linux.

## Where code goes

| Crate | Rule |
|---|---|
| `kestrel-core` | Domain logic only. **No** UI, Tauri or platform imports — the CLI and tests depend on this staying clean. |
| `kestrel-capture` | Anything platform-specific, behind the `CaptureBackend` trait. |
| `src-tauri` | Thin shell: tray, shortcuts, IPC commands. Business logic belongs in a crate. |
| `src` | React UI. Talks to Rust only through `src/lib/ipc.ts`. |

## Adding an uploader

This is the easiest useful contribution. Implement the `Uploader` trait, add a
recorded request/response fixture, and register it. If a service only needs an
HTTP call, consider shipping a `.sxcu` custom uploader instead of Rust code —
the engine already supports ShareX's full syntax.

## Platform honesty

Never let a feature silently fail on a platform that cannot support it. Report
it through `Capabilities` so the UI can disable the control and explain why.
Wayland in particular forbids window enumeration and global key grabs by
design; that is not a bug to work around.

## Commit messages

Imperative mood, present tense. Explain *why* in the body when it is not
obvious from the diff.
