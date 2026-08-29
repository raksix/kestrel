import { useCallback, useEffect, useState } from "react";
import {
  captureDisplay,
  captureWindow,
  closeWindowPicker,
  listDisplayPreviews,
  listWindowPreviews,
  type TargetPreview,
} from "../../lib/ipc";
import "./picker.css";

type Tab = "windows" | "displays";

/**
 * The window / display picker.
 *
 * The list and every thumbnail arrive together from one screen grab, so the
 * grid appears at once instead of filling in one slow capture at a time.
 */
export default function Picker({ initialTab }: { initialTab: Tab }) {
  const [tab, setTab] = useState<Tab>(initialTab);
  const [targets, setTargets] = useState<TargetPreview[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSelected(0);

    const load = tab === "windows" ? listWindowPreviews : listDisplayPreviews;
    load()
      .then((next) => {
        if (cancelled) return;
        setTargets(next);
        setError(null);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [tab]);

  const choose = useCallback(
    async (target: TargetPreview) => {
      try {
        if (tab === "windows") {
          await captureWindow(target.id);
        } else {
          await captureDisplay(target.id);
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
      if (targets.length === 0) return;

      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        setSelected((i) => (i + 1) % targets.length);
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        setSelected((i) => (i - 1 + targets.length) % targets.length);
      } else if (event.key === "Enter") {
        event.preventDefault();
        void choose(targets[selected]);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [targets, selected, choose]);

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
        {loading && <span className="muted">Yükleniyor…</span>}
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

      {!error && !loading && targets.length === 0 && (
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
        {targets.map((target, index) => (
          <button
            key={`${tab}-${target.id}`}
            type="button"
            role="option"
            aria-selected={index === selected}
            className={`picker__item ${index === selected ? "picker__item--selected" : ""}`}
            onMouseEnter={() => setSelected(index)}
            onClick={() => void choose(target)}
          >
            <div className="picker__thumb">
              {target.preview ? (
                <img src={target.preview} alt="" />
              ) : (
                <span className="picker__thumb-placeholder" aria-hidden="true" />
              )}
            </div>
            <span className="picker__title" title={target.title}>
              {target.title || `#${target.id}`}
            </span>
            <span className="picker__subtitle">
              {target.subtitle} · {target.width} × {target.height}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
