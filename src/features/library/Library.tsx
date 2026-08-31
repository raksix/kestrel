import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import {
  historyClear,
  historyList,
  historyRemove,
  libraryThumbnail,
  onHistoryChanged,
  onRecordingChanged,
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

const extensionOf = (path: string) => path.split(".").pop()?.toLowerCase() ?? "";

/** Extensions the recorder can produce, plus the ones the converter can. */
const VIDEO = new Set(["mp4", "mov", "mkv", "webm", "avi", "m4v"]);

/**
 * What kind of file an entry points at.
 *
 * From the extension rather than from the database, because the history stores
 * what was captured and not how to display it — and because an entry written by
 * an older build has no such column to read.
 */
function kindOf(entry: HistoryEntry): "image" | "video" | "gif" | "none" {
  if (!entry.path) return "none";
  const extension = extensionOf(entry.path);
  if (VIDEO.has(extension)) return "video";
  if (extension === "gif") return "gif";
  return "image";
}

/**
 * The tile picture for one entry.
 *
 * An image or a GIF is loaded straight off disk over the asset protocol — the
 * grid holds hundreds of these, and pushing them through IPC would not scale.
 * A video has no picture to load, so Rust extracts one frame into the cache and
 * this shows that. Until this existed every recording in the library was a
 * blank tile, which is exactly what "recordings are not saved" looked like.
 */
function Thumbnail({ entry }: { entry: HistoryEntry }) {
  const kind = kindOf(entry);
  const [poster, setPoster] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (kind !== "video" || !entry.path) return;
    let cancelled = false;
    libraryThumbnail(entry.path)
      .then((path) => {
        if (!cancelled) setPoster(path);
      })
      // A missing ffmpeg or a moved file costs the picture, not the entry: the
      // filename, the size and every action below still work.
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [entry.path, kind]);

  if (kind === "none") return <span className="muted">Dosya yok</span>;

  if (kind === "video") {
    if (failed) return <span className="muted">Video</span>;
    if (!poster) return <span className="muted">Kare alınıyor…</span>;
    return (
      <>
        <img src={convertFileSrc(poster)} alt="" loading="lazy" />
        <span className="library__play" aria-hidden="true" />
      </>
    );
  }

  return (
    <>
      <img
        src={convertFileSrc(entry.path as string)}
        alt=""
        loading="lazy"
        onError={() => setFailed(true)}
      />
      {kind === "gif" && <span className="library__tag">GIF</span>}
      {failed && <span className="muted">Görüntü açılamadı</span>}
    </>
  );
}

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

  // Almost nothing that lands in the history is started here: it comes from the
  // tray, a global shortcut or the selection overlay. Without these the library
  // is a snapshot of whatever it happened to load, and a recording finished
  // while this panel was open never appeared at all.
  //
  // Read through a ref so subscribing does not depend on the current search
  // text — re-listening on every keystroke would drop events in the gap.
  const refreshRef = useRef(refresh);
  useEffect(() => {
    refreshRef.current = refresh;
  }, [refresh]);

  useEffect(() => {
    const reload = () => refreshRef.current();
    const unlisteners = [
      onHistoryChanged(reload),
      // A recording only reaches the history when it stops, and that is exactly
      // when this fires with `active: false`.
      onRecordingChanged((status) => {
        if (!status.active) reload();
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => undefined));
    };
  }, []);

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
        <button type="button" className="button" onClick={refresh}>
          Yenile
        </button>
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
            {group.items.map((entry) => {
              const kind = kindOf(entry);
              return (
                <figure key={entry.id} className="library__item">
                  <div className="library__thumb">
                    <Thumbnail entry={entry} />
                  </div>
                  <figcaption className="library__meta">
                    <span className="library__name" title={entry.filename}>
                      {entry.filename}
                    </span>
                    <span className="library__sub">
                      {time(entry.createdAt)} · {entry.width} × {entry.height}
                      {kind === "video" ? " · video" : ""}
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
                          Bağlantıyı aç
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
                      <>
                        {/* The only way to actually watch a recording from
                            here: the webview cannot play it inline, and a
                            video the library will not open is a video the
                            library may as well not list. */}
                        <button
                          type="button"
                          className="button"
                          title={kind === "video" ? "Varsayılan oynatıcıda aç" : "Varsayılan uygulamada aç"}
                          onClick={async () => {
                            try {
                              await openPath(entry.path as string);
                            } catch (e) {
                              setError(String(e));
                            }
                          }}
                        >
                          {kind === "video" ? "Oynat" : "Aç"}
                        </button>
                        <button
                          type="button"
                          className="button"
                          onClick={() => void copy(entry.path as string, "Yol")}
                        >
                          Yol
                        </button>
                      </>
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
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}
