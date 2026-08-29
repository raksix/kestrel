import { useEffect, useRef, useState } from "react";
import { overlaySample, type OverlaySample } from "../../lib/ipc";

/**
 * ShareX's magnifier: a zoomed pixel view under the cursor, with a grid, a
 * crosshair on the exact pixel, and the colour under it.
 *
 * The pixels come from Rust one small patch at a time rather than by painting
 * the whole screenshot into the overlay. That is the same reason the overlay is
 * transparent in the first place — a full-screen image crossing the IPC
 * boundary is megabytes, and it would also have to be kept in sync with a
 * frozen frame that Rust already owns. A 33x33 patch is about a kilobyte.
 *
 * Requests are serialised: at most one is in flight, and pointer moves that
 * arrive while it is are collapsed into a single follow-up. Without that, a
 * fast drag queues hundreds of round trips and the magnifier lags behind the
 * cursor by seconds.
 */
const RADIUS = 12;
const PIXEL = 8;

export default function Magnifier({
  cursor,
  origin,
  bounds,
}: {
  /** Cursor position in overlay-window coordinates. */
  cursor: { x: number; y: number };
  /** Where this overlay window sits in global screen coordinates. */
  origin: { x: number; y: number };
  bounds: { width: number; height: number };
}) {
  const [sample, setSample] = useState<OverlaySample | null>(null);
  const inFlight = useRef(false);
  const pending = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const request = (point: { x: number; y: number }) => {
      if (inFlight.current) {
        pending.current = point;
        return;
      }
      inFlight.current = true;

      overlaySample(Math.round(point.x + origin.x), Math.round(point.y + origin.y), RADIUS)
        .then(setSample)
        .catch(() => {
          // The selection can finish while a request is in flight, which ends
          // the frozen frames. Nothing to report — the magnifier is going away.
        })
        .finally(() => {
          inFlight.current = false;
          const next = pending.current;
          pending.current = null;
          if (next) request(next);
        });
    };

    request(cursor);
  }, [cursor, origin]);

  if (!sample) return null;

  const width = sample.width * PIXEL;
  const height = sample.height * PIXEL;

  // Keep the panel on screen: near the right or bottom edge it flips to the
  // other side of the cursor rather than being clipped, which is precisely
  // where a magnifier is most useful.
  const flipX = cursor.x + 24 + width > bounds.width;
  const flipY = cursor.y + 24 + height + 24 > bounds.height;

  return (
    <div
      className="overlay__magnifier"
      style={{
        left: flipX ? cursor.x - width - 24 : cursor.x + 24,
        top: flipY ? cursor.y - height - 48 : cursor.y + 24,
      }}
      aria-hidden="true"
    >
      <div className="overlay__magnifier-view" style={{ width, height }}>
        <img
          src={sample.image}
          width={width}
          height={height}
          alt=""
          // Nearest-neighbour: the point is to see individual pixels, and a
          // smoothed magnifier shows a blur where the edge actually is.
          style={{ imageRendering: "pixelated" }}
        />
        <div
          className="overlay__magnifier-grid"
          style={{ backgroundSize: `${PIXEL}px ${PIXEL}px` }}
        />
        <div
          className="overlay__magnifier-target"
          style={{
            left: sample.centreX * PIXEL,
            top: sample.centreY * PIXEL,
            width: PIXEL,
            height: PIXEL,
          }}
        />
      </div>
      <div className="overlay__magnifier-colour">
        <span className="overlay__magnifier-swatch" style={{ background: sample.hex }} />
        <code>{sample.hex}</code>
      </div>
    </div>
  );
}
