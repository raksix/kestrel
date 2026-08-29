import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  boundsOf,
  changesGeometry,
  closeEditor,
  defaultEffect,
  defaultShadow,
  emptyFrame,
  frameOutputSize,
  editorExport,
  editorSession,
  editorSetEffects,
  fromHex,
  importSxie,
  hitTest,
  rectFromCorners,
  renumberSteps,
  toHex,
  translate,
  TRANSPARENT,
  type Background,
  type Color,
  type EditorOpened,
  type Effect,
  type Frame,
  type Point,
  type Shape,
} from "../../lib/editorTypes";
import { imageFromEvent, placeAt } from "../../lib/paste";
import { drawDocument, drawSelection, setImageReadyHandler } from "./canvas";
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
  { id: "text", key: "t", label: "Metin" },
  { id: "step", key: "n", label: "Adım" },
  { id: "crop", key: "c", label: "Kırp" },
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
  /** Index of the text shape currently being typed into, if any. */
  const [editing, setEditing] = useState<number | null>(null);
  const [frame, setFrame] = useState<Frame>(emptyFrame());
  const [effects, setEffects] = useState<Effect[]>([]);
  const [effectsBusy, setEffectsBusy] = useState(false);
  const [sxieNote, setSxieNote] = useState<string | null>(null);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textInputRef = useRef<HTMLTextAreaElement>(null);
  const undoStack = useRef<Shape[][]>([]);
  const redoStack = useRef<Shape[][]>([]);
  const frameUndoStack = useRef<Frame[]>([]);
  const frameRedoStack = useRef<Frame[]>([]);

  // ── Session ───────────────────────────────────────────────────────────

  const loadBase = useCallback((opened: EditorOpened) => {
    setSession(opened);
    const img = new Image();
    // The base is served from disk over the asset protocol; a data URL for
    // a full-resolution capture is megabytes of base64. The revision is a
    // cache buster — the path stays the same when effects rewrite the file.
    img.src = `${convertFileSrc(opened.path)}?r=${opened.revision}`;
    img.onload = () => setImage(img);
    img.onerror = () => setError("Görsel yüklenemedi.");
  }, []);

  useEffect(() => {
    editorSession()
      .then(loadBase)
      .catch((e) => setError(String(e)));
  }, [loadBase]);

  // ── Effects ───────────────────────────────────────────────────────────

  const applyEffects = useCallback(
    (next: Effect[]) => {
      // Rust owns this: the chain is applied to the untouched original, so a
      // removed effect really is undone rather than approximated.
      //
      // The list is only updated once Rust accepts it. Showing the effect
      // first would leave the panel claiming a blur that the image does not
      // have when the call is refused.
      setEffectsBusy(true);
      editorSetEffects(next, shapesRef.current.length)
        .then((opened) => {
          setError(null);
          setEffects(next);
          loadBase(opened);
        })
        .catch((e) => setError(String(e)))
        .finally(() => setEffectsBusy(false));
    },
    [loadBase],
  );

  const importPreset = useCallback(async () => {
    const chosen = await openDialog({
      filters: [{ name: "ShareX görsel efektleri", extensions: ["sxie", "json"] }],
    });
    if (!chosen || Array.isArray(chosen)) return;

    try {
      const preset = await importSxie(chosen);
      // Say what was left out. The `.sxie` format is not documented, so a
      // preset can legitimately contain effects Kestrel has no equivalent for
      // — and quietly applying the rest would be the wrong kind of helpful.
      setSxieNote(
        preset.unsupported.length > 0
          ? `${preset.unsupported.length} efekt aktarılamadı: ${preset.unsupported.join(", ")}`
          : null,
      );
      applyEffects([...effects, ...preset.effects]);
    } catch (e) {
      setError(String(e));
    }
  }, [applyEffects, effects]);

  // ── History ───────────────────────────────────────────────────────────

  const commit = useCallback((next: Shape[]) => {
    undoStack.current.push(shapesRef.current);
    frameUndoStack.current.push(frameRef.current);
    if (undoStack.current.length > HISTORY_LIMIT) {
      undoStack.current.shift();
      frameUndoStack.current.shift();
    }
    redoStack.current = [];
    frameRedoStack.current = [];
    setShapes(renumberSteps(next));
  }, []);

  // A ref mirror so `commit` can capture the previous list without re-creating
  // itself on every shape change.
  const shapesRef = useRef<Shape[]>([]);
  useEffect(() => {
    shapesRef.current = shapes;
  }, [shapes]);

  const frameRef = useRef<Frame>(emptyFrame());
  useEffect(() => {
    frameRef.current = frame;
  }, [frame]);

  // ── Paste and drop ────────────────────────────────────────────────────

  useEffect(() => {
    const accept = async (event: ClipboardEvent | DragEvent) => {
      if (editing !== null) return; // The textarea owns the paste while it is open.
      const image = await imageFromEvent(event);
      if (!image || !session) return;
      event.preventDefault();

      commit([
        ...shapesRef.current,
        {
          kind: "image",
          rect: placeAt(
            image,
            { x: session.width / 2, y: session.height / 2 },
            { width: session.width, height: session.height },
          ),
          data: image.data,
          opacity: 1,
        },
      ]);
    };

    const onPaste = (event: ClipboardEvent) => void accept(event);
    const onDrop = (event: DragEvent) => void accept(event);
    const onDragOver = (event: DragEvent) => event.preventDefault();

    window.addEventListener("paste", onPaste);
    window.addEventListener("drop", onDrop);
    window.addEventListener("dragover", onDragOver);
    return () => {
      window.removeEventListener("paste", onPaste);
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragover", onDragOver);
    };
  }, [commit, editing, session]);

  const undo = useCallback(() => {
    const previous = undoStack.current.pop();
    if (!previous) return;
    redoStack.current.push(shapesRef.current);
    setShapes(previous);

    const previousFrame = frameUndoStack.current.pop();
    if (previousFrame) {
      frameRedoStack.current.push(frameRef.current);
      setFrame(previousFrame);
    }
    setSelected(null);
  }, []);

  const redo = useCallback(() => {
    const next = redoStack.current.pop();
    if (!next) return;
    undoStack.current.push(shapesRef.current);
    setShapes(next);

    const nextFrame = frameRedoStack.current.pop();
    if (nextFrame) {
      frameUndoStack.current.push(frameRef.current);
      setFrame(nextFrame);
    }
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
    drawDocument(ctx, image, session.width, session.height, painted, frame);

    // A pasted image decodes asynchronously, so the paint right after a paste
    // has nothing to draw for it yet.
    setImageReadyHandler(() =>
      drawDocument(ctx, image, session.width, session.height, painted, frame),
    );

    if (selected !== null && shapes[selected]) {
      // Selection chrome is drawn in output space, so it has to move with the
      // crop and padding rather than staying in base image coordinates.
      const bounds = boundsOf(shapes[selected]);
      drawSelection(
        ctx,
        {
          ...bounds,
          x: bounds.x - (frame.crop?.x ?? 0) + frame.padding,
          y: bounds.y - (frame.crop?.y ?? 0) + frame.padding,
        },
        canvas.clientWidth / canvas.width,
      );
    }
  }, [image, session, shapes, previewShape, selected, frame]);

  const [outputWidth, outputHeight] = useMemo(
    () => (session ? frameOutputSize(frame, [session.width, session.height]) : [0, 0]),
    [frame, session],
  );

  /** Frame edits share the undo history with annotations, as in Rust. */
  const applyFrame = useCallback(
    (next: Frame) => {
      undoStack.current.push(shapesRef.current);
      frameUndoStack.current.push(frame);
      redoStack.current = [];
      frameRedoStack.current = [];
      setFrame(next);
    },
    [frame],
  );

  // ── Pointer input ─────────────────────────────────────────────────────

  const toImageSpace = useCallback(
    (event: React.PointerEvent<HTMLCanvasElement>): Point => {
      const canvas = event.currentTarget;
      const rect = canvas.getBoundingClientRect();
      // The canvas shows output space; map back through the frame into base
      // image pixels, which is where shapes live.
      const outX = ((event.clientX - rect.left) / rect.width) * canvas.width;
      const outY = ((event.clientY - rect.top) / rect.height) * canvas.height;
      return {
        x: outX - frame.padding + (frame.crop?.x ?? 0),
        y: outY - frame.padding + (frame.crop?.y ?? 0),
      };
    },
    [frame],
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

    if (tool === "crop") {
      setDrag({ tool, origin: point, current: point });
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

    if (drag.tool === "crop") {
      const rect = rectFromCorners(drag.origin, drag.current);
      setDrag(null);
      // A stray click should not crop the image down to nothing.
      if (rect.width > 8 && rect.height > 8) {
        applyFrame({ ...frame, crop: rect });
      }
      return;
    }

    const shape = buildShape(drag, color, strokeWidth, nextStepNumber(shapes));
    setDrag(null);
    if (!shape) return;

    commit([...shapes, shape]);
    if (shape.kind === "text") {
      // Nothing has been typed yet; open the inline editor straight away
      // rather than leaving an invisible empty box on the canvas.
      setEditing(shapes.length);
    }
  };

  /** Finish typing: keep the text, or drop the shape if nothing was entered. */
  const commitText = useCallback(
    (value: string) => {
      if (editing === null) return;
      const index = editing;
      setEditing(null);

      setShapes((current) => {
        const shape = current[index];
        if (!shape || shape.kind !== "text") return current;
        if (value.trim() === "") {
          return renumberSteps(current.filter((_, i) => i !== index));
        }
        return current.map((s, i) =>
          i === index && s.kind === "text" ? { ...s, content: value } : s,
        );
      });
    },
    [editing],
  );

  // ── Keyboard ──────────────────────────────────────────────────────────

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // While a text box has focus every key belongs to it.
      if (editing !== null) return;
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
  }, [undo, redo, commit, shapes, selected, editing]);

  useEffect(() => {
    if (editing !== null) textInputRef.current?.focus();
  }, [editing]);

  // ── Export ────────────────────────────────────────────────────────────

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await editorExport({ shapes, frame });
      await closeEditor();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }, [shapes, frame]);

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
            {outputWidth} × {outputHeight}
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
            Metin: sürükle, yaz, Enter ile bitir. Shift+Enter satır ekler.
          </p>
          <FramePanel frame={frame} onChange={applyFrame} />
          <EffectsPanel
            effects={effects}
            busy={effectsBusy}
            locked={shapes.length > 0}
            note={sxieNote}
            size={session ? [session.width, session.height] : [0, 0]}
            onChange={applyEffects}
            onImport={importPreset}
          />
        </nav>

        <div className="editor__canvas-wrap">
          {session && (
            <canvas
              ref={canvasRef}
              className="editor__canvas"
              width={outputWidth}
              height={outputHeight}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
            />
          )}
          {editing !== null && session && (
            <TextBox
              inputRef={textInputRef}
              shape={shapes[editing]}
              canvas={canvasRef.current}
              imageWidth={session.width}
              onCommit={commitText}
            />
          )}
        </div>
      </div>
    </div>
  );
}

