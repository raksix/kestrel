import type { Color } from "../../lib/editorTypes";
import { fromHex, toHex } from "../../lib/editorTypes";

/**
 * Tools available while the selection is still being made.
 *
 * The same set as the editor's, with the same keyboard letters, because
 * ShareX's overlay is a full annotation surface and having to remember which
 * marks are available *here* versus *there* is a worse cost than a slightly
 * busier toolbar. The editor is still one keypress away for anything that
 * wants a second pass.
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
        thirteen tools plus five actions the single row overflowed the window
        and clipped everything after "Geri" — including the confirm button,
        which is the one control that must never be unreachable.
      */}
      <div className="overlay__toolbar-row">
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

      </div>

      <div className="overlay__toolbar-row">
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
      <button type="button" className="overlay__tool" onClick={onRedo} disabled={!canRedo}>
        İleri
      </button>
      <button
        type="button"
        className="overlay__tool"
        aria-pressed={magnify}
        title="Büyüteç (M) — piksel ızgarası ve renk okuması"
        onClick={() => onMagnify(!magnify)}
      >
        Büyüteç
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
    </div>
  );
}
