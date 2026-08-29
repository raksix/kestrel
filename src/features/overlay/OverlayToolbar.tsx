import type { Color } from "../../lib/editorTypes";
import { fromHex, toHex } from "../../lib/editorTypes";

/**
 * Tools available while the selection is still being made.
 *
 * Deliberately a subset of the editor's: these are the ones people reach for
 * without leaving the capture — mark a thing, hide a thing, number a thing.
 * Anything more considered belongs in the editor, which is one keypress away.
 */
export const OVERLAY_TOOLS = [
  { id: "select", key: "v", label: "Seç" },
  { id: "rectangle", key: "r", label: "Kutu" },
  { id: "ellipse", key: "e", label: "Elips" },
  { id: "arrow", key: "a", label: "Ok" },
  { id: "line", key: "l", label: "Çizgi" },
  { id: "freehand", key: "f", label: "Kalem" },
  { id: "step", key: "n", label: "Adım" },
  { id: "highlight", key: "h", label: "Vurgu" },
  { id: "blur", key: "b", label: "Bulanık" },
  { id: "pixelate", key: "p", label: "Piksel" },
] as const;

export type OverlayTool = (typeof OVERLAY_TOOLS)[number]["id"] | null;

export default function OverlayToolbar({
  tool,
  color,
  width,
  canUndo,
  onTool,
  onColor,
  onWidth,
  onUndo,
  onConfirm,
  onCancel,
}: {
  tool: OverlayTool;
  color: Color;
  width: number;
  canUndo: boolean;
  onTool: (tool: OverlayTool) => void;
  onColor: (color: Color) => void;
  onWidth: (width: number) => void;
  onUndo: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
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
      {OVERLAY_TOOLS.map((item) => (
        <button
          key={item.id}
          type="button"
          className="overlay__tool"
          aria-pressed={tool === item.id}
          title={`${item.label} (${item.key.toUpperCase()})`}
          onClick={() => onTool(tool === item.id ? null : item.id)}
        >
          {item.label}
        </button>
      ))}

      <span className="overlay__toolbar-divider" aria-hidden="true" />

      <input
        type="color"
        className="overlay__color"
        aria-label="Renk"
        value={toHex(color)}
        onChange={(event) => onColor(fromHex(event.target.value))}
      />
      <input
        type="range"
        className="overlay__width"
        aria-label="Kalınlık"
        min={1}
        max={20}
        value={width}
        onChange={(event) => onWidth(Number(event.target.value))}
      />

      <span className="overlay__toolbar-divider" aria-hidden="true" />

      <button type="button" className="overlay__tool" onClick={onUndo} disabled={!canUndo}>
        Geri
      </button>
      <button type="button" className="overlay__tool" onClick={onCancel}>
        İptal
      </button>
      <button
        type="button"
        className="overlay__tool overlay__tool--confirm"
        onClick={onConfirm}
      >
        Yakala
      </button>
    </div>
  );
}
