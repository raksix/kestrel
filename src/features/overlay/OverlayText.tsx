import { useEffect, useRef, useState } from "react";
import type { Color, Rect, Shape } from "../../lib/editorTypes";
import { rgba } from "../../lib/editorTypes";

/**
 * Typing text onto the overlay.
 *
 * A real `<textarea>`, positioned over the spot the shape will occupy, rather
 * than key events collected by hand. That is what keeps input methods, Turkish
 * dead keys, dictation and screen readers working — none of which survive a
 * hand-rolled keystroke capture.
 *
 * Enter commits and Shift+Enter adds a line, matching the editor. Escape
 * abandons the box without leaving an empty shape behind, because an invisible
 * zero-character annotation is a thing you can only find by deleting it.
 */
export default function OverlayText({
  rect,
  balloon,
  tail,
  color,
  onCommit,
  onCancel,
}: {
  rect: Rect;
  balloon: boolean;
  tail: { x: number; y: number };
  color: Color;
  onCommit: (shape: Shape) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  const size = Math.max(14, Math.min(rect.height * 0.6, 48));

  const commit = () => {
    const content = value.trim();
    if (!content) {
      onCancel();
      return;
    }

    onCommit(
      balloon
        ? {
            kind: "speech_balloon",
            rect,
            tail,
            content,
            stroke: { color, width: 2 },
            fill: rgba(255, 255, 255, 235),
            text_color: rgba(20, 20, 20),
            size,
          }
        : {
            kind: "text",
            rect,
            content,
            color,
            // A dark outline is what keeps light text legible over a screenshot
            // of anything. Without it, white on white is a real outcome.
            outline: rgba(0, 0, 0, 190),
            size,
            bold: false,
            italic: false,
          },
    );
  };

  return (
    <textarea
      ref={ref}
      className="overlay__text-input"
      value={value}
      spellCheck={false}
      aria-label={balloon ? "Balon metni" : "Metin"}
      style={{
        left: rect.x,
        top: rect.y,
        width: Math.max(rect.width, 80),
        height: Math.max(rect.height, size + 12),
        fontSize: size,
        color: `rgb(${color.r}, ${color.g}, ${color.b})`,
      }}
      onChange={(event) => setValue(event.target.value)}
      // The overlay listens for keys globally to switch tools; without this
      // every letter typed would also pick a different tool.
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
        if (event.key === "Enter" && !event.shiftKey) {
          event.preventDefault();
          commit();
        }
      }}
      onPointerDown={(event) => event.stopPropagation()}
      onBlur={commit}
    />
  );
}