/** The effect kinds offered, grouped as ShareX groups them. */
const EFFECT_GROUPS: { label: string; kinds: [Effect["kind"], string][] }[] = [
  {
    label: "Boyut",
    kinds: [
      ["resize", "Yeniden boyutlandır"],
      ["rotate", "Döndür"],
      ["flip", "Aynala"],
      ["auto_crop", "Otomatik kırp"],
    ],
  },
  {
    label: "Ayar",
    kinds: [
      ["brightness", "Parlaklık"],
      ["contrast", "Kontrast"],
      ["gamma", "Gama"],
      ["saturation", "Doygunluk"],
      ["opacity", "Saydamlık"],
    ],
  },
  {
    label: "Filtre",
    kinds: [
      ["grayscale", "Gri tonlama"],
      ["sepia", "Sepya"],
      ["invert", "Tersle"],
      ["blur", "Bulanıklaştır"],
      ["sharpen", "Keskinleştir"],
      ["pixelate", "Pikselleştir"],
    ],
  },
  { label: "Çizim", kinds: [["border", "Kenarlık"]] },
];

const EFFECT_LABELS = new Map(EFFECT_GROUPS.flatMap((group) => group.kinds));

/**
 * ShareX's image effect chain.
 *
 * The order is the point — a blur then a border is not the same picture as a
 * border then a blur — so the list is reorderable rather than a set of toggles.
 */
