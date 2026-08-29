import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  boundsOf,
  closeEditor,
  editorExport,
  editorSession,
  fromHex,
  hitTest,
  rectFromCorners,
  renumberSteps,
  toHex,
  translate,
  TRANSPARENT,
  type Color,
  type EditorOpened,
  type Point,
  type Shape,
} from "../../lib/editorTypes";
import { drawDocument, drawSelection } from "./canvas";
import "./editor.css";

/**
 * Tool identifiers and their ShareX keyboard shortcuts. Keeping the letters
 * identical means muscle memory carries over unchanged.
 */
const TOOLS = [
  { id: "select", key: "v", label: "Seç" },
  { id: "rectangle", key: "r", label: "Dikdörtgen" },
  { id: "ellipse", key: "e", label: "Elips" },
  { id: "line", key: "l", label: "Çizgi" },
  { id: "arrow", key: "a", label: "Ok" },
  { id: "freehand", key: "f", label: "Serbest" },
  { id: "step", key: "n", label: "Adım" },
  { id: "highlight", key: "h", label: "Vurgu" },
  { id: "blur", key: "b", label: "Bulanık" },
  { id: "pixelate", key: "p", label: "Piksel" },
  { id: "spotlight", key: "s", label: "Spot" },
] as const;

type ToolId = (typeof TOOLS)[number]["id"];

const HISTORY_LIMIT = 200;

interface Drag {
  tool: ToolId;
  origin: Point;
  current: Point;
  /** Set when dragging an existing shape rather than creating one. */
  movingIndex?: number;
  /**
   * The shape list as it stood before a move began. A move is applied live for
   * responsiveness, so by the time the drag ends the current list is already
   * the moved one — undoing to *that* would do nothing visible.
   */
  before?: Shape[];
  points?: Point[];
}

