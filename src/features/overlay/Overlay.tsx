import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cancelRegionCapture,
  clipboardImage,
  commitRegionCapture,
  commitRegionRecording,
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
import { imageFromClipboard, imageFromEvent, placeAt } from "../../lib/paste";
import Magnifier from "./Magnifier";
import OverlayText from "./OverlayText";
import OverlayToolbar, {
  digitFor,
  OVERLAY_TOOLS,
  type OverlayTool,
} from "./OverlayToolbar";
import RecordToolbar from "./RecordToolbar";
import "./overlay.css";

/** What the rectangle being drawn is for. */
export type OverlayMode = "capture" | "record";

/**
 * The recorder's frame rate, shown next to the selection size.
 *
 * Mirrored from `RecordSettings::default` in Rust rather than fetched: it is a
 * label, and one extra IPC round trip at the moment the overlay appears is
 * exactly what the overlay path is built to avoid.
 */
const RECORD_FPS = 30;

interface OverlayProps {
  /** Display bounds in the global logical coordinate space. */
  origin: { x: number; y: number };
  size: { width: number; height: number };
  /** Physical pixels per logical point on this display. */
  scale: number;
  /**
   * The frozen screen for this display, staged on disk by Rust.
   *
   * Null when staging failed, in which case the overlay falls back to being
   * transparent over the live screen — usable, but without a dimmed backdrop
   * and with blur and pixelate previewing nothing, since they can only redact
   * pixels they can see.
   */
  framePath: string | null;
  /**
   * Whether the selection becomes a screenshot or a screen recording.
   *
   * In record mode there is no frozen frame — the user has to see the live
   * screen to frame what they are about to record — and the annotation tools
   * are gone, because nothing drawn here could survive into a video.
   */
  mode: OverlayMode;
  /** In record mode, whether the output is an animated GIF. */
  gif: boolean;
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
/** How much of the screen the dim takes away. Matches ShareX closely enough
 * that a screenshot of the overlay looks familiar. */
const DIM = "rgba(0, 0, 0, 0.42)";

/**
 * Dim everything except the selection.
 *
 * Four rectangles rather than a full fill with a hole punched in it: punching
 * a hole means `clearRect`, which would take the frozen frame with it and leave
 * the selection showing the live screen instead of the capture.
 *
 * With no selection yet the whole screen dims. Hovering a window used to leave
 * that window bright, which meant most of the screen stayed undimmed and the
 * overlay did not look like it had done anything — the window snap is shown by
 * its outline instead.
 */
function dimAround(
  ctx: CanvasRenderingContext2D,
  selection: Region | null,
  width: number,
  height: number,
): void {
  ctx.save();
  ctx.fillStyle = DIM;

  if (!selection || selection.width < 1 || selection.height < 1) {
    ctx.fillRect(0, 0, width, height);
    ctx.restore();
    return;
  }

  const { x, y, width: w, height: h } = selection;
  ctx.fillRect(0, 0, width, y);
  ctx.fillRect(0, y + h, width, height - (y + h));
  ctx.fillRect(0, y, x, h);
  ctx.fillRect(x + w, y, width - (x + w), h);
  ctx.restore();
}

export default function Overlay({
  origin,
  size,
  scale,
  framePath,
  mode,
  gif,
}: OverlayProps) {
  const recording = mode === "record";

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
  const [frame, setFrame] = useState<HTMLImageElement | null>(null);

  const committing = useRef(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!framePath) return;
    const element = new Image();
    element.onload = () => setFrame(element);
    element.onerror = () =>
      // Not fatal: without a backdrop the overlay still selects, so this is a
      // downgrade rather than a failure.
      console.warn("the frozen frame could not be loaded; overlay has no backdrop");
    element.src = convertFileSrc(framePath);
  }, [framePath]);

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

      // Local CSS pixels are logical points on this display; the commands both
      // take global logical coordinates.
      const global = {
        x: Math.round(region.x + origin.x),
        y: Math.round(region.y + origin.y),
        width: Math.round(region.width),
        height: Math.round(region.height),
      };

      try {
        if (recording) {
          // No document: a recording is encoded from the live screen frame by
          // frame, so there is nothing for annotations to be composited onto.
          await commitRegionRecording(global);
          return;
        }

        // Annotations were drawn in screen space; the captured image starts at
        // the selection origin and is in physical pixels.
        const drawn = shapesRef.current.map((shape) =>
          transformShape(shape, -region.x, -region.y, scale),
        );
        const document = drawn.length > 0 ? JSON.stringify({ shapes: drawn }) : undefined;

        await commitRegionCapture(global, document);
      } catch (error) {
        // In record mode the overlays are already closed by the time an error
        // can happen, so the failure is reported by Rust as a capture failure
        // and surfaces in the main window instead of in a window that is gone.
        console.error(recording ? "region recording failed" : "region capture failed", error);
        committing.current = false;
        setBusy(false);
      }
    },
    [origin, recording, scale],
  );

  const cancel = useCallback(() => {
    if (committing.current) return;
    committing.current = true;
    void cancelRegionCapture();
  }, []);

  /**
   * Move or resize the selection by one arrow key.
   *
   * A drag lands within a pixel or two of what was meant, and for a recording
   * the size decides the encoded frame, so the last few pixels are worth
   * having. Alt resizes instead of moving; Shift makes the step ten.
   */
  const nudge = useCallback((event: KeyboardEvent) => {
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

  const highlight = hovered ? toLocal(hovered.region) : null;
  const active = selection ?? highlight;

  // Everything is drawn on one canvas: the frozen screen, the annotations, then
  // the dim. Layering these as separate DOM elements was how blur and pixelate
  // ended up with nothing to sample.
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const paint = () => {
      // The backing store is in physical pixels so the frozen screen is not
      // softened on a Retina display, but everything below works in logical
      // points — pointer coordinates, shape geometry, the dim — so the
      // transform converts once here instead of at every call site.
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      ctx.clearRect(0, 0, size.width, size.height);

      // The frozen screen first, so blur and pixelate have real pixels to
      // redact. On a transparent canvas they sampled nothing and looked like
      // they only affected the shapes that had already been drawn.
      if (frame) {
        ctx.drawImage(frame, 0, 0, size.width, size.height);
      }

      const painted = previewShape ? [...shapes, previewShape] : shapes;
      drawShapesOnly(ctx, painted, size.width, size.height);

      // The dim goes on last and only outside the selection, so annotations
      // inside it stay at full strength while everything that will not be
      // captured recedes.
      //
      // In record mode there is no frozen frame by design, and the dim is
      // painted anyway: a translucent black over the transparent window dims
      // the live screen underneath, which is the only thing that shows the user
      // what is inside the rectangle and what is not. For a capture the dim is
      // skipped without a backdrop, because blur and pixelate previews would
      // then be dimming pixels they cannot see.
      if (frame || recording) {
        dimAround(ctx, selection, size.width, size.height);
      }
    };

    paint();

    // A pasted image decodes asynchronously, so the first paint after a paste
    // draws nothing for it. This is how it gets a second chance.
    setImageReadyHandler(paint);
    return () => setImageReadyHandler(null);
  }, [shapes, previewShape, size, scale, frame, recording, selection]);

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

    // A recording is not committed by letting go of the mouse. A screenshot is
    // instant and repeatable, so releasing the drag is the whole gesture; a
    // recording is a decision you then live inside for a minute, and starting
    // one on a slightly wrong rectangle costs a retake. So the release only
    // *sets* the rectangle — Enter or the record button starts it, and the
    // arrow keys can still fix the last few pixels first.
    if (recording) {
      if (isClick) {
        const target = windowAt(point);
        // Clicking a window frames it, which is the fast way to record one app.
        setSelection(target ? toLocal(target.region) : null);
        setHovered(null);
      } else {
        setSelection(region);
      }
      return;
    }

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
    // Nothing pasted can end up in a video, so in record mode a paste is left
    // to whatever is underneath rather than being swallowed by the overlay.
    if (recording) return;

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
  }, [addShape, recording, size]);

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
      // Everything below this point either draws or picks a tool, neither of
      // which exists while a recording region is being chosen. Enter, Space,
      // the arrows and Escape are handled above and below it.
      if (recording) {
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
        if (selection && event.key.startsWith("Arrow")) {
          event.preventDefault();
          nudge(event);
        }
        return;
      }

      if (mod && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (event.shiftKey) redo();
        else undo();
        return;
      }
      if (mod && event.key.toLowerCase() === "v") {
        // The webview's own paste event does not reach a transparent
        // always-on-top window reliably, so the shortcut is handled here and
        // the clipboard is read through Rust.
        event.preventDefault();
        void imageFromClipboard(clipboardImage).then((image) => {
          if (!image) return;
          addShape({
            kind: "image",
            rect: placeAt(image, cursorRef.current, size),
            data: image.data,
            opacity: 1,
          });
        });
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
        nudge(event);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [addShape, cancel, commit, editing, nudge, recording, redo, selection, size, tool, undo]);

  return (
    <div
      className={`overlay ${tool ? "overlay--drawing" : ""} ${
        recording ? "overlay--record" : ""
      }`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onContextMenu={(event) => event.preventDefault()}
      role="application"
      aria-label={recording ? "Kaydedilecek bölgeyi seç" : "Bölge seçimi"}
    >
      {/*
        No dim elements here: the canvas paints the frozen screen, the
        annotations and the dim in one pass, so blur and pixelate have real
        pixels to work from. When the frame could not be staged the canvas is
        transparent and the live screen shows through undimmed.
      */}
      <canvas
        ref={canvasRef}
        className="overlay__canvas"
        // The backing store is physical pixels; CSS stretches it back to the
        // window. Without this the frozen screen is drawn at half resolution on
        // a Retina display and the overlay looks softer than the desktop it is
        // covering.
        width={Math.round(size.width * scale)}
        height={Math.round(size.height * scale)}
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

      {/* A recording rectangle carries a red frame as well as the white one:
          it is the same shape as a screenshot selection and means something
          very different, and red is what every recorder on every platform
          uses to say so. */}
      {recording && active && (
        <div
          className="overlay__record-ring"
          style={{ left: active.x, top: active.y, width: active.width, height: active.height }}
          aria-hidden="true"
        />
      )}

      {busy ? (
        <div className="overlay__hint">
          <span>{recording ? "Kayıt başlatılıyor…" : "Yakalanıyor…"}</span>
        </div>
      ) : recording ? (
        <RecordToolbar
          gif={gif}
          selection={selection}
          fps={RECORD_FPS}
          onCancel={cancel}
          onStart={() =>
            void commit(selection ?? { x: 0, y: 0, width: size.width, height: size.height })
          }
        />
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

      {/* The magnifier samples the *frozen* screen, which record mode
          deliberately does not have. */}
      {magnify && !recording && !busy && !editing && (
        <Magnifier cursor={cursor} origin={origin} bounds={size} />
      )}
    </div>
  );
}