function EffectsPanel({
  effects,
  busy,
  locked,
  note,
  size,
  onChange,
  onImport,
}: {
  effects: Effect[];
  busy: boolean;
  /** True once annotations exist, which rules out geometry-changing effects. */
  locked: boolean;
  note: string | null;
  size: [number, number];
  onChange: (effects: Effect[]) => void;
  onImport: () => void;
}) {
  const replace = (index: number, effect: Effect) =>
    onChange(effects.map((existing, i) => (i === index ? effect : existing)));

  const move = (index: number, by: number) => {
    const to = index + by;
    if (to < 0 || to >= effects.length) return;
    const next = [...effects];
    [next[index], next[to]] = [next[to], next[index]];
    onChange(next);
  };

  return (
    <section className="editor__frame">
      <h2 className="editor__frame-title">Efektler</h2>

      {locked && (
        <p className="editor__note">
          Çizim varken boyut değiştiren efektler kapalı — resim kayarsa
          işaretlemeler yanlış yere düşer.
        </p>
      )}

      <label className="editor__frame-field">
        <span>Ekle</span>
        <select
          className="input"
          value=""
          disabled={busy}
          onChange={(e) => {
            if (!e.target.value) return;
            onChange([...effects, defaultEffect(e.target.value as Effect["kind"], size)]);
            e.target.value = "";
          }}
        >
          <option value="">Efekt seç…</option>
          {EFFECT_GROUPS.map((group) => (
            <optgroup key={group.label} label={group.label}>
              {group.kinds.map(([kind, label]) => (
                <option
                  key={kind}
                  value={kind}
                  disabled={locked && changesGeometry([{ kind } as Effect])}
                >
                  {label}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>

      <ol className="editor__effects">
        {effects.map((effect, index) => (
          <li key={`${effect.kind}-${index}`} className="editor__effect">
            <div className="editor__effect-head">
              <span className="editor__effect-name">
                {index + 1}. {EFFECT_LABELS.get(effect.kind) ?? effect.kind}
              </span>
              <button
                type="button"
                className="button button--icon"
                aria-label="Yukarı taşı"
                disabled={busy || index === 0}
                onClick={() => move(index, -1)}
              >
                ↑
              </button>
              <button
                type="button"
                className="button button--icon"
                aria-label="Aşağı taşı"
                disabled={busy || index === effects.length - 1}
                onClick={() => move(index, 1)}
              >
                ↓
              </button>
              <button
                type="button"
                className="button button--icon"
                aria-label="Kaldır"
                disabled={busy}
                onClick={() => onChange(effects.filter((_, i) => i !== index))}
              >
                ✕
              </button>
            </div>
            <EffectControls
              effect={effect}
              disabled={busy}
              onChange={(next) => replace(index, next)}
            />
          </li>
        ))}
      </ol>

      {effects.length > 0 && (
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => onChange([])}
        >
          Tümünü kaldır
        </button>
      )}

      <button type="button" className="button" disabled={busy} onClick={onImport}>
        .sxie içe aktar…
      </button>
      {note && <p className="editor__note">{note}</p>}
    </section>
  );
}

/** The one control each effect needs, or nothing for the ones with no options. */
function EffectControls({
  effect,
  disabled,
  onChange,
}: {
  effect: Effect;
  disabled: boolean;
  onChange: (effect: Effect) => void;
}) {
  const slider = (
    label: string,
    value: number,
    min: number,
    max: number,
    step: number,
    apply: (value: number) => Effect,
  ) => (
    <label className="editor__frame-field">
      <span>{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(apply(Number(e.target.value)))}
      />
      <span className="editor__value">{value}</span>
    </label>
  );

  switch (effect.kind) {
    case "brightness":
    case "contrast":
    case "saturation":
      return slider("Miktar", effect.amount, -1, 1, 0.05, (amount) => ({ ...effect, amount }));
    case "opacity":
      return slider("Miktar", effect.amount, 0, 1, 0.05, (amount) => ({ ...effect, amount }));
    case "gamma":
      return slider("Değer", effect.value, 0.1, 3, 0.05, (value) => ({ ...effect, value }));
    case "blur":
      return slider("Yarıçap", effect.radius, 1, 40, 1, (radius) => ({ ...effect, radius }));
    case "sharpen":
      return slider("Miktar", effect.amount, 0, 2, 0.05, (amount) => ({ ...effect, amount }));
    case "pixelate":
      return slider("Blok", effect.block, 2, 64, 1, (block) => ({ ...effect, block }));
    case "auto_crop":
      return slider("Tolerans", effect.tolerance, 0, 64, 1, (tolerance) => ({
        ...effect,
        tolerance,
      }));

    case "resize":
      return (
        <>
          <label className="editor__frame-field">
            <span>Genişlik</span>
            <input
              type="number"
              className="input"
              min={1}
              value={effect.width}
              disabled={disabled}
              onChange={(e) =>
                onChange({ ...effect, width: Math.max(1, Number(e.target.value)) })
              }
            />
          </label>
          <label className="editor__frame-field">
            <span>Yükseklik</span>
            <input
              type="number"
              className="input"
              min={1}
              value={effect.height}
              disabled={disabled}
              onChange={(e) =>
                onChange({ ...effect, height: Math.max(1, Number(e.target.value)) })
              }
            />
          </label>
          <label className="editor__frame-check">
            <input
              type="checkbox"
              checked={effect.keep_aspect}
              disabled={disabled}
              onChange={(e) => onChange({ ...effect, keep_aspect: e.target.checked })}
            />
            <span>Oranı koru</span>
          </label>
        </>
      );

    case "rotate":
      return (
        <label className="editor__frame-field">
          <span>Açı</span>
          <select
            className="input"
            value={effect.rotation}
            disabled={disabled}
            onChange={(e) =>
              onChange({ ...effect, rotation: e.target.value as typeof effect.rotation })
            }
          >
            <option value="none">0°</option>
            <option value="quarter">90°</option>
            <option value="half">180°</option>
            <option value="three_quarters">270°</option>
          </select>
        </label>
      );

    case "flip":
      return (
        <>
          <label className="editor__frame-check">
            <input
              type="checkbox"
              checked={effect.horizontal}
              disabled={disabled}
              onChange={(e) => onChange({ ...effect, horizontal: e.target.checked })}
            />
            <span>Yatay</span>
          </label>
          <label className="editor__frame-check">
            <input
              type="checkbox"
              checked={effect.vertical}
              disabled={disabled}
              onChange={(e) => onChange({ ...effect, vertical: e.target.checked })}
            />
            <span>Dikey</span>
          </label>
        </>
      );

    case "border":
      return (
        <>
          {slider("Kalınlık", effect.width, 0, 40, 1, (width) => ({ ...effect, width }))}
          <label className="editor__frame-field">
            <span>Renk</span>
            <input
              type="color"
              value={toHex(effect.color)}
              disabled={disabled}
              onChange={(e) => onChange({ ...effect, color: fromHex(e.target.value) })}
            />
          </label>
        </>
      );

    default:
      // Grayscale, sepia and invert have nothing to configure.
      return null;
  }
}

/** ShareX's image beautifier: padding, corners, shadow and background. */
function FramePanel({
  frame,
  onChange,
}: {
  frame: Frame;
  onChange: (frame: Frame) => void;
}) {
  const background = frame.background.kind;

  return (
    <section className="editor__frame">
      <h2 className="editor__frame-title">Çerçeve</h2>

      {frame.crop && (
        <button
          type="button"
          className="button"
          onClick={() => onChange({ ...frame, crop: null })}
        >
          Kırpmayı kaldır
        </button>
      )}

      <label className="editor__frame-field">
        <span>Boşluk</span>
        <input
          type="range"
          min={0}
          max={160}
          step={4}
          value={frame.padding}
          onChange={(event) => onChange({ ...frame, padding: Number(event.target.value) })}
        />
      </label>

      <label className="editor__frame-field">
        <span>Köşe</span>
        <input
          type="range"
          min={0}
          max={64}
          step={2}
          value={frame.corner_radius}
          onChange={(event) =>
            onChange({ ...frame, corner_radius: Number(event.target.value) })
          }
        />
      </label>

      <label className="editor__frame-check">
        <input
          type="checkbox"
          checked={frame.shadow !== null}
          onChange={(event) =>
            onChange({ ...frame, shadow: event.target.checked ? defaultShadow() : null })
          }
        />
        <span>Gölge</span>
      </label>

      <label className="editor__frame-field">
        <span>Zemin</span>
        <select
          value={background}
          onChange={(event) => onChange({ ...frame, background: makeBackground(event.target.value) })}
        >
          <option value="transparent">Şeffaf</option>
          <option value="solid">Düz renk</option>
          <option value="gradient">Gradyan</option>
        </select>
      </label>

      {frame.background.kind === "solid" && (
        <input
          type="color"
          aria-label="Zemin rengi"
          value={toHex(frame.background.color)}
          onChange={(event) =>
            onChange({
              ...frame,
              background: { kind: "solid", color: fromHex(event.target.value) },
            })
          }
        />
      )}
    </section>
  );
}

function makeBackground(kind: string): Background {
  switch (kind) {
    case "solid":
      return { kind: "solid", color: fromHex("#f2f2f0") };
    case "gradient":
      return { kind: "gradient", from: fromHex("#5a9bf0"), to: fromHex("#a06bf0"), angle: 45 };
    default:
      return { kind: "transparent" };
  }
}

/**
 * The inline text box, positioned over the shape it edits.
 *
 * Typing happens in a real textarea rather than on the canvas so that input
 * methods, selection, autocorrect and accessibility all keep working — none of
 * which a hand-rolled canvas caret would give us.
 */
function TextBox({
  inputRef,
  shape,
  canvas,
  imageWidth,
  onCommit,
}: {
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
  shape: Shape | undefined;
  canvas: HTMLCanvasElement | null;
  imageWidth: number;
  onCommit: (value: string) => void;
}) {
  const [value, setValue] = useState(shape?.kind === "text" ? shape.content : "");

  if (!shape || shape.kind !== "text" || !canvas) return null;

  // The canvas is displayed scaled to fit, so the box has to scale with it.
  const scale = canvas.clientWidth / imageWidth;

  return (
    <textarea
      ref={inputRef}
      className="editor__text-input"
      value={value}
      spellCheck={false}
      style={{
        left: canvas.offsetLeft + shape.rect.x * scale,
        top: canvas.offsetTop + shape.rect.y * scale,
        width: Math.max(shape.rect.width * scale, 80),
        height: Math.max(shape.rect.height * scale, shape.size * scale * 1.4),
        fontSize: shape.size * scale,
        color: `rgb(${shape.color.r}, ${shape.color.g}, ${shape.color.b})`,
      }}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => onCommit(value)}
      onKeyDown={(event) => {
        // Enter commits; Shift+Enter adds a line, as in every annotation tool.
        if (event.key === "Enter" && !event.shiftKey) {
          event.preventDefault();
          onCommit(value);
        }
        if (event.key === "Escape") {
          event.preventDefault();
          onCommit("");
        }
      }}
    />
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
    case "text":
      return {
        kind: "text",
        // A click with no drag still gets a usable box.
        rect: tooSmall
          ? { x: drag.origin.x, y: drag.origin.y, width: 180, height: width * 6 }
          : rect,
        content: "",
        color,
        outline: { r: 0, g: 0, b: 0, a: 200 },
        size: Math.max(width * 5, 16),
        bold: false,
        italic: false,
      };
    case "step":
      return buildStep(drag.current, color, stepNumber);
    // Handled before this point: they change the frame, not the shape list.
    case "crop":
    case "select":
      return null;
  }
}
