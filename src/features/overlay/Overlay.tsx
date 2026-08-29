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
import { drawShapesOnly, setImageReadyHandler } from "../editor/canvas";
import { imageFromEvent, placeAt } from "../../lib/paste";
import Magnifier from "./Magnifier";
import OverlayText from "./OverlayText";
import OverlayToolbar, {
  digitFor,
  OVERLAY_TOOLS,
  type OverlayTool,
} from "./OverlayToolbar";
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
  // Text is typed into a real textarea rather than captured key by key, so
  // input methods, dictation and screen readers all keep working.
  const [editing, setEditing] = useState<{ rect: Region; balloon: boolean; tail: Point } | null>(
    null,
  );
  const [magnify, setMagnify] = useState(false);
  const [redoStack, setRedoStack] = useState<Shape[][]>([]);

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

  /// Add a shape and drop the redo history, as any editor does: once you draw
  /// something new, the branch you undid is gone.
  const addShape = useCallback((shape: Shape) => {
    setShapes((current) => renumberSteps([...current, shape]));
    setRedoStack([]);
  }, []);

  const undo = useCallback(() => {
    setShapes((current) => {
      if (current.length === 0) return current;
      setRedoStack((stack) => [...stack, current]);
      return renumberSteps(current.slice(0, -1));
    });
  }, []);

  const redo = useCallback(() => {
    setRedoStack((stack) => {
      const previous = stack[stack.length - 1];
      if (!previous) return stack;
      setShapes(renumberSteps(previous));
      return stack.slice(0, -1);
    });
  }, []);

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
        case "spotlight":
          // Dims everything *outside* the rectangle, which is the opposite of
          // a highlight and the right tool for "look here" on a busy screen.
          return tiny ? null : { kind: "spotlight", rect, dim: 150 };
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

    const paint = () => {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const painted = previewShape ? [...shapes, previewShape] : shapes;
      drawShapesOnly(ctx, painted, canvas.width, canvas.height);
    };

    paint();

    // A pasted image decodes asynchronously, so the first paint after a paste
    // draws nothing for it. This is how it gets a second chance.
    setImageReadyHandler(paint);
    return () => setImageReadyHandler(null);
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
        addShape(buildShape(point, point)!);
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
      // Text and balloons need content before they are worth anything, so the
      // drag only sizes the box and the textarea decides whether a shape
      // exists at all.
      if (tool === "text" || tool === "balloon") {
        const rect = rectFromPoints(drawing.origin, point);
        setDrawing(null);
        setEditing({
          rect: rect.width < 8 || rect.height < 8 ? { ...rect, width: 220, height: 44 } : rect,
          balloon: tool === "balloon",
          // The tail points back at where the drag started, which is the
          // gesture people already make when they mean "this thing here".
          tail: drawing.origin,
        });
        return;
      }

      const shape = buildShape(drawing.origin, point, drawing.points);
      setDrawing(null);
      if (shape) addShape(shape);
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

  // ── Paste and drop ────────────────────────────────────────────────────

  const cursorRef = useRef<Point>({ x: 0, y: 0 });
  useEffect(() => {
    cursorRef.current = cursor;
  }, [cursor]);

  useEffect(() => {
    const accept = async (event: ClipboardEvent | DragEvent) => {
      const image = await imageFromEvent(event);
      if (!image) return;

      // Only swallow the event once there is actually an image; pasting text
      // onto a screenshot should fall through rather than being eaten.
      event.preventDefault();

      addShape({
        kind: "image",
        rect: placeAt(image, cursorRef.current, size),
        data: image.data,
        opacity: 1,
      });
    };

    const onPaste = (event: ClipboardEvent) => void accept(event);
    const onDrop = (event: DragEvent) => void accept(event);
    // Without this the webview navigates to the dropped file and the overlay
    // is replaced by an image viewer.
    const onDragOver = (event: DragEvent) => event.preventDefault();

    window.addEventListener("paste", onPaste);
    window.addEventListener("drop", onDrop);
    window.addEventListener("dragover", onDragOver);
    return () => {
      window.removeEventListener("paste", onPaste);
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragover", onDragOver);
    };
  }, [addShape, size]);

  // ── Keyboard ──────────────────────────────────────────────────────────

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // The textarea handles its own keys and stops them propagating; this is
      // the belt to that braces, so a stray focus can never turn typing into
      // tool switching.
      if (editing) return;

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
        if (event.shiftKey) redo();
        else undo();
        return;
      }
      if (!mod && event.key.toLowerCase() === "m") {
        event.preventDefault();
        setMagnify((on) => !on);
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

      if (!mod) {
        // A digit picks a tool by position and a letter picks it by name. The
        // digit is checked first because a letter tool could otherwise shadow
        // one — and because the numbers are the labels people can see.
        const byDigit = OVERLAY_TOOLS.findIndex(
          (_, index) => digitFor(index) === event.key,
        );
        const match =
          byDigit >= 0
            ? OVERLAY_TOOLS[byDigit]
            : OVERLAY_TOOLS.find((t) => t.key === event.key.toLowerCase());

        if (match) {
          event.preventDefault();
          setTool(match.id === "select" ? null : match.id);
          return;
        }
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
  }, [cancel, commit, editing, redo, selection, size, tool, undo]);

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
          canRedo={redoStack.length > 0}
          magnify={magnify}
          onTool={setTool}
          onColor={setColor}
          onWidth={setStrokeWidth}
          onUndo={undo}
          onRedo={redo}
          onMagnify={setMagnify}
          onCancel={cancel}
          onConfirm={() =>
            void commit(selection ?? { x: 0, y: 0, width: size.width, height: size.height })
          }
        />
      )}

      {editing && (
        <OverlayText
          rect={editing.rect}
          balloon={editing.balloon}
          tail={editing.tail}
          color={color}
          onCommit={(shape) => {
            addShape(shape);
            setEditing(null);
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {magnify && !busy && !editing && (
        <Magnifier cursor={cursor} origin={origin} bounds={size} />
      )}
    </div>
  );
}
