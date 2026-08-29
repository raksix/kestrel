import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { closePin } from "../../lib/ipc";
import "./pin.css";

interface PinProps {
  path: string;
  width: number;
  height: number;
}

/** Scale bounds. Below the lower one a pin is unreadable; above it covers the
 *  screen it is meant to float above. */
const MIN_SCALE = 0.1;
const MAX_SCALE = 4;
const SCALE_STEP = 0.1;

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

/**
 * A capture floated above everything else.
 *
 * The keys are ShareX's: drag to move, right click or Escape to close, middle
 * click to reset, double click to minimise, wheel to scale, modifier+wheel for
 * opacity, and Mod+C to copy.
 */
export default function Pin({ path, width, height }: PinProps) {
  const [scale, setScale] = useState(1);
  const [opacity, setOpacity] = useState(1);
  const [minimised, setMinimised] = useState(false);
  const label = useRef(getCurrentWindow().label);

  // The window is opened already fitted, so the initial scale is 1 relative to
  // whatever size it was given rather than to the image's pixel size.
  const baseSize = useRef<{ width: number; height: number } | null>(null);
  useEffect(() => {
    getCurrentWindow()
      .innerSize()
      .then(async (size) => {
        const factor = await getCurrentWindow().scaleFactor();
        const logical = size.toLogical(factor);
        baseSize.current = { width: logical.width, height: logical.height };
      })
      .catch(() => undefined);
  }, []);

  const applyScale = useCallback((next: number) => {
    const base = baseSize.current;
    if (!base) return;
    const clamped = clamp(next, MIN_SCALE, MAX_SCALE);
    setScale(clamped);
    void getCurrentWindow().setSize(
      new LogicalSize(
        Math.max(Math.round(base.width * clamped), 32),
        Math.max(Math.round(base.height * clamped), 32),
      ),
    );
  }, []);

  const close = useCallback(() => {
    void closePin(label.current);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;

      if (event.key === "Escape") {
        event.preventDefault();
        close();
        return;
      }
      if (mod && event.key.toLowerCase() === "c") {
        event.preventDefault();
        // The path, not the pixels: a pin is a reference to a capture that is
        // already on disk, and a path is what another app can actually use.
        void writeText(path);
        return;
      }
      if (event.key === "+" || event.key === "=") {
        event.preventDefault();
        mod ? setOpacity((o) => clamp(o + 0.1, 0.2, 1)) : applyScale(scale + SCALE_STEP);
      }
      if (event.key === "-" || event.key === "_") {
        event.preventDefault();
        mod ? setOpacity((o) => clamp(o - 0.1, 0.2, 1)) : applyScale(scale - SCALE_STEP);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [applyScale, close, path, scale]);

  return (
    <div
      className={`pin ${minimised ? "pin--minimised" : ""}`}
      style={{ opacity }}
      onContextMenu={(event) => {
        event.preventDefault();
        close();
      }}
      onPointerDown={(event) => {
        if (event.button === 1) {
          // Middle click resets both scale and opacity, as in ShareX.
          applyScale(1);
          setOpacity(1);
          return;
        }
        if (event.button === 0) {
          // Dragging the image moves the window; there is no title bar.
          void getCurrentWindow().startDragging();
        }
      }}
      onDoubleClick={() => setMinimised((value) => !value)}
      onWheel={(event) => {
        const direction = event.deltaY < 0 ? 1 : -1;
        if (event.metaKey || event.ctrlKey) {
          setOpacity((o) => clamp(o + direction * 0.05, 0.2, 1));
        } else {
          applyScale(scale + direction * SCALE_STEP);
        }
      }}
      role="img"
      aria-label={`Sabitlenmiş yakalama, ${width} × ${height}`}
    >
      <img className="pin__image" src={convertFileSrc(path)} alt="" draggable={false} />
      <div className="pin__hint">
        Sürükle taşı · sağ tık kapat · tekerlek boyut · çift tık küçült
      </div>
    </div>
  );
}
