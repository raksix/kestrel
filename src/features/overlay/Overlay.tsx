import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelRegionCapture,
  commitRegionCapture,
  listWindows,
  type Region,
  type WindowInfo,
} from "../../lib/ipc";
import "./overlay.css";

interface OverlayProps {
  /** Display bounds in the global logical coordinate space. */
  origin: { x: number; y: number };
  size: { width: number; height: number };
}

interface Point {
  x: number;
  y: number;
}

/** Below this a drag is treated as a click, so window snapping still works. */
const CLICK_THRESHOLD = 4;

function rectFromPoints(a: Point, b: Point): Region {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  };
}

/**
 * Full-screen selection overlay.
 *
 * Coordinates inside this component are *local* CSS pixels, which map 1:1 to
 * logical points because the window is sized to the display's logical bounds.
 * Only at commit time are they shifted into global space by adding the display
 * origin — the same space the Rust side crops in.
 */
export default function Overlay({ origin, size }: OverlayProps) {
  const [anchor, setAnchor] = useState<Point | null>(null);
  const [cursor, setCursor] = useState<Point>({ x: 0, y: 0 });
  const [selection, setSelection] = useState<Region | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [hovered, setHovered] = useState<WindowInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const committing = useRef(false);

  // Window rects power snap-to-window. If enumeration is unavailable (Wayland,
  // or a missing permission) the overlay still works as a plain drag selector.
  useEffect(() => {
    listWindows()
      .then(setWindows)
      .catch(() => setWindows([]));
  }, []);

  const commit = useCallback(
    async (region: Region) => {
      if (committing.current) return;
      if (region.width < 1 || region.height < 1) return;
      committing.current = true;
      setBusy(true);
      try {
        await commitRegionCapture({
          x: Math.round(region.x + origin.x),
          y: Math.round(region.y + origin.y),
          width: Math.round(region.width),
          height: Math.round(region.height),
        });
      } catch (error) {
        console.error("region capture failed", error);
        committing.current = false;
        setBusy(false);
      }
    },
    [origin],
  );

  const cancel = useCallback(() => {
    if (committing.current) return;
    committing.current = true;
    void cancelRegionCapture();
  }, []);

  // The window under the cursor, front-most first. `windows` arrives sorted by
  // stacking order, so the first hit is the visible one.
  const windowAt = useCallback(
    (point: Point): WindowInfo | null => {
      const globalX = point.x + origin.x;
      const globalY = point.y + origin.y;
      return (
        windows.find(
          (w) =>
            globalX >= w.region.x &&
            globalX < w.region.x + w.region.width &&
            globalY >= w.region.y &&
            globalY < w.region.y + w.region.height,
        ) ?? null
      );
    },
    [windows, origin],
  );

  const toLocal = useCallback(
    (region: Region): Region => ({
      x: region.x - origin.x,
      y: region.y - origin.y,
      width: region.width,
      height: region.height,
    }),
    [origin],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        cancel();
        return;
      }
      if (event.key === "Enter" && selection) {
        event.preventDefault();
        void commit(selection);
        return;
      }
      if (event.key === " " || event.code === "Space") {
        // Space captures the whole display, matching ShareX.
        event.preventDefault();
        void commit({ x: 0, y: 0, width: size.width, height: size.height });
        return;
      }

      // Arrow keys nudge the committed selection: 1px, or 10px with shift.
      if (selection && event.key.startsWith("Arrow")) {
        event.preventDefault();
        const step = event.shiftKey ? 10 : 1;
        const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
        const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;

        setSelection((current) => {
          if (!current) return current;
          // Alt resizes from the bottom-right; plain arrows move.
          if (event.altKey) {
            return {
              ...current,
              width: Math.max(1, current.width + dx),
              height: Math.max(1, current.height + dy),
            };
          }
          return { ...current, x: current.x + dx, y: current.y + dy };
        });
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [cancel, commit, selection, size]);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button === 2) {
      cancel();
      return;
    }
    const point = { x: event.clientX, y: event.clientY };
    setAnchor(point);
    setSelection({ x: point.x, y: point.y, width: 0, height: 0 });
    (event.target as HTMLElement).setPointerCapture?.(event.pointerId);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const point = { x: event.clientX, y: event.clientY };
    setCursor(point);

    if (anchor) {
      let next = rectFromPoints(anchor, point);
      // Shift constrains to a square, matching ShareX's proportional resize.
      if (event.shiftKey) {
        const edge = Math.max(next.width, next.height);
        next = {
          x: point.x < anchor.x ? anchor.x - edge : anchor.x,
          y: point.y < anchor.y ? anchor.y - edge : anchor.y,
          width: edge,
          height: edge,
        };
      }
      setSelection(next);
      setHovered(null);
    } else {
      setHovered(windowAt(point));
    }
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!anchor) return;
    const point = { x: event.clientX, y: event.clientY };
    const region = rectFromPoints(anchor, point);
    setAnchor(null);

    const isClick = region.width < CLICK_THRESHOLD && region.height < CLICK_THRESHOLD;
    if (isClick) {
      // A click with no drag means "capture the window under the cursor".
      const target = windowAt(point);
      if (target) {
        void commit(toLocal(target.region));
      } else {
        setSelection(null);
      }
      return;
    }

    void commit(region);
  };

  const highlight = hovered ? toLocal(hovered.region) : null;
  const active = selection ?? highlight;

  return (
    <div
      className="overlay"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onContextMenu={(event) => event.preventDefault()}
      role="application"
      aria-label="Bölge seçimi"
    >
      {/* Dimming drawn as four rectangles around the selection, so the
          selected area stays at full brightness without a mask. */}
      {active ? (
        <>
          <div className="overlay__dim" style={{ inset: `0 0 auto 0`, height: active.y }} />
          <div
            className="overlay__dim"
            style={{ top: active.y + active.height, left: 0, right: 0, bottom: 0 }}
          />
          <div
            className="overlay__dim"
            style={{ top: active.y, left: 0, width: active.x, height: active.height }}
          />
          <div
            className="overlay__dim"
            style={{
              top: active.y,
              left: active.x + active.width,
              right: 0,
              height: active.height,
            }}
          />
        </>
      ) : (
        <div className="overlay__dim" style={{ inset: 0 }} />
      )}

      {!anchor && !selection && (
        <>
          <div className="overlay__crosshair overlay__crosshair--h" style={{ top: cursor.y }} />
          <div className="overlay__crosshair overlay__crosshair--v" style={{ left: cursor.x }} />
        </>
      )}

      {active && (
        <div
          className={`overlay__selection ${hovered && !selection ? "overlay__selection--snap" : ""}`}
          style={{
            left: active.x,
            top: active.y,
            width: active.width,
            height: active.height,
          }}
        >
          {!hovered || selection ? (
            <>
              <span className="overlay__handle overlay__handle--nw" />
              <span className="overlay__handle overlay__handle--ne" />
              <span className="overlay__handle overlay__handle--sw" />
              <span className="overlay__handle overlay__handle--se" />
            </>
          ) : null}
        </div>
      )}

      {active && (
        <div
          className="overlay__badge"
          style={{
            left: active.x,
            top: active.y > 28 ? active.y - 26 : active.y + active.height + 6,
          }}
        >
          {hovered && !selection
            ? `${hovered.app_name || hovered.title || "Pencere"} · ${Math.round(active.width)} × ${Math.round(active.height)}`
            : `${Math.round(active.width)} × ${Math.round(active.height)}`}
        </div>
      )}

      <div className="overlay__hint">
        {busy ? (
          <span>Yakalanıyor…</span>
        ) : (
          <>
            <span>Sürükle: bölge seç</span>
            <span>Tıkla: pencereyi yakala</span>
            <span>Space: tüm ekran</span>
            <span>Esc: iptal</span>
          </>
        )}
      </div>
    </div>
  );
}
