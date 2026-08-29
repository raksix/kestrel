/**
 * Canvas 2D preview of an annotation document.
 *
 * This is a *preview*. The file Kestrel writes is rendered in Rust
 * (`crates/kestrel-editor/src/render.rs`) so that an export is identical on
 * every platform and is not capped at screen resolution. The two renderers
 * are kept visually close, but where they differ the Rust one is correct.
 */
import {
  toCss,
  type Color,
  type Point,
  type Rect,
  type Shape,
  type Stroke,
} from "../../lib/editorTypes";

/** Draw the base image and every annotation, in painting order. */
export function drawDocument(
  ctx: CanvasRenderingContext2D,
  base: CanvasImageSource,
  width: number,
  height: number,
  shapes: Shape[],
): void {
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(base, 0, 0, width, height);
  for (const shape of shapes) {
    drawShape(ctx, shape, width, height);
  }
}

function drawShape(
  ctx: CanvasRenderingContext2D,
  shape: Shape,
  canvasWidth: number,
  canvasHeight: number,
): void {
  ctx.save();

  switch (shape.kind) {
    case "rectangle": {
      path(ctx, () => roundedRect(ctx, shape.rect, shape.corner_radius));
      if (shape.fill.a > 0) {
        ctx.fillStyle = toCss(shape.fill);
        ctx.fill();
      }
      strokePath(ctx, shape.stroke);
      break;
    }
    case "ellipse": {
      path(ctx, () => {
        const r = shape.rect;
        ctx.ellipse(
          r.x + r.width / 2,
          r.y + r.height / 2,
          Math.max(r.width / 2, 0.01),
          Math.max(r.height / 2, 0.01),
          0,
          0,
          Math.PI * 2,
        );
      });
      if (shape.fill.a > 0) {
        ctx.fillStyle = toCss(shape.fill);
        ctx.fill();
      }
      strokePath(ctx, shape.stroke);
      break;
    }
    case "line": {
      path(ctx, () => {
        ctx.moveTo(shape.from.x, shape.from.y);
        ctx.lineTo(shape.to.x, shape.to.y);
      });
      strokePath(ctx, shape.stroke);
      break;
    }
    case "arrow": {
      drawArrow(ctx, shape);
      break;
    }
    case "freehand": {
      if (shape.points.length === 0) break;
      path(ctx, () => {
        ctx.moveTo(shape.points[0].x, shape.points[0].y);
        for (const point of shape.points.slice(1)) {
          ctx.lineTo(point.x, point.y);
        }
      });
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      strokePath(ctx, shape.stroke);
      break;
    }
    case "highlight": {
      ctx.fillStyle = toCss(shape.color);
      ctx.fillRect(shape.rect.x, shape.rect.y, shape.rect.width, shape.rect.height);
      break;
    }
    case "blur": {
      filterRegion(ctx, shape.rect, `blur(${Math.max(shape.radius, 1)}px)`);
      break;
    }
    case "pixelate": {
      pixelateRegion(ctx, shape.rect, Math.max(shape.block, 2));
      break;
    }
    case "spotlight": {
      // Dim everything outside, drawn as four bands so no compositing mode is
      // needed and the inside is left untouched.
      ctx.fillStyle = `rgba(0, 0, 0, ${Math.min(Math.max(shape.dim, 0), 1)})`;
      const r = shape.rect;
      ctx.fillRect(0, 0, canvasWidth, r.y);
      ctx.fillRect(0, r.y + r.height, canvasWidth, canvasHeight - (r.y + r.height));
      ctx.fillRect(0, r.y, r.x, r.height);
      ctx.fillRect(r.x + r.width, r.y, canvasWidth - (r.x + r.width), r.height);
      break;
    }
    case "step": {
      ctx.beginPath();
      ctx.arc(shape.center.x, shape.center.y, shape.radius, 0, Math.PI * 2);
      ctx.fillStyle = toCss(shape.fill);
      ctx.fill();

      ctx.fillStyle = toCss(shape.text_color);
      ctx.font = `600 ${Math.round(shape.radius * 1.2)}px ui-sans-serif, system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(shape.number), shape.center.x, shape.center.y + shape.radius * 0.05);
      break;
    }
    case "text": {
      drawText(
        ctx,
        shape.content,
        shape.rect.x,
        shape.rect.y,
        shape.size,
        shape.bold,
        shape.italic,
        shape.color,
        shape.outline,
      );
      break;
    }
    case "speech_balloon": {
      // Tail first, so the bubble fill covers the join and leaves no seam.
      drawBalloonTail(ctx, shape.rect, shape.tail, shape.fill);
      const radius = Math.min(shape.rect.height * 0.25, 18);
      path(ctx, () => roundedRect(ctx, shape.rect, radius));
      ctx.fillStyle = toCss(shape.fill);
      ctx.fill();
      strokePath(ctx, shape.stroke);

      ctx.fillStyle = toCss(shape.text_color);
      ctx.font = `600 ${shape.size}px ui-sans-serif, system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(
        shape.content,
        shape.rect.x + shape.rect.width / 2,
        shape.rect.y + shape.rect.height / 2,
      );
      break;
    }
  }

  ctx.restore();
}