export default function Editor() {
  const [session, setSession] = useState<EditorOpened | null>(null);
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [tool, setTool] = useState<ToolId>("rectangle");
  const [color, setColor] = useState<Color>(fromHex("#ff453a"));
  const [strokeWidth, setStrokeWidth] = useState(4);
  const [selected, setSelected] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const undoStack = useRef<Shape[][]>([]);
  const redoStack = useRef<Shape[][]>([]);

  // ── Session ───────────────────────────────────────────────────────────

  useEffect(() => {
    editorSession()
      .then((opened) => {
        setSession(opened);
        const img = new Image();
        // The base is served from disk over the asset protocol; a data URL for
        // a full-resolution capture is megabytes of base64.
        img.src = convertFileSrc(opened.path);
        img.onload = () => setImage(img);
        img.onerror = () => setError("Görsel yüklenemedi.");
      })
      .catch((e) => setError(String(e)));
  }, []);

  // ── History ───────────────────────────────────────────────────────────

  const commit = useCallback((next: Shape[]) => {
    undoStack.current.push(shapesRef.current);
    if (undoStack.current.length > HISTORY_LIMIT) undoStack.current.shift();
    redoStack.current = [];
    setShapes(renumberSteps(next));
  }, []);

  // A ref mirror so `commit` can capture the previous list without re-creating
  // itself on every shape change.
  const shapesRef = useRef<Shape[]>([]);
  useEffect(() => {
    shapesRef.current = shapes;
  }, [shapes]);

  const undo = useCallback(() => {
    const previous = undoStack.current.pop();
    if (!previous) return;
    redoStack.current.push(shapesRef.current);
    setShapes(previous);
    setSelected(null);
  }, []);

  const redo = useCallback(() => {
    const next = redoStack.current.pop();
    if (!next) return;
    undoStack.current.push(shapesRef.current);
    setShapes(next);
    setSelected(null);
  }, []);

  // ── Painting ──────────────────────────────────────────────────────────

  /** The shape a drag would produce right now, for live feedback. */
  const previewShape = useMemo<Shape | null>(() => {
    if (!drag || drag.movingIndex !== undefined) return null;
    return buildShape(drag, color, strokeWidth, nextStepNumber(shapes));
  }, [drag, color, strokeWidth, shapes]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || !image || !session) return;

    const painted = previewShape ? [...shapes, previewShape] : shapes;
    drawDocument(ctx, image, session.width, session.height, painted);

    if (selected !== null && shapes[selected]) {
      drawSelection(ctx, boundsOf(shapes[selected]), canvas.clientWidth / session.width);
    }
  }, [image, session, shapes, previewShape, selected]);

  // ── Pointer input ─────────────────────────────────────────────────────

  const toImageSpace = useCallback(
    (event: React.PointerEvent<HTMLCanvasElement>): Point => {
      const canvas = event.currentTarget;
      const rect = canvas.getBoundingClientRect();
      // The canvas is displayed scaled to fit; map back to image pixels.
      return {
        x: ((event.clientX - rect.left) / rect.width) * canvas.width,
        y: ((event.clientY - rect.top) / rect.height) * canvas.height,
      };
    },
    [],
  );

  const onPointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const point = toImageSpace(event);
    event.currentTarget.setPointerCapture(event.pointerId);

    if (tool === "select") {
      const index = [...shapes].reverse().findIndex((shape) => hitTest(shape, point));
      const actual = index === -1 ? null : shapes.length - 1 - index;
      setSelected(actual);
      if (actual !== null) {
        setDrag({
          tool,
          origin: point,
          current: point,
          movingIndex: actual,
          before: shapes,
        });
      }
      return;
    }

    if (tool === "step") {
      // A step is placed with a single click; there is nothing to drag.
      commit([...shapes, buildStep(point, color, nextStepNumber(shapes))]);
      return;
    }

    setDrag({
      tool,
      origin: point,
      current: point,
      points: tool === "freehand" ? [point] : undefined,
    });
  };

  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!drag) return;
    const point = toImageSpace(event);

    if (drag.movingIndex !== undefined) {
      const dx = point.x - drag.current.x;
      const dy = point.y - drag.current.y;
      setShapes((current) =>
        current.map((shape, i) => (i === drag.movingIndex ? translate(shape, dx, dy) : shape)),
      );
      setDrag({ ...drag, current: point });
      return;
    }

    setDrag({
      ...drag,
      current: point,
      points: drag.points ? [...drag.points, point] : undefined,
    });
  };

  const onPointerUp = () => {
    if (!drag) return;

    if (drag.movingIndex !== undefined) {
      // One history entry for the whole drag, holding the pre-move list.
      if (drag.before) {
        undoStack.current.push(drag.before);
        if (undoStack.current.length > HISTORY_LIMIT) undoStack.current.shift();
        redoStack.current = [];
      }
      setDrag(null);
      return;
    }

    const shape = buildShape(drag, color, strokeWidth, nextStepNumber(shapes));
    setDrag(null);
    if (shape) commit([...shapes, shape]);
  };

  // ── Keyboard ──────────────────────────────────────────────────────────

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;

      if (mod && event.key.toLowerCase() === "z") {
        event.preventDefault();
        event.shiftKey ? redo() : undo();
        return;
      }
      if (mod && event.key === "Enter") return;
      if (event.key === "Escape") {
        event.preventDefault();
        void closeEditor();
        return;
      }
      if (event.key === "Delete" || event.key === "Backspace") {
        if (selected === null) return;
        event.preventDefault();
        if (event.shiftKey) {
          commit([]);
        } else {
          commit(shapes.filter((_, i) => i !== selected));
        }
        setSelected(null);
        return;
      }
      if (mod) return;

      const match = TOOLS.find((t) => t.key === event.key.toLowerCase());
      if (match) {
        event.preventDefault();
        setTool(match.id);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [undo, redo, commit, shapes, selected]);

  // ── Export ────────────────────────────────────────────────────────────

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await editorExport({ shapes });
      await closeEditor();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }, [shapes]);

  if (error && !session) {
    return (
      <div className="editor editor--empty">
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      </div>
    );
  }

  return (
    <div className="editor">
      <header className="editor__toolbar">
        <button type="button" className="button" onClick={undo} aria-label="Geri al">
          Geri al
        </button>
        <button type="button" className="button" onClick={redo} aria-label="Yinele">
          Yinele
        </button>

        <span className="editor__divider" aria-hidden="true" />

        <label className="editor__field">
          <span className="visually-hidden">Renk</span>
          <input
            type="color"
            value={toHex(color)}
            onChange={(event) => setColor(fromHex(event.target.value))}
          />
        </label>

        <label className="editor__field">
          <span className="editor__field-label">Kalınlık</span>
          <input
            type="range"
            min={1}
            max={24}
            step={1}
            value={strokeWidth}
            onChange={(event) => setStrokeWidth(Number(event.target.value))}
          />
          <span className="editor__value">{strokeWidth}</span>
        </label>

        <div className="toolbar__spacer" />

        {session && (
          <span className="muted editor__size">
            {session.width} × {session.height}
          </span>
        )}
        <button type="button" className="button" onClick={() => void closeEditor()}>
          İptal
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => void save()}
          disabled={busy}
        >
          {busy ? "Kaydediliyor…" : "Bitti"}
        </button>
      </header>

      {error && session && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      <div className="editor__body">
        <nav className="editor__tools" aria-label="Araçlar">
          {TOOLS.map((item) => (
            <button
              key={item.id}
              type="button"
              className="editor__tool"
              aria-pressed={tool === item.id}
              title={`${item.label} (${item.key.toUpperCase()})`}
              onClick={() => setTool(item.id)}
            >
              <span className="editor__tool-label">{item.label}</span>
              <kbd className="kbd">{item.key.toUpperCase()}</kbd>
            </button>
          ))}
          <p className="editor__note">
            Metin ve konuşma balonu, font altyapısı gelene kadar devre dışı.
          </p>
        </nav>

        <div className="editor__canvas-wrap">
          {session && (
            <canvas
              ref={canvasRef}
              className="editor__canvas"
              width={session.width}
              height={session.height}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
            />
          )}
        </div>
      </div>
    </div>
  );
}

