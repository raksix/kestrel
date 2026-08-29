import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  cancelRegionCapture,
  commitRegionCapture,
  listWindows,
  type Region,
  type WindowInfo,
} from "../../lib/ipc";
import {
  fromHex,
  renumberSteps,
  transformShape,
  TRANSPARENT,
  type Color,
  type Shape,
} from "../../lib/editorTypes";
import { drawShapesOnly } from "../editor/canvas";
import OverlayToolbar, { OVERLAY_TOOLS, type OverlayTool } from "./OverlayToolbar";
import "./overlay.css";

interface OverlayProps {
  /** Display bounds in the global logical coordinate space. */
  origin: { x: number; y: number };
  size: { width: number; height: number };
  /** Physical pixels per logical point on this display. */
  scale: number;
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
 * Full-screen selection overlay, with annotation.
 *
 * Coordinates here are *local* CSS pixels, which map 1:1 to logical points
 * because the window is sized to the display's logical bounds. Only at commit
 * time are they shifted into global space, and annotations additionally scaled
 * into the captured image's physical pixels.
 *
 * The interaction follows ShareX: pick a tool and draw, then drag a region and
 * release to finish. With no tool selected — the default — a drag is the
 * selection itself, so the fast path stays one gesture.
 */
export default function Overlay({ origin, size, scale }: OverlayProps) {
  const [anchor, setAnchor] = useState<Point | null>(null);
  const [cursor, setCursor] = useState<Point>({ x: 0, y: 0 });
  const [selection, setSelection] = useState<Region | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [hovered, setHovered] = useState<WindowInfo | null>(null);
  const [busy, setBusy] = useState(false);

  const [tool, setTool] = useState<OverlayTool>(null);
  const [color, setColor] = useState<Color>(fromHex("#ff453a"));
  const [strokeWidth, setStrokeWidth] = useState(4);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [drawing, setDrawing] = useState<{ origin: Point; current: Point; points?: Point[] } | null>(
    null,
  );

  const committing = useRef(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Window rects power snap-to-window. If enumeration is unavailable (Wayland,
  // or a missing permission) the overlay still works as a plain drag selector.
  useEffect(() => {
    listWindows()
      .then(setWindows)
      .catch(() => setWindows([]));
  }, []);

  const shapesRef = useRef<Shape[]>([]);
  useEffect(() => {
    shapesRef.current = shapes;
  }, [shapes]);

  const commit = useCallback(
    async (region: Region) => {
      if (committing.current) return;
      if (region.width < 1 || region.height < 1) return;
      committing.current = true;
      setBusy(true);

      // Annotations were drawn in screen space; the captured image starts at
      // the selection origin and is in physical pixels.
      const drawn = shapesRef.current.map((shape) =>
        transformShape(shape, -region.x, -region.y, scale),
      );
      const document = drawn.length > 0 ? JSON.stringify({ shapes: drawn }) : undefined;

      try {
        await commitRegionCapture(
          {
            x: Math.round(region.x + origin.x),
            y: Math.round(region.y + origin.y),
            width: Math.round(region.width),
            height: Math.round(region.height),
          },
          document,
        );
      } catch (error) {
        console.error("region capture failed", error);
        committing.current = false;
        setBusy(false);
      }
    },
    [origin, scale],
  );

  const cancel = useCallback(() => {
    if (committing.current) return;
    committing.current = true;
    void cancelRegionCapture();
  }, []);

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

  // ── Drawing ───────────────────────────────────────────────────────────

  const buildShape = useCallback(
    (from: Point, to: Point, points?: Point[]): Shape | null => {
      const stroke = { color, width: strokeWidth };
      const rect = rectFromPoints(from, to);
      const tiny = rect.width < 2 && rect.height < 2;
      const stepNumber = shapes.filter((s) => s.kind === "step").length + 1;

      switch (tool) {
        case "rectangle":
          return tiny ? null : { kind: "rectangle", rect, stroke, fill: TRANSPARENT, corner_radius: 0 };
        case "ellipse":
          return tiny ? null : { kind: "ellipse", rect, stroke, fill: TRANSPARENT };
        case "line":
          return tiny ? null : { kind: "line", from, to, stroke };
        case "arrow":
          return tiny ? null : { kind: "arrow", from, to, stroke, head: "end" };
        case "freehand":
          return !points || points.length < 2 ? null : { kind: "freehand", points, stroke };
        case "highlight":
          return tiny ? null : { kind: "highlight", rect, color: { ...color, a: 90 } };
        case "blur":
          return tiny ? null : { kind: "blur", rect, radius: 12 };
        case "pixelate":
          return tiny ? null : { kind: "pixelate", rect, block: 12 };
        case "step":
          return {
            kind: "step",
            center: to,
            radius: 16,
            number: stepNumber,
            fill: color,
            text_color: { r: 255, g: 255, b: 255, a: 255 },
          };
        default:
          return null;
      }
    },
    [tool, color, strokeWidth, shapes],
  );

  const previewShape = useMemo(
    () => (drawing ? buildShape(drawing.origin, drawing.current, drawing.points) : null),
    [drawing, buildShape],
  );

  // Annotations are painted on their own transparent canvas above the dimming,
  // so the dim layer never ends up composited into a shape.
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const painted = previewShape ? [...shapes, previewShape] : shapes;
    drawShapesOnly(ctx, painted, canvas.width, canvas.height);
  }, [shapes, previewShape, size]);

  // ── Pointer ───────────────────────────────────────────────────────────

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button === 2) {
      cancel();
      return;
    }
    const point = { x: event.clientX, y: event.clientY };
    (event.target as HTMLElement).setPointerCapture?.(event.pointerId);