/**
 * Draw text from its top-left corner, with an optional outline.
 *
 * The outline is stamped in eight directions around the fill — the same
 * approach the Rust renderer takes, so the preview and the export agree. It is
 * what keeps light text legible over an arbitrary screenshot.
 */
function drawText(
  ctx: CanvasRenderingContext2D,
  content: string,
  x: number,
  y: number,
  size: number,
  bold: boolean,
  italic: boolean,
  color: Color,
  outline: Color,
): void {
  if (!content) return;

  const weight = bold ? "700" : "400";
  const slant = italic ? "italic " : "";
  ctx.font = `${slant}${weight} ${size}px ui-sans-serif, system-ui, sans-serif`;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  const lines = content.split("\n");
  const lineHeight = size * 1.25;

  if (outline.a > 0) {
    const spread = Math.min(Math.max(size / 16, 1), 3);
    ctx.fillStyle = toCss(outline);
    for (const [dx, dy] of OUTLINE_OFFSETS) {
      lines.forEach((line, index) => {
        ctx.fillText(line, x + dx * spread, y + dy * spread + lineHeight * index);
      });
    }
  }

  ctx.fillStyle = toCss(color);
  lines.forEach((line, index) => {
    ctx.fillText(line, x, y + lineHeight * index);
  });
}

const OUTLINE_OFFSETS: [number, number][] = [
  [-1, -1],
  [0, -1],
  [1, -1],
  [-1, 0],
  [1, 0],
  [-1, 1],
  [0, 1],
  [1, 1],
];

/** The pointer from a balloon towards whatever it is labelling. */
function drawBalloonTail(
  ctx: CanvasRenderingContext2D,
  rect: Rect,
  tail: Point,
  fill: Color,
): void {
  const cx = rect.x + rect.width / 2;
  const cy = rect.y + rect.height / 2;
  const dx = tail.x - cx;
  const dy = tail.y - cy;
  const len = Math.hypot(dx, dy);
  if (len < 0.001) return;

  const ux = dx / len;
  const uy = dy / len;
  const half = Math.max(Math.min(rect.width, rect.height) * 0.18, 6);
  const baseX = cx + ux * (len * 0.2);
  const baseY = cy + uy * (len * 0.2);

  ctx.beginPath();
  ctx.moveTo(tail.x, tail.y);
  ctx.lineTo(baseX - uy * half, baseY + ux * half);
  ctx.lineTo(baseX + uy * half, baseY - ux * half);
  ctx.closePath();
  ctx.fillStyle = toCss(fill);
  ctx.fill();
}

function path(ctx: CanvasRenderingContext2D, build: () => void): void {
  ctx.beginPath();
  build();
}

function strokePath(ctx: CanvasRenderingContext2D, stroke: Stroke): void {
  if (stroke.width <= 0 || stroke.color.a === 0) return;
  ctx.strokeStyle = toCss(stroke.color);
  ctx.lineWidth = stroke.width;
  ctx.stroke();
}

function roundedRect(ctx: CanvasRenderingContext2D, rect: Rect, radius: number): void {
  const r = Math.max(0, Math.min(radius, rect.width / 2, rect.height / 2));
  if (r <= 0) {
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    return;
  }
  ctx.roundRect(rect.x, rect.y, rect.width, rect.height, r);
}

