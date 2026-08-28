import { useCallback, useEffect, useMemo, useState } from "react";
import {
  capture,
  formatShortcut,
  listWorkflows,
  onCaptureComplete,
  onCaptureFailed,
  platformCapabilities,
  previewFilename,
  type Capabilities,
  type CaptureOutput,
  type Workflow,
} from "./lib/ipc";

type Section = "capture" | "workflows" | "library";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "capture", label: "Yakala" },
  { id: "workflows", label: "Workflow'lar" },
  { id: "library", label: "Kütüphane" },
];

/**
 * Which capability a workflow depends on. Used to disable — and explain —
 * actions the current platform cannot perform, instead of failing on click.
 */
function unavailableReason(
  workflow: Workflow,
  capabilities: Capabilities | null,
): string | null {
  if (!capabilities) return null;
  if (
    (workflow.method === "active_window" || workflow.method === "window_menu") &&
    !capabilities.window_capture
  ) {
    return "Bu oturumda pencere yakalama yok (Wayland pencere listesi vermiyor).";
  }
  if (workflow.method === "scrolling_capture" && !capabilities.scrolling_capture) {
    return "Kaydırmalı yakalama bu platformda henüz desteklenmiyor.";
  }
  return null;
}

export default function App() {
  const [section, setSection] = useState<Section>("capture");
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  const [latest, setLatest] = useState<CaptureOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    listWorkflows().then(setWorkflows).catch((e) => setError(String(e)));
    platformCapabilities().then(setCapabilities).catch((e) => setError(String(e)));
  }, []);

  // Captures triggered from the tray or a global shortcut arrive as events,
  // so the window shows them even though it did not start them.
  useEffect(() => {
    const unlisteners = [
      onCaptureComplete((output) => {
        setLatest(output);
        setError(null);
        setBusy(false);
      }),
      onCaptureFailed((message) => {
        setError(message);
        setBusy(false);
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => undefined));
    };
  }, []);

  const runWorkflow = useCallback(async (workflow: Workflow) => {
    setBusy(true);
    setError(null);
    try {
      setLatest(await capture(workflow.method));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Ana gezinme">
        <div className="sidebar__brand">Kestrel</div>
        <div>
          <div className="sidebar__group-label" id="nav-group">
            Bölümler
          </div>
          <div role="list" aria-labelledby="nav-group">
            {SECTIONS.map((item) => (
              <button
                key={item.id}
                type="button"
                role="listitem"
                className="nav-item"
                aria-current={section === item.id ? "page" : undefined}
                onClick={() => setSection(item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>
        <PlatformSummary capabilities={capabilities} />
      </nav>

      <main className="content">
        <header className="toolbar">
          <h1 className="toolbar__title">
            {SECTIONS.find((s) => s.id === section)?.label}
          </h1>
          <div className="toolbar__spacer" />
          {busy && <span className="muted">Yakalanıyor…</span>}
        </header>

        <div className="panel">
          {error && (
            <p className="status status--error" role="alert">
              <span className="dot" aria-hidden="true" />
              {error}
            </p>
          )}

          {section === "capture" && (
            <CapturePanel
              workflows={workflows}
              capabilities={capabilities}
              busy={busy}
              onRun={runWorkflow}
            />
          )}
          {section === "workflows" && <WorkflowPanel workflows={workflows} />}
          {section === "library" && <LibraryPanel latest={latest} />}
        </div>
      </main>

      {latest && <CaptureCard output={latest} onDismiss={() => setLatest(null)} />}
    </div>
  );
}

function CapturePanel({
  workflows,
  capabilities,
  busy,
  onRun,
}: {
  workflows: Workflow[];
  capabilities: Capabilities | null;
  busy: boolean;
  onRun: (workflow: Workflow) => void;
}) {
  if (workflows.length === 0) {
    return <p className="muted">Workflow yükleniyor…</p>;
  }

  return (
    <div className="stack">
      <div className="grid">
        {workflows.map((workflow) => {
          const reason = unavailableReason(workflow, capabilities);
          return (
            <button
              key={workflow.id}
              type="button"
              className="action-tile"
              disabled={busy || reason !== null}
              title={reason ?? undefined}
              onClick={() => onRun(workflow)}
            >
              <span className="action-tile__name">{workflow.name}</span>
              <span className="action-tile__meta">
                {workflow.shortcut && (
                  <kbd className="kbd">{formatShortcut(workflow.shortcut)}</kbd>
                )}
                <span>{reason ?? workflow.method.replace(/_/g, " ")}</span>
              </span>
            </button>
          );
        })}
      </div>
      <FilenamePlayground />
    </div>
  );
}

/**
 * The live filename preview from docs/02-DESIGN.md §3.6 — the thing that makes
 * ShareX's `%y-%mo-%d` token vocabulary learnable instead of guesswork.
 */
function FilenamePlayground() {
  const [pattern, setPattern] = useState("%y-%mo-%d_%h-%mi-%s");
  const [preview, setPreview] = useState("");

  useEffect(() => {
    let cancelled = false;
    previewFilename(pattern)
      .then((value) => {
        if (!cancelled) setPreview(value);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [pattern]);

  return (
    <section className="card">
      <h2 className="card__title">Dosya adı deseni</h2>
      <p className="card__hint">
        ShareX token'ları birebir desteklenir. Değiştirdikçe sonuç aşağıda güncellenir.
      </p>
      <div className="stack" style={{ marginTop: "var(--space-3)" }}>
        <label>
          <span className="visually-hidden">Dosya adı deseni</span>
          <input
            className="input input--mono"
            value={pattern}
            spellCheck={false}
            onChange={(event) => setPattern(event.target.value)}
          />
        </label>
        <p className="mono muted" aria-live="polite">
          {preview ? `${preview}.png` : "—"}
        </p>
      </div>
    </section>
  );
}

function WorkflowPanel({ workflows }: { workflows: Workflow[] }) {
  return (
    <div className="stack">
      {workflows.map((workflow) => (
        <section key={workflow.id} className="card">
          <div className="row">
            <h2 className="card__title" style={{ margin: 0 }}>
              {workflow.name}
            </h2>
            <div className="toolbar__spacer" />
            {workflow.shortcut && (
              <kbd className="kbd">{formatShortcut(workflow.shortcut)}</kbd>
            )}
          </div>
          <p className="card__hint">
            {workflow.method.replace(/_/g, " ")} · {workflow.settings.filename_pattern}
          </p>
        </section>
      ))}
    </div>
  );
}

function LibraryPanel({ latest }: { latest: CaptureOutput | null }) {
  if (!latest) {
    return (
      <div className="card">
        <h2 className="card__title">Henüz yakalama yok</h2>
        <p className="card__hint">
          Yakala bölümünden bir işlem seç ya da global kısayolu kullan.
        </p>
      </div>
    );
  }

  return (
    <div className="grid">
      <figure className="card" style={{ margin: 0 }}>
        <img
          src={latest.preview}
          alt="Son yakalama önizlemesi"
          style={{ width: "100%", borderRadius: "var(--radius)", display: "block" }}
        />
        <figcaption className="card__hint" style={{ marginTop: "var(--space-2)" }}>
          {latest.width} × {latest.height}
          {latest.path ? ` · ${latest.path}` : ""}
        </figcaption>
      </figure>
    </div>
  );
}

/**
 * The post-capture floating card from docs/02-DESIGN.md §3.3.
 */
function CaptureCard({
  output,
  onDismiss,
}: {
  output: CaptureOutput;
  onDismiss: () => void;
}) {
  const filename = useMemo(
    () => output.path?.split(/[\\/]/).pop() ?? "Kaydedilmedi",
    [output.path],
  );

  return (
    <aside className="capture-card" aria-label="Son yakalama">
      <img className="capture-card__image" src={output.preview} alt="" />
      <div className="capture-card__body">
        <div className="row">
          <span className="capture-card__meta" title={output.path ?? undefined}>
            {filename}
          </span>
          <div className="toolbar__spacer" />
          <button
            type="button"
            className="button"
            style={{ minHeight: 22, padding: "0 var(--space-2)" }}
            onClick={onDismiss}
          >
            Kapat
          </button>
        </div>
        <span className="capture-card__meta">
          {output.width} × {output.height}
          {output.copiedToClipboard ? " · panoya kopyalandı" : ""}
        </span>
      </div>
    </aside>
  );
}

function PlatformSummary({ capabilities }: { capabilities: Capabilities | null }) {
  if (!capabilities) return null;

  const rows: [string, boolean][] = [
    ["Pencere yakalama", capabilities.window_capture],
    ["Global kısayol", capabilities.global_shortcuts],
    ["Kaydırmalı yakalama", capabilities.scrolling_capture],
  ];

  return (
    <div style={{ marginTop: "auto" }}>
      <div className="sidebar__group-label">Bu platformda</div>
      <ul style={{ listStyle: "none", margin: 0, padding: "0 var(--space-2)" }}>
        {rows.map(([label, supported]) => (
          <li
            key={label}
            className="row"
            style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}
          >
            <span
              className="dot"
              style={{ color: supported ? "var(--success)" : "var(--text-muted)" }}
              aria-hidden="true"
            />
            <span>{label}</span>
            <span className="visually-hidden">
              {supported ? "destekleniyor" : "desteklenmiyor"}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
