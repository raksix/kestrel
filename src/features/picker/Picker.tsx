import { useCallback, useEffect, useState } from "react";
import {
  captureDisplay,
  captureWindow,
  closeWindowPicker,
  displayThumbnail,
  listDisplays,
  listWindows,
  windowThumbnail,
  type DisplayInfo,
  type WindowInfo,
} from "../../lib/ipc";
import "./picker.css";

type Tab = "windows" | "displays";

interface Candidate {
  key: string;
  id: number;
  title: string;
  subtitle: string;
  width: number;
  height: number;
}

/**
 * The window / display picker.
 *
 * Thumbnails are fetched one at a time after the list renders, so a machine
 * with thirty open windows shows its list immediately instead of blocking on
 * thirty screen captures.
 */
export default function Picker({ initialTab }: { initialTab: Tab }) {
  const [tab, setTab] = useState<Tab>(initialTab);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [thumbnails, setThumbnails] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setThumbnails({});
    setSelected(0);

    const load = async () => {
      try {
        if (tab === "windows") {
          const windows = await listWindows();
          if (cancelled) return;
          setCandidates(
            windows.map((w: WindowInfo) => ({
              key: `w${w.id}`,
              id: w.id,
              title: w.title || w.app_name || `Pencere ${w.id}`,
              subtitle: w.app_name,
              width: w.region.width,
              height: w.region.height,
            })),
          );
        } else {
          const displays = await listDisplays();
          if (cancelled) return;
          setCandidates(
            displays.map((d: DisplayInfo) => ({
              key: `d${d.id}`,
              id: d.id,
              title: d.name,
              subtitle: d.is_primary ? "Birincil ekran" : "Ekran",
              width: d.region.width,
              height: d.region.height,
            })),
          );
        }
        setError(null);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [tab]);

  // Sequential thumbnail fetch: each capture is a real screen grab, and firing
  // them all at once makes the whole picker stutter.
  useEffect(() => {
    let cancelled = false;

    const fetchAll = async () => {
      for (const candidate of candidates) {
        if (cancelled) return;
        try {
          const preview =
            tab === "windows"
              ? await windowThumbnail(candidate.id)
              : await displayThumbnail(candidate.id);
          if (cancelled) return;
          setThumbnails((current) => ({ ...current, [candidate.key]: preview }));
        } catch {
          // A window that closed mid-scan simply has no thumbnail.
        }
      }
    };

    void fetchAll();
    return () => {
      cancelled = true;
    };
  }, [candidates, tab]);

  const choose = useCallback(
    async (candidate: Candidate) => {
      try {
        if (tab === "windows") {
          await captureWindow(candidate.id);
        } else {
          await captureDisplay(candidate.id);
        }
        await closeWindowPicker();
      } catch (e) {
        setError(String(e));
      }
    },
    [tab],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void closeWindowPicker();
        return;
      }
      if (event.key === "Tab") {
        event.preventDefault();
        setTab((current) => (current === "windows" ? "displays" : "windows"));
        return;
      }
      if (candidates.length === 0) return;

      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        setSelected((i) => (i + 1) % candidates.length);
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        setSelected((i) => (i - 1 + candidates.length) % candidates.length);
      } else if (event.key === "Enter") {
        event.preventDefault();
        void choose(candidates[selected]);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [candidates, selected, choose]);

  return (
    <div className="picker">
      <header className="picker__header">
        <div className="picker__tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "windows"}
            className="picker__tab"
            onClick={() => setTab("windows")}
          >
            Pencereler
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "displays"}
            className="picker__tab"
            onClick={() => setTab("displays")}
          >
            Ekranlar
          </button>
        </div>
        <button type="button" className="button" onClick={() => void closeWindowPicker()}>
          İptal
        </button>
      </header>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      {!error && !loading && candidates.length === 0 && (
        <div className="picker__empty">
          <p className="card__title">Yakalanacak bir şey bulunamadı</p>
          <p className="card__hint">
            {tab === "windows"
              ? "Açık pencere görünmüyor. macOS'ta bu genellikle Ekran Kaydı izninin verilmediği anlamına gelir."
              : "Ekran bilgisi alınamadı."}
          </p>
        </div>
      )}

      <div className="picker__grid" role="listbox" aria-label="Yakalama hedefleri">
        {candidates.map((candidate, index) => (
          <button
            key={candidate.key}
            type="button"
            role="option"
            aria-selected={index === selected}
            className={`picker__item ${index === selected ? "picker__item--selected" : ""}`}
            onMouseEnter={() => setSelected(index)}
            onClick={() => void choose(candidate)}
          >
            <div className="picker__thumb">
              {thumbnails[candidate.key] ? (
                <img src={thumbnails[candidate.key]} alt="" />
              ) : (
                <span className="picker__thumb-placeholder" aria-hidden="true" />
              )}
            </div>
            <span className="picker__title" title={candidate.title}>
              {candidate.title}
            </span>
            <span className="picker__subtitle">
              {candidate.subtitle} · {candidate.width} × {candidate.height}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
