import { useCallback, useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  historyClear,
  historyList,
  historyRemove,
  uploadLastCapture,
  type HistoryEntry,
} from "../../lib/ipc";
import "./library.css";

/** Group label for an entry, matching how people actually look for a capture. */
function dayLabel(createdAt: number): string {
  const date = new Date(createdAt * 1000);
  const today = new Date();
  const isSameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();

  if (isSameDay(date, today)) return "Bugün";
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (isSameDay(date, yesterday)) return "Dün";

  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

const time = (createdAt: number) =>
  new Date(createdAt * 1000).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

export default function Library() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [search, setSearch] = useState("");
  const [uploadedOnly, setUploadedOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    historyList({ text: search || undefined, uploadedOnly })
      .then(setEntries)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [search, uploadedOnly]);

  // Debounced, so typing in the search box does not run a query per keystroke.
  useEffect(() => {
    const timer = window.setTimeout(refresh, 150);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  const copy = useCallback(async (text: string, what: string) => {
    await writeText(text);
    setNotice(`${what} kopyalandı`);
    window.setTimeout(() => setNotice(null), 1800);
  }, []);

  if (loading && entries.length === 0 && !search) {
    return <p className="muted">Yükleniyor…</p>;
  }

  // Entries arrive newest-first, so grouping in order preserves that order.
  const groups: { label: string; items: HistoryEntry[] }[] = [];
  for (const entry of entries) {
    const label = dayLabel(entry.createdAt);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(entry);
    else groups.push({ label, items: [entry] });
  }

  return (
    <div className="stack">
      <div className="row library__filters">
        <input
          className="input"
          type="search"
          placeholder="Dosya adı, pencere, URL veya tanınan metin ara"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <label className="row" style={{ gap: "var(--space-1)", whiteSpace: "nowrap" }}>
          <input
            type="checkbox"
            checked={uploadedOnly}
            onChange={(event) => setUploadedOnly(event.target.checked)}
          />
          <span className="muted">Sadece yüklenenler</span>
        </label>
        <button
          type="button"
          className="button"
          onClick={async () => {
            await historyClear();
            refresh();
          }}
        >
          Geçmişi temizle
        </button>
      </div>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
      {notice && (
        <p className="status status--ok" role="status">
          <span className="dot" aria-hidden="true" />
          {notice}
        </p>
      )}

      {entries.length === 0 && (
        <div className="card">
          <h2 className="card__title">
            {search || uploadedOnly ? "Eşleşen yakalama yok" : "Henüz yakalama yok"}
          </h2>
          <p className="card__hint">
            {search || uploadedOnly
              ? "Aramayı veya filtreyi değiştir."
              : "Yakala bölümünden bir işlem seç ya da global kısayolu kullan."}
          </p>
        </div>
      )}

      {groups.map((group) => (
        <section key={group.label} className="stack" style={{ gap: "var(--space-2)" }}>
          <h2 className="library__day">{group.label}</h2>
          <div className="grid">
            {group.items.map((entry) => (
              <figure key={entry.id} className="library__item">
                <div className="library__thumb">
                  {entry.path ? (
                    // Served from disk rather than re-encoded through IPC; the
                    // library can hold hundreds of these at once.
                    <img src={convertFileSrc(entry.path)} alt="" loading="lazy" />
                  ) : (
                    <span className="muted">Dosya yok</span>
                  )}
                </div>
                <figcaption className="library__meta">
                  <span className="library__name" title={entry.filename}>
                    {entry.filename}
                  </span>
                  <span className="library__sub">
                    {time(entry.createdAt)} · {entry.width} × {entry.height}
                    {entry.destination ? ` · ${entry.destination}` : ""}
                  </span>
                </figcaption>
                <div className="library__actions">
                  {entry.url ? (
                    <>
                      <button
                        type="button"
                        className="button"
                        onClick={() => void copy(entry.url as string, "URL")}
                      >
                        URL
                      </button>
                      <button
                        type="button"
                        className="button"
                        onClick={() => void openUrl(entry.url as string)}
                      >
                        Aç
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="button"
                      title="Son yakalamayı yükler"
                      onClick={async () => {
                        try {
                          await uploadLastCapture();
                          refresh();
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      Yükle
                    </button>
                  )}
                  {entry.path && (
                    <button
                      type="button"
                      className="button"
                      onClick={() => void copy(entry.path as string, "Yol")}
                    >
                      Yol
                    </button>
                  )}
                  <button
                    type="button"
                    className="button"
                    title="Sadece listeden kaldırır, dosyayı silmez"
                    onClick={async () => {
                      await historyRemove(entry.id);
                      refresh();
                    }}
                  >
                    Kaldır
                  </button>
                </div>
              </figure>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
