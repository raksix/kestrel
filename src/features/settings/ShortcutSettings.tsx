import { useCallback, useEffect, useState } from "react";
import {
  acceleratorFromEvent,
  formatShortcut,
  onShortcutsChanged,
  resetShortcuts,
  setWorkflowEnabled,
  setWorkflowShortcut,
  shortcutRegistrationReport,
  type ShortcutReport,
  type Workflow,
} from "../../lib/ipc";
import "./settings.css";

/**
 * Shortcut editor.
 *
 * Registering a global shortcut can fail — another application may already own
 * the combination, and the OS gives it to whoever asked first. Rather than
 * leaving the user pressing a dead key, every binding shows whether it was
 * actually accepted.
 */
export default function ShortcutSettings({
  workflows,
  onWorkflowsChanged,
}: {
  workflows: Workflow[];
  onWorkflowsChanged: (workflows: Workflow[]) => void;
}) {
  const [recording, setRecording] = useState<string | null>(null);
  const [reports, setReports] = useState<ShortcutReport[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    shortcutRegistrationReport().then(setReports).catch(() => undefined);
    const unlisten = onShortcutsChanged(setReports);
    return () => {
      unlisten.then((un) => un()).catch(() => undefined);
    };
  }, []);

  const apply = useCallback(
    async (id: string, accelerator: string | null) => {
      try {
        onWorkflowsChanged(await setWorkflowShortcut(id, accelerator));
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [onWorkflowsChanged],
  );

  // While recording, the whole window listens: the user is pressing modifier
  // combinations that a focused input would otherwise swallow or act on.
  useEffect(() => {
    if (!recording) return;

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        setRecording(null);
        return;
      }
      if (event.key === "Backspace" || event.key === "Delete") {
        void apply(recording, null);
        setRecording(null);
        return;
      }

      const accelerator = acceleratorFromEvent(event);
      if (!accelerator) return; // Modifiers only so far — keep listening.

      void apply(recording, accelerator);
      setRecording(null);
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording, apply]);

  const reportFor = (id: string) => reports.find((r) => r.workflowId === id);

  return (
    <section className="stack">
      <div className="row">
        <div>
          <h2 className="card__title">Kısayollar</h2>
          <p className="card__hint">
            Bir kısayolu değiştirmek için üzerine tıkla ve yeni tuş birleşimine bas.
            Silmek için kaydederken Backspace, vazgeçmek için Esc.
          </p>
        </div>
        <div className="toolbar__spacer" />
        <button
          type="button"
          className="button"
          onClick={async () => {
            try {
              onWorkflowsChanged(await resetShortcuts());
              setError(null);
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          Varsayılanlara dön
        </button>
      </div>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      <ul className="shortcut-list">
        {workflows.map((workflow) => {
          const report = reportFor(workflow.id);
          const failed = workflow.enabled && report && !report.registered;
          const stolen = workflow.enabled && report?.systemConflict;
          const isRecording = recording === workflow.id;

          return (
            <li key={workflow.id} className="shortcut-row">
              <label className="shortcut-row__toggle">
                <input
                  type="checkbox"
                  checked={workflow.enabled}
                  onChange={async (event) => {
                    try {
                      onWorkflowsChanged(
                        await setWorkflowEnabled(workflow.id, event.target.checked),
                      );
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                />
                <span className="visually-hidden">{workflow.name} etkin</span>
              </label>

              <div className="shortcut-row__label">
                <span className="shortcut-row__name">{workflow.name}</span>
                {failed && (
                  <span className="shortcut-row__warning">
                    Kayıt edilemedi — başka bir uygulama bu kısayolu almış
                  </span>
                )}
                {!failed && stolen && (
                  <span className="shortcut-row__warning">
                    İşletim sistemi bu kısayolu kullanıyor ({report?.systemConflict}), tuşa
                    bastığında Kestrel'e ulaşmaz. Başka bir birleşim seç.
                  </span>
                )}
              </div>

              <button
                type="button"
                className={`shortcut-key ${isRecording ? "shortcut-key--recording" : ""} ${
                  failed || stolen ? "shortcut-key--failed" : ""
                }`}
                onClick={() => setRecording(isRecording ? null : workflow.id)}
                aria-label={`${workflow.name} kısayolunu değiştir`}
              >
                {isRecording
                  ? "Tuşlara bas…"
                  : workflow.shortcut
                    ? formatShortcut(workflow.shortcut)
                    : "Atanmadı"}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
