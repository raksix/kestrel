import { useCallback, useEffect, useMemo, useState } from "react";
import Destinations from "./features/destinations/Destinations";
import Library from "./features/library/Library";
import PermissionGate from "./features/settings/PermissionGate";
import ShortcutSettings from "./features/settings/ShortcutSettings";
import { openEditor } from "./lib/editorTypes";
import { pinLastCapture } from "./lib/ipc";
import RecordingBar from "./features/record/RecordingBar";
import {
  formatShortcut,
  getSettings,
  listWorkflows,
  onCaptureComplete,
  onCaptureFailed,
  platformCapabilities,
  previewFilename,
  runWorkflow,
  setFilenamePattern,
  type Capabilities,
  type CaptureOutput,
  type Workflow,
} from "./lib/ipc";

type Section = "capture" | "shortcuts" | "library" | "destinations";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "capture", label: "Yakala" },
  { id: "shortcuts", label: "Kısayollar" },
  { id: "library", label: "Kütüphane" },
  { id: "destinations", label: "Hedefler" },
];

/** Human wording for each capture method, used as tile subtitles. */
const METHOD_LABEL: Record<string, string> = {
  region: "Sürükleyerek bölge seç",
  region_light: "Sade bölge seçimi",
  region_transparent: "Şeffaf bölge seçimi",
  fullscreen: "Tüm ekranlar tek görüntüde",
  active_monitor: "İmlecin bulunduğu ekran",
  active_window: "Öndeki pencere, seçmeden",
  window_menu: "Pencere listesinden seç",
  monitor_menu: "Ekran listesinden seç",
  screen_recording: "Video kaydı",
  screen_recording_gif: "GIF kaydı",
  scrolling_capture: "Kaydırarak uzun sayfa",
  last_region: "Son kullanılan bölge",
  custom_region: "Kayıtlı sabit bölge",
  auto_capture: "Zamanlayıcıyla otomatik",
};

/** Features that exist in the model but are not built yet (see docs/00-PLAN.md). */
const NOT_YET_BUILT = new Set([
  "scrolling_capture",
  "last_region",
  "custom_region",
  "auto_capture",
]);

function unavailableReason(
  workflow: Workflow,
  capabilities: Capabilities | null,
): string | null {
  if (NOT_YET_BUILT.has(workflow.method)) {
    return "Henüz hazır değil";
  }
  if (!capabilities) return null;
  if (!capabilities.screenPermission || capabilities.screenPermission === "denied") {
    return "Ekran Kaydı izni gerekiyor";
  }
  if (
    (workflow.method === "active_window" || workflow.method === "window_menu") &&
    !capabilities.windowCapture
  ) {
    return "Bu oturumda pencere yakalama yok";
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

  const refresh = useCallback(() => {
    listWorkflows().then(setWorkflows).catch((e) => setError(String(e)));
    platformCapabilities().then(setCapabilities).catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  // Captures triggered from the tray, a shortcut or the overlay arrive as
  // events, so this window reflects them even though it did not start them.
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

  const run = useCallback(async (workflow: Workflow) => {
    setBusy(true);
    setError(null);
    try {
      // Interactive workflows (overlay, picker) return null and finish later
      // through the capture-complete event.
      const output = await runWorkflow(workflow.id);
      if (output) setLatest(output);
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
          <h1 className="toolbar__title">{SECTIONS.find((s) => s.id === section)?.label}</h1>
          <div className="toolbar__spacer" />
          {busy && <span className="muted">Yakalanıyor…</span>}
        </header>

        <div className="panel">
          <div className="stack">
            <PermissionGate onGranted={refresh} />

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
                onRun={run}
              />
            )}
            {section === "shortcuts" && (
              <ShortcutSettings workflows={workflows} onWorkflowsChanged={setWorkflows} />
            )}
            {section === "library" && <Library />}
            {section === "destinations" && <Destinations />}
          </div>
        </div>
      </main>

      <RecordingBar />
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
                <span>{reason ?? METHOD_LABEL[workflow.method] ?? workflow.method}</span>
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
 * The live filename preview from docs/02-DESIGN.md §3.6 — what makes ShareX's
 * `%y-%mo-%d` token vocabulary learnable instead of guesswork.
 */
function FilenamePlayground() {
  const [pattern, setPattern] = useState("%y-%mo-%d_%h-%mi-%s");
  const [preview, setPreview] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    getSettings()
      .then((settings) => setPattern(settings.defaults.filename_pattern))
      .catch(() => undefined);
  }, []);

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
        ShareX token'ları birebir desteklenir: %y yıl, %mo ay, %d gün, %h saat, %i artan
        sayı, %ra rastgele karakter, %t pencere başlığı.
      </p>
      <div className="stack" style={{ marginTop: "var(--space-3)" }}>
        <label>
          <span className="visually-hidden">Dosya adı deseni</span>
          <input
            className="input input--mono"
            value={pattern}
            spellCheck={false}
            onChange={(event) => {
              setPattern(event.target.value);
              setSaved(false);
            }}
          />
        </label>
        <div className="row">
          <p className="mono muted" aria-live="polite" style={{ margin: 0, flex: 1 }}>
            {preview ? `${preview}.png` : "—"}
          </p>
          <button
            type="button"
            className="button"
            onClick={async () => {
              await setFilenamePattern(pattern);
              setSaved(true);
            }}
          >
            {saved ? "Kaydedildi" : "Kaydet"}
          </button>
        </div>
      </div>
    </section>
  );
}

/** The post-capture floating card from docs/02-DESIGN.md §3.3. */
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
            onClick={() => void openEditor()}
          >
            Düzenle
          </button>
          <button
            type="button"
            className="button"
            style={{ minHeight: 22, padding: "0 var(--space-2)" }}
            onClick={() => void pinLastCapture()}
          >
            Sabitle
          </button>
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
    ["Ekran izni", capabilities.screenPermission !== "denied"],
    ["Pencere yakalama", capabilities.windowCapture],
    ["Global kısayol", capabilities.globalShortcuts],
    ["Kaydırmalı yakalama", capabilities.scrollingCapture],
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