function drawArrow(
  ctx: CanvasRenderingContext2D,
  shape: Extract<Shape, { kind: "arrow" }>,
): void {
  const dx = shape.to.x - shape.from.x;
  const dy = shape.to.y - shape.from.y;
  const length = Math.hypot(dx, dy);
  if (length < 0.001) return;

  const ux = dx / length;
  const uy = dy / length;
  const headLength = Math.max(shape.stroke.width * 4, 12);
  const hasEnd = shape.head === "end" || shape.head === "both";
  const hasStart = shape.head === "both";

  // Stop the shaft short of each head so the tip stays sharp.
  const start = hasStart
    ? { x: shape.from.x + ux * headLength * 0.6, y: shape.from.y + uy * headLength * 0.6 }
    : shape.from;
  const end = hasEnd
    ? { x: shape.to.x - ux * headLength * 0.6, y: shape.to.y - uy * headLength * 0.6 }
    : shape.to;

  path(ctx, () => {
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(end.x, end.y);
  });
  ctx.lineCap = "round";
  strokePath(ctx, shape.stroke);

  ctx.fillStyle = toCss(shape.stroke.color);
  if (hasEnd) arrowHead(ctx, shape.to, ux, uy, headLength);
  if (hasStart) arrowHead(ctx, shape.from, -ux, -uy, headLength);
}

function arrowHead(
  ctx: CanvasRenderingContext2D,
  tip: { x: number; y: number },
  ux: number,
  uy: number,
  length: number,
): void {
  const halfWidth = length * 0.42;
  const baseX = tip.x - ux * length;
  const baseY = tip.y - uy * length;
  // Perpendicular to the direction of travel.
  const px = -uy;
  const py = ux;

  ctx.beginPath();
  ctx.moveTo(tip.x, tip.y);
  ctx.lineTo(baseX + px * halfWidth, baseY + py * halfWidth);
  ctx.lineTo(baseX - px * halfWidth, baseY - py * halfWidth);
  ctx.closePath();
  ctx.fill();
}

/**
 * Apply a CSS filter to a region of what is already on the canvas.
 *
 * The region is copied out, filtered, and drawn back, so annotations painted
 * earlier are included — matching the Rust renderer, which also operates on the
 * running composite rather than the original capture.
 */
function filterRegion(ctx: CanvasRenderingContext2D, rect: Rect, filter: string): void {
  const region = clampRegion(ctx, rect);
  if (!region) return;

  const scratch = document.createElement("canvas");
  scratch.width = region.width;
  scratch.height = region.height;
  const scratchCtx = scratch.getContext("2d");
  if (!scratchCtx) return;

  scratchCtx.drawImage(
    ctx.canvas,
    region.x,
    region.y,
    region.width,
    region.height,
    0,
    0,
    region.width,
    region.height,
  );

  ctx.save();
  ctx.filter = filter;
  // Clip so the filter's own bleed cannot spill outside the redacted area.
  ctx.beginPath();
  ctx.rect(region.x, region.y, region.width, region.height);
  ctx.clip();
  ctx.drawImage(scratch, region.x, region.y);
  ctx.restore();
}

function pixelateRegion(ctx: CanvasRenderingContext2D, rect: Rect, block: number): void {
  const region = clampRegion(ctx, rect);
  if (!region) return;

  const smallWidth = Math.max(1, Math.round(region.width / block));
  const smallHeight = Math.max(1, Math.round(region.height / block));

  const scratch = document.createElement("canvas");
  scratch.width = smallWidth;
  scratch.height = smallHeight;
  const scratchCtx = scratch.getContext("2d");
  if (!scratchCtx) return;

  scratchCtx.drawImage(
    ctx.canvas,
    region.x,
    region.y,
    region.width,
    region.height,
    0,
    0,
    smallWidth,
    smallHeight,
  );

  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(scratch, 0, 0, smallWidth, smallHeight, region.x, region.y, region.width, region.height);
  ctx.restore();
}

/** Integer, on-canvas version of a rectangle, or null if nothing is visible. */
function clampRegion(ctx: CanvasRenderingContext2D, rect: Rect): Rect | null {
  const x = Math.max(0, Math.floor(rect.x));
  const y = Math.max(0, Math.floor(rect.y));
  const right = Math.min(ctx.canvas.width, Math.ceil(rect.x + rect.width));
  const bottom = Math.min(ctx.canvas.height, Math.ceil(rect.y + rect.height));

  if (right <= x || bottom <= y) return null;
  return { x, y, width: right - x, height: bottom - y };
}

/** Selection chrome, drawn on the overlay layer rather than into the image. */
export function drawSelection(
  ctx: CanvasRenderingContext2D,
  rect: Rect,
  scale: number,
): void {
  const line = 1 / scale;
  ctx.save();
  ctx.strokeStyle = "#4c9aff";
  ctx.lineWidth = line * 2;
  ctx.setLineDash([line * 5, line * 4]);
  ctx.strokeRect(rect.x, rect.y, rect.width, rect.height);
  ctx.restore();
}
