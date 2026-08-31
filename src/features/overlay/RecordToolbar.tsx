import { ACTION_ICONS } from "./icons";

/**
 * The overlay toolbar while a *recording* region is being chosen.
 *
 * Deliberately not the annotation toolbar with some buttons hidden. Nothing
 * drawn here could survive into a video — the file is encoded frame by frame
 * from the live screen, not composed from a still — so offering the marks would
 * be offering something that silently does nothing. What is left is the two
 * decisions that do matter: the size of the rectangle, and whether the result
 * is a video or a GIF.
 */
export default function RecordToolbar({
  gif,
  selection,
  fps,
  onCancel,
  onStart,
}: {
  gif: boolean;
  /** The chosen rectangle, or null while nothing has been dragged yet. */
  selection: { width: number; height: number } | null;
  fps: number;
  onCancel: () => void;
  onStart: () => void;
}) {
  const ready = !!selection && selection.width >= 8 && selection.height >= 8;

  return (
    <div
      className="overlay__toolbar overlay__toolbar--record"
      // The toolbar sits on the selection surface; without this every click
      // would start a new drag underneath it.
      onPointerDown={(event) => event.stopPropagation()}
      onPointerUp={(event) => event.stopPropagation()}
      role="toolbar"
      aria-label="Bölge kaydı"
    >
      <div className="overlay__toolbar-row">
        <span className="overlay__record-kind">
          {gif ? <ACTION_ICONS.film /> : <ACTION_ICONS.record />}
          <span>{gif ? "GIF kaydı" : "Video kaydı"}</span>
        </span>

        <span className="overlay__toolbar-divider" aria-hidden="true" />

        {/* The numbers people actually check before pressing record: what the
            frame will be, and that the rectangle is a sane size. */}
        <span className="overlay__record-size" aria-live="polite">
          {ready
            ? `${Math.round(selection!.width)} × ${Math.round(selection!.height)} · ${fps} fps`
            : "Kaydedilecek alanı sürükle"}
        </span>

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
          className="overlay__tool overlay__tool--record"
          onClick={onStart}
          disabled={!ready}
          title={
            ready
              ? "Kaydı başlat — Enter"
              : "Önce bir alan seç — ya da Boşluk ile tüm ekranı kaydet"
          }
        >
          <ACTION_ICONS.record />
          <span>Kaydı başlat</span>
        </button>
      </div>

      <div className="overlay__toolbar-row overlay__record-hints">
        <span>Enter başlat</span>
        <span>Boşluk tüm ekran</span>
        <span>Shift kare</span>
        <span>Yön tuşları ince ayar</span>
        <span>Esc iptal</span>
      </div>
    </div>
  );
}
