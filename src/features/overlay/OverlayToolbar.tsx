import type { Color } from "../../lib/editorTypes";
import { fromHex, toHex } from "../../lib/editorTypes";
import { ACTION_ICONS, ICONS } from "./icons";

/**
 * Tools available while the selection is still being made.
 *
 * The same set as the editor's, with the same letters, because ShareX's overlay
 * is a full annotation surface and having to remember which marks are available
 * *here* versus *there* costs more than a slightly busier toolbar.
 *
 * Each tool also answers to its position — 1 through 9, then 0 — so the common
 * ones are reachable without knowing the letter, and the number sits on the
 * icon as its own label. The letters stay because they are ShareX's and muscle
 * memory carries over; the numbers are for everyone else.
 */
export const OVERLAY_TOOLS = [
  { id: "select", key: "v", label: "Seç" },
  { id: "rectangle", key: "r", label: "Kutu" },
  { id: "ellipse", key: "e", label: "Elips" },
  { id: "arrow", key: "a", label: "Ok" },
  { id: "line", key: "l", label: "Çizgi" },
  { id: "freehand", key: "f", label: "Kalem" },
  { id: "text", key: "t", label: "Metin" },
  { id: "balloon", key: "c", label: "Balon" },
  { id: "step", key: "n", label: "Adım" },
  { id: "highlight", key: "h", label: "Vurgu" },
  { id: "spotlight", key: "s", label: "Işık" },
  { id: "blur", key: "b", label: "Bulanık" },
  { id: "pixelate", key: "p", label: "Piksel" },
] as const;

export type OverlayTool = (typeof OVERLAY_TOOLS)[number]["id"] | null;

/**
 * The digit that selects each tool, by position.
 *
 * Ten digits for thirteen tools, so the last three answer to letters only. That
 * is better than inventing a two-key sequence for them: the digits exist to
 * make the common tools instant, not to be a complete second alphabet.
 */
export const digitFor = (index: number): string | null =>
  index < 10 ? String((index + 1) % 10) : null;

/**
 * The colours worth one click.
 *
 * Chosen to stay legible on a screenshot of anything: a saturated red, orange
 * and yellow for marks, white and black because half of all screenshots are
 * dark and half are light, and green, blue and purple for when red already
 * means something in the picture.
 */
const SWATCHES = [
  "#ff453a",
  "#ff9f0a",
  "#ffd60a",
  "#32d74b",
  "#0a84ff",
  "#bf5af2",
  "#ffffff",
  "#000000",
] as const;

/** Stroke widths as four sizes rather than a slider you have to aim at. */
const WIDTHS = [2, 4, 8, 14] as const;

export default function OverlayToolbar({
  tool,
  color,
  width,
  canUndo,
  canRedo,
  magnify,
  onTool,
  onColor,
  onWidth,
  onUndo,
  onRedo,
  onMagnify,
  onConfirm,
  onCancel,
}: {
  tool: OverlayTool;
  color: Color;
  width: number;
  canUndo: boolean;
  canRedo: boolean;
  magnify: boolean;
  onTool: (tool: OverlayTool) => void;
  onColor: (color: Color) => void;
  onWidth: (width: number) => void;
  onUndo: () => void;
  onRedo: () => void;
  onMagnify: (on: boolean) => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const current = toHex(color).toLowerCase();

  return (
    <div
      className="overlay__toolbar"
      // The toolbar sits on the selection surface; without this every click
      // would start a new drag underneath it.
      onPointerDown={(event) => event.stopPropagation()}
      onPointerUp={(event) => event.stopPropagation()}
      role="toolbar"
      aria-label="Anotasyon araçları"
    >
      {/*
        Two rows by construction rather than by relying on flex wrapping. With
        thirteen tools plus the colour picker and five actions, a single row
        overflowed the window and clipped everything after the middle —
        including the confirm button, which is the one control that must never
        be unreachable.
      */}
      <div className="overlay__toolbar-row">
        {OVERLAY_TOOLS.map((item, index) => {
          const Icon = ICONS[item.id];
          const digit = digitFor(index);
          return (
            <button
              key={item.id}
              type="button"
              className="overlay__tool"
              aria-pressed={tool === item.id}
              aria-label={item.label}
              title={`${item.label} — ${item.key.toUpperCase()}${digit ? ` veya ${digit}` : ""}`}
              onClick={() => onTool(tool === item.id ? null : item.id)}
            >
              {Icon ? <Icon /> : item.label}
              {digit && (
                <span className="overlay__tool-digit" aria-hidden="true">
                  {digit}
                </span>
              )}
            </button>
          );
        })}
      </div>

      <div className="overlay__toolbar-row">
        <div className="overlay__swatches" role="group" aria-label="Renk">
          {SWATCHES.map((swatch) => (
            <button
              key={swatch}
              type="button"
              className="overlay__swatch"
              style={{ background: swatch }}
              aria-label={swatch}
              aria-pressed={current === swatch}
              onClick={() => onColor(fromHex(swatch))}
            />
          ))}
          {/*
            The native picker as the last swatch rather than a separate control:
            it is the same decision, so it belongs in the same row, and it shows
            the current colour when that colour is not one of the presets.
          */}
          <label className="overlay__swatch overlay__swatch--custom" title="Başka renk">
            <span style={{ background: current }} />
            <input
              type="color"
              aria-label="Özel renk"
              value={current}
              onChange={(event) => onColor(fromHex(event.target.value))}
            />
          </label>
        </div>

        <span className="overlay__toolbar-divider" aria-hidden="true" />

        <div className="overlay__widths" role="group" aria-label="Kalınlık">
          {WIDTHS.map((size) => (
            <button
              key={size}
              type="button"
              className="overlay__width-dot"
              aria-label={`${size} piksel`}
              aria-pressed={width === size}
              onClick={() => onWidth(size)}
            >
              <span style={{ width: size, height: size }} />
            </button>
          ))}
        </div>

        <span className="overlay__toolbar-divider" aria-hidden="true" />

        <button
          type="button"
          className="overlay__tool"
          onClick={onUndo}
          disabled={!canUndo}
          aria-label="Geri al"
          title="Geri al — ⌘Z"
        >
          <ACTION_ICONS.undo />
        </button>
        <button
          type="button"
          className="overlay__tool"
          onClick={onRedo}
          disabled={!canRedo}
          aria-label="Yinele"
          title="Yinele — ⇧⌘Z"
        >
          <ACTION_ICONS.redo />
        </button>
        <button
          type="button"
          className="overlay__tool"
          aria-pressed={magnify}
          aria-label="Büyüteç"
          title="Büyüteç — M · piksel ızgarası ve renk okuması"
          onClick={() => onMagnify(!magnify)}
        >
          <ACTION_ICONS.magnify />
        </button>

        <span className="overlay__toolbar-divider" aria-hidden="true" />

        <button
          type="button"
          className="overlay__tool"
          onClick={onCancel}
          aria-label="İptal"
          title="İptal — Esc"
        >
          <ACTION_ICONS.cancel />
        </button>
        <button
          type="button"
          className="overlay__tool overlay__tool--confirm"
          onClick={onConfirm}
          title="Yakala — Enter"
        >
          <ACTION_ICONS.confirm />
          <span>Yakala</span>
        </button>
      </div>
    </div>
  );
}