    if (tool) {
      if (tool === "step") {
        setShapes((current) => renumberSteps([...current, buildShape(point, point)!]));
        return;
      }
      setDrawing({
        origin: point,
        current: point,
        points: tool === "freehand" ? [point] : undefined,
      });
      return;
    }

    setAnchor(point);
    setSelection({ x: point.x, y: point.y, width: 0, height: 0 });
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const point = { x: event.clientX, y: event.clientY };
    setCursor(point);

    if (drawing) {
      setDrawing({
        ...drawing,
        current: point,
        points: drawing.points ? [...drawing.points, point] : undefined,
      });
      return;
    }

    if (anchor) {
      let next = rectFromPoints(anchor, point);
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
      return;
    }

    if (!tool) setHovered(windowAt(point));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    const point = { x: event.clientX, y: event.clientY };

    if (drawing) {
      const shape = buildShape(drawing.origin, point, drawing.points);
      setDrawing(null);
      if (shape) setShapes((current) => renumberSteps([...current, shape]));
      return;
    }

    if (!anchor) return;
    const region = rectFromPoints(anchor, point);
    setAnchor(null);

    const isClick = region.width < CLICK_THRESHOLD && region.height < CLICK_THRESHOLD;
    if (isClick) {
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

  // ── Keyboard ──────────────────────────────────────────────────────────

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;

      if (event.key === "Escape") {
        event.preventDefault();
        // Escape backs out of a tool first; only then does it cancel, so a
        // mis-picked tool does not cost the whole capture.
        if (tool) setTool(null);
        else cancel();
        return;
      }
      if (mod && event.key.toLowerCase() === "z") {
        event.preventDefault();
        setShapes((current) => renumberSteps(current.slice(0, -1)));
        return;
      }
      if (event.key === "Enter" && selection) {
        event.preventDefault();
        void commit(selection);
        return;
      }
      if (event.key === " " || event.code === "Space") {
        event.preventDefault();
        void commit({ x: 0, y: 0, width: size.width, height: size.height });
        return;
      }

      const match = OVERLAY_TOOLS.find((t) => t.key === event.key.toLowerCase());
      if (match && !mod) {
        event.preventDefault();
        setTool(match.id === "select" ? null : match.id);
        return;
      }

      if (selection && event.key.startsWith("Arrow")) {
        event.preventDefault();
        const step = event.shiftKey ? 10 : 1;
        const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
        const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;

        setSelection((current) => {
          if (!current) return current;
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
  }, [cancel, commit, selection, size, tool]);

  const highlight = hovered ? toLocal(hovered.region) : null;
  const active = selection ?? highlight;

  return (
    <div
      className={`overlay ${tool ? "overlay--drawing" : ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onContextMenu={(event) => event.preventDefault()}
      role="application"
      aria-label="Bölge seçimi"
    >
      {active ? (
        <>
          <div className="overlay__dim" style={{ inset: "0 0 auto 0", height: active.y }} />
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
            style={{ top: active.y, left: active.x + active.width, right: 0, height: active.height }}
          />
        </>
      ) : (
        <div className="overlay__dim" style={{ inset: 0 }} />
      )}

      <canvas
        ref={canvasRef}
        className="overlay__canvas"
        width={size.width}
        height={size.height}
      />

      {!anchor && !selection && !tool && (
        <>
          <div className="overlay__crosshair overlay__crosshair--h" style={{ top: cursor.y }} />
          <div className="overlay__crosshair overlay__crosshair--v" style={{ left: cursor.x }} />
        </>
      )}

      {active && (
        <div
          className={`overlay__selection ${hovered && !selection ? "overlay__selection--snap" : ""}`}
          style={{ left: active.x, top: active.y, width: active.width, height: active.height }}
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

      {busy ? (
        <div className="overlay__hint">
          <span>Yakalanıyor…</span>
        </div>
      ) : (
        <OverlayToolbar
          tool={tool}
          color={color}
          width={strokeWidth}
          canUndo={shapes.length > 0}
          onTool={setTool}
          onColor={setColor}
          onWidth={setStrokeWidth}
          onUndo={() => setShapes((current) => renumberSteps(current.slice(0, -1)))}
          onCancel={cancel}
          onConfirm={() =>
            void commit(selection ?? { x: 0, y: 0, width: size.width, height: size.height })
          }
        />
      )}
    </div>
  );
}