// ── Shape construction ──────────────────────────────────────────────────

function nextStepNumber(shapes: Shape[]): number {
  return shapes.filter((s) => s.kind === "step").length + 1;
}

function buildStep(center: Point, color: Color, number: number): Shape {
  return {
    kind: "step",
    center,
    radius: 16,
    number,
    fill: color,
    text_color: { r: 255, g: 255, b: 255, a: 255 },
  };
}

function buildShape(
  drag: Drag,
  color: Color,
  width: number,
  stepNumber: number,
): Shape | null {
  const stroke = { color, width };
  const rect = rectFromCorners(drag.origin, drag.current);

  // A click that produced no drag is a mis-click for the area tools, not a
  // zero-sized shape the user then has to find and delete.
  const tooSmall = rect.width < 2 && rect.height < 2;

  switch (drag.tool) {
    case "rectangle":
      return tooSmall
        ? null
        : { kind: "rectangle", rect, stroke, fill: TRANSPARENT, corner_radius: 0 };
    case "ellipse":
      return tooSmall ? null : { kind: "ellipse", rect, stroke, fill: TRANSPARENT };
    case "line":
      return tooSmall ? null : { kind: "line", from: drag.origin, to: drag.current, stroke };
    case "arrow":
      return tooSmall
        ? null
        : { kind: "arrow", from: drag.origin, to: drag.current, stroke, head: "end" };
    case "freehand": {
      const points = drag.points ?? [];
      return points.length < 2 ? null : { kind: "freehand", points, stroke };
    }
    case "highlight":
      return tooSmall
        ? null
        : { kind: "highlight", rect, color: { ...color, a: 90 } };
    case "blur":
      return tooSmall ? null : { kind: "blur", rect, radius: 12 };
    case "pixelate":
      return tooSmall ? null : { kind: "pixelate", rect, block: 12 };
    case "spotlight":
      return tooSmall ? null : { kind: "spotlight", rect, dim: 0.55 };
    case "step":
      return buildStep(drag.current, color, stepNumber);
    case "select":
      return null;
  }
}
