/**
 * Mirrors `crates/kestrel-editor/src/shape.rs`.
 *
 * The canvas draws a live preview from these values and Rust renders the file
 * that actually gets written, so both halves must agree on the shape of a
 * document exactly. Field names are the Rust ones — snake_case — because serde
 * serialises them verbatim.
 */
import { invoke } from "@tauri-apps/api/core";
import type { CaptureOutput } from "./ipc";

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Color {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface Stroke {
  color: Color;
  width: number;
}

export type ArrowHead = "none" | "end" | "both";

export type Shape =
  | { kind: "rectangle"; rect: Rect; stroke: Stroke; fill: Color; corner_radius: number }
  | { kind: "ellipse"; rect: Rect; stroke: Stroke; fill: Color }
  | { kind: "line"; from: Point; to: Point; stroke: Stroke }
  | { kind: "arrow"; from: Point; to: Point; stroke: Stroke; head: ArrowHead }
  | { kind: "freehand"; points: Point[]; stroke: Stroke }
  | {
      kind: "text";
      rect: Rect;
      content: string;
      color: Color;
      outline: Color;
      size: number;
      bold: boolean;
      italic: boolean;
    }
  | {
      kind: "speech_balloon";
      rect: Rect;
      tail: Point;
      content: string;
      stroke: Stroke;
      fill: Color;
      text_color: Color;
      size: number;
    }
  | {
      kind: "step";
      center: Point;
      radius: number;
      number: number;
      fill: Color;
      text_color: Color;
    }
  | { kind: "highlight"; rect: Rect; color: Color }
  | { kind: "blur"; rect: Rect; radius: number }
  | { kind: "pixelate"; rect: Rect; block: number }
  | { kind: "spotlight"; rect: Rect; dim: number };

export type Background =
  | { kind: "transparent" }
  | { kind: "solid"; color: Color }
  | { kind: "gradient"; from: Color; to: Color; angle: number };

export interface Shadow {
  color: Color;
  blur: number;
  offset_x: number;
  offset_y: number;
}

/**
 * Crop, padding, corners, shadow and background — ShareX's crop tool and image
 * beautifier combined, because they all change the size of the output rather
 * than drawing onto it.
 */
export interface Frame {
  crop: Rect | null;
  padding: number;
  corner_radius: number;
  shadow: Shadow | null;
  background: Background;
}

export const emptyFrame = (): Frame => ({
  crop: null,
  padding: 0,
  corner_radius: 0,
  shadow: null,
  background: { kind: "transparent" },
});

export const defaultShadow = (): Shadow => ({
  color: rgba(0, 0, 0, 110),
  blur: 24,
  offset_x: 0,
  offset_y: 12,
});

/** The document shape Rust's serde expects. History is not part of the file. */
export interface EditorDocument {
  shapes: Shape[];
  frame: Frame;
}

/** Output size for a frame, mirroring `Frame::output_size`. */
export function frameOutputSize(frame: Frame, source: [number, number]): [number, number] {
  const [w, h] = frame.crop
    ? [Math.max(frame.crop.width, 1), Math.max(frame.crop.height, 1)]
    : source;
  const pad = Math.max(frame.padding, 0) * 2;
  return [Math.round(w + pad), Math.round(h + pad)];
}

export interface EditorOpened {
  path: string;
  width: number;
  height: number;
  /** Bumped whenever Rust rewrites the staged file, to bust the webview cache. */
  revision: number;
}

export const openEditor = () => invoke<EditorOpened>("open_editor");
export const editorSession = () => invoke<EditorOpened>("editor_session");
export const closeEditor = () => invoke<void>("close_editor");
export const editorExport = (document: EditorDocument) =>
  invoke<CaptureOutput>("editor_export", { document: JSON.stringify(document) });

// ── Image effects ───────────────────────────────────────────────────────

/** Mirrors `kestrel_editor::Effect`; the tag is serde's `kind`. */
export type Effect =
  | { kind: "resize"; width: number; height: number; keep_aspect: boolean }
  | { kind: "rotate"; rotation: Rotation }
  | { kind: "flip"; horizontal: boolean; vertical: boolean }
  | { kind: "auto_crop"; tolerance: number }
  | { kind: "brightness"; amount: number }
  | { kind: "contrast"; amount: number }
  | { kind: "gamma"; value: number }
  | { kind: "saturation"; amount: number }
  | { kind: "opacity"; amount: number }
  | { kind: "grayscale" }
  | { kind: "sepia" }
  | { kind: "invert" }
  | { kind: "blur"; radius: number }
  | { kind: "sharpen"; amount: number }
  | { kind: "pixelate"; block: number }
  | { kind: "border"; width: number; color: Color };

export type Rotation = "none" | "quarter" | "half" | "three_quarters";

/** Kinds that move pixels — mirrors `Effect::changes_geometry`. */
const GEOMETRIC = new Set(["resize", "rotate", "flip", "auto_crop", "border"]);

export const changesGeometry = (effects: Effect[]) =>
  effects.some((effect) => GEOMETRIC.has(effect.kind));

export const editorSetEffects = (effects: Effect[], annotationCount: number) =>
  invoke<EditorOpened>("editor_set_effects", { effects, annotationCount });

export interface SxiePreset {
  name: string | null;
  effects: Effect[];
  /** Effects in the file Kestrel has no equivalent for. */
  unsupported: string[];
}

export const importSxie = (path: string) => invoke<SxiePreset>("import_sxie", { path });

/** A new effect of each kind, with the values ShareX defaults to. */
export function defaultEffect(kind: Effect["kind"], size: [number, number]): Effect {
  switch (kind) {
    case "resize":
      return { kind, width: size[0], height: size[1], keep_aspect: true };
    case "rotate":
      return { kind, rotation: "quarter" };
    case "flip":
      return { kind, horizontal: true, vertical: false };
    case "auto_crop":
      return { kind, tolerance: 10 };
    case "brightness":
    case "contrast":
    case "saturation":
      return { kind, amount: 0.2 };
    case "gamma":
      return { kind, value: 1.2 };
    case "opacity":
      return { kind, amount: 0.8 };
    case "blur":
      return { kind, radius: 6 };
    case "sharpen":
      return { kind, amount: 0.5 };
    case "pixelate":
      return { kind, block: 10 };
    case "border":
      return { kind, width: 4, color: rgba(0, 0, 0) };
    default:
      return { kind } as Effect;
  }
}

// ── Colour helpers ──────────────────────────────────────────────────────

export const rgba = (r: number, g: number, b: number, a = 255): Color => ({ r, g, b, a });
export const TRANSPARENT: Color = rgba(0, 0, 0, 0);

export const toCss = (c: Color) => `rgba(${c.r}, ${c.g}, ${c.b}, ${(c.a / 255).toFixed(3)})`;

export function fromHex(hex: string): Color {
  const value = hex.replace("#", "");
  return rgba(
    parseInt(value.slice(0, 2), 16),
    parseInt(value.slice(2, 4), 16),
    parseInt(value.slice(4, 6), 16),
  );
}

export const toHex = (c: Color) =>
  `#${[c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;

// ── Geometry ────────────────────────────────────────────────────────────

export function rectFromCorners(a: Point, b: Point): Rect {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  };
}

/** Mirrors `Shape::bounds` closely enough for hit testing and selection UI. */
export function boundsOf(shape: Shape): Rect {
  switch (shape.kind) {
    case "rectangle":
    case "ellipse": {
      const half = shape.stroke.width / 2;
      return {
        x: shape.rect.x - half,
        y: shape.rect.y - half,
        width: shape.rect.width + shape.stroke.width,
        height: shape.rect.height + shape.stroke.width,
      };
    }
    case "line":
      return rectFromCorners(shape.from, shape.to);
    case "arrow": {
      const pad = shape.stroke.width * 3;
      const base = rectFromCorners(shape.from, shape.to);
      return {
        x: base.x - pad,
        y: base.y - pad,
        width: base.width + pad * 2,
        height: base.height + pad * 2,
      };
    }
    case "freehand": {
      if (shape.points.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
      const xs = shape.points.map((p) => p.x);
      const ys = shape.points.map((p) => p.y);
      return {
        x: Math.min(...xs),
        y: Math.min(...ys),
        width: Math.max(...xs) - Math.min(...xs),
        height: Math.max(...ys) - Math.min(...ys),
      };
    }
    case "step":
      return {
        x: shape.center.x - shape.radius,
        y: shape.center.y - shape.radius,
        width: shape.radius * 2,
        height: shape.radius * 2,
      };
    default:
      return shape.rect;
  }
}

export function rectContains(rect: Rect, p: Point): boolean {
  return (
    p.x >= rect.x && p.x <= rect.x + rect.width && p.y >= rect.y && p.y <= rect.y + rect.height
  );
}

export function hitTest(shape: Shape, p: Point): boolean {
  if (shape.kind === "step") {
    return Math.hypot(shape.center.x - p.x, shape.center.y - p.y) <= shape.radius;
  }
  if (shape.kind === "freehand") {
    const tolerance = Math.max(shape.stroke.width, 6);
    return shape.points.some((point) => Math.hypot(point.x - p.x, point.y - p.y) <= tolerance);
  }
  return rectContains(boundsOf(shape), p);
}

export function translate(shape: Shape, dx: number, dy: number): Shape {
  const movePoint = (p: Point): Point => ({ x: p.x + dx, y: p.y + dy });
  const moveRect = (r: Rect): Rect => ({ ...r, x: r.x + dx, y: r.y + dy });

  switch (shape.kind) {
    case "line":
      return { ...shape, from: movePoint(shape.from), to: movePoint(shape.to) };
    case "arrow":
      return { ...shape, from: movePoint(shape.from), to: movePoint(shape.to) };
    case "freehand":
      return { ...shape, points: shape.points.map(movePoint) };
    case "step":
      return { ...shape, center: movePoint(shape.center) };
    case "speech_balloon":
      return { ...shape, rect: moveRect(shape.rect), tail: movePoint(shape.tail) };
    default:
      return { ...shape, rect: moveRect(shape.rect) };
  }
}

/**
 * Move and scale a shape into another coordinate space.
 *
 * The selection overlay works in logical points while the captured image is in
 * physical pixels, so on a Retina display every shape has to be scaled by the
 * display's factor as well as shifted by the crop origin. Getting this wrong
 * puts annotations at half position and half size, which is exactly the kind of
 * bug that only shows up on one machine.
 */
export function transformShape(shape: Shape, dx: number, dy: number, scale: number): Shape {
  const point = (p: Point): Point => ({ x: (p.x + dx) * scale, y: (p.y + dy) * scale });
  const rect = (r: Rect): Rect => ({
    x: (r.x + dx) * scale,
    y: (r.y + dy) * scale,
    width: r.width * scale,
    height: r.height * scale,
  });

  switch (shape.kind) {
    case "line":
      return { ...shape, from: point(shape.from), to: point(shape.to), stroke: scaleStroke(shape.stroke, scale) };
    case "arrow":
      return { ...shape, from: point(shape.from), to: point(shape.to), stroke: scaleStroke(shape.stroke, scale) };
    case "freehand":
      return { ...shape, points: shape.points.map(point), stroke: scaleStroke(shape.stroke, scale) };
    case "step":
      return { ...shape, center: point(shape.center), radius: shape.radius * scale };
    case "rectangle":
      return { ...shape, rect: rect(shape.rect), stroke: scaleStroke(shape.stroke, scale), corner_radius: shape.corner_radius * scale };
    case "ellipse":
      return { ...shape, rect: rect(shape.rect), stroke: scaleStroke(shape.stroke, scale) };
    case "blur":
      return { ...shape, rect: rect(shape.rect), radius: shape.radius * scale };
    case "text":
      return { ...shape, rect: rect(shape.rect), size: shape.size * scale };
    case "speech_balloon":
      return { ...shape, rect: rect(shape.rect), tail: point(shape.tail), size: shape.size * scale, stroke: scaleStroke(shape.stroke, scale) };
    case "pixelate":
      // Block size scales too, or a redaction becomes finer — and therefore
      // more readable — on a high-density display.
      return { ...shape, rect: rect(shape.rect), block: Math.max(2, Math.round(shape.block * scale)) };
    default:
      return { ...shape, rect: rect(shape.rect) };
  }
}

const scaleStroke = (stroke: Stroke, scale: number): Stroke => ({
  ...stroke,
  width: stroke.width * scale,
});

/** Step callouts are renumbered 1..n so deleting one leaves no gap. */
export function renumberSteps(shapes: Shape[]): Shape[] {
  let next = 1;
  return shapes.map((shape) =>
    shape.kind === "step" ? { ...shape, number: next++ } : shape,
  );
}
