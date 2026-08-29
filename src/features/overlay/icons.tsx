/**
 * Tool icons for the overlay.
 *
 * Inline SVG rather than an icon font or a sprite sheet: the overlay is one of
 * the first things drawn after a shortcut is pressed, and waiting on a font or
 * a second asset to load is a visible flash of unstyled toolbar at exactly the
 * wrong moment.
 *
 * Every icon is drawn on a 24-unit grid with `currentColor`, so a single CSS
 * rule controls the colour in every state and the icons stay aligned with the
 * text labels beside them.
 */

const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

function Svg({ children }: { children: React.ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
      {children}
    </svg>
  );
}

export const ICONS: Record<string, () => React.ReactElement> = {
  select: () => (
    <Svg>
      <path {...stroke} d="M5 3l14 8-6 1.5L10 19z" />
    </Svg>
  ),
  rectangle: () => (
    <Svg>
      <rect {...stroke} x="3.5" y="5.5" width="17" height="13" rx="1.5" />
    </Svg>
  ),
  ellipse: () => (
    <Svg>
      <ellipse {...stroke} cx="12" cy="12" rx="8.5" ry="6.5" />
    </Svg>
  ),
  arrow: () => (
    <Svg>
      <path {...stroke} d="M4 20L20 4M20 4h-7M20 4v7" />
    </Svg>
  ),
  line: () => (
    <Svg>
      <path {...stroke} d="M4 20L20 4" />
    </Svg>
  ),
  // A pencil drawing a line, not a bare squiggle: the squiggle read as a
  // waveform rather than as "draw freehand".
  freehand: () => (
    <Svg>
      <path {...stroke} d="M15.5 4.5l4 4-9 9-5 1 1-5z" />
      <path {...stroke} d="M13.5 6.5l4 4" />
    </Svg>
  ),
  text: () => (
    <Svg>
      <path {...stroke} d="M5 6h14M12 6v13M9 19h6" />
    </Svg>
  ),
  balloon: () => (
    <Svg>
      <path {...stroke} d="M4 5h16v10H10l-4 4v-4H4z" />
    </Svg>
  ),
  step: () => (
    <Svg>
      <circle {...stroke} cx="12" cy="12" r="8.5" />
      <path {...stroke} d="M10.5 9.5L12.5 8.5V16" />
    </Svg>
  ),
  // A chisel-tip marker over the band it leaves. The previous icon was the
  // same pencil shape as freehand, so the two tools were indistinguishable.
  highlight: () => (
    <Svg>
      <path {...stroke} d="M8 13l6-6 4 4-6 6H8z" />
      <path {...stroke} d="M14 5l2-2 4 4-2 2" />
      <path
        fill="currentColor"
        stroke="none"
        opacity="0.55"
        d="M3.5 19h17v2.5h-17z"
      />
    </Svg>
  ),
  spotlight: () => (
    <Svg>
      <circle {...stroke} cx="12" cy="12" r="4.5" />
      <path {...stroke} d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M19 5l-1.5 1.5M6.5 17.5L5 19" />
    </Svg>
  ),
  blur: () => (
    <Svg>
      <circle {...stroke} cx="12" cy="12" r="8.5" strokeDasharray="2 2.5" />
      <circle {...stroke} cx="12" cy="12" r="4" strokeDasharray="2 2.5" />
    </Svg>
  ),
  // A checkerboard of filled blocks. Five outlined squares in a quincunx read
  // as the command key, which is not a thing this tool does.
  pixelate: () => (
    <Svg>
      <path
        fill="currentColor"
        stroke="none"
        d="M4 4h5.3v5.3H4zM14.7 4H20v5.3h-5.3zM9.3 9.3h5.4v5.4H9.3zM4 14.7h5.3V20H4zM14.7 14.7H20V20h-5.3z"
      />
      <rect
        {...stroke}
        x="3.5"
        y="3.5"
        width="17"
        height="17"
        rx="1"
        opacity="0.45"
      />
    </Svg>
  ),
};

/** Actions get icons too, so the second row reads as one thing with the first. */
export const ACTION_ICONS = {
  // The arrowhead is the whole icon. Without it these read as two plain
  // circles and undo is indistinguishable from redo.
  undo: () => (
    <Svg>
      <path {...stroke} d="M4 9h9a5.5 5.5 0 010 11H7" />
      <path {...stroke} d="M4 9l4-4M4 9l4 4" />
    </Svg>
  ),
  redo: () => (
    <Svg>
      <path {...stroke} d="M20 9h-9a5.5 5.5 0 000 11h6" />
      <path {...stroke} d="M20 9l-4-4M20 9l-4 4" />
    </Svg>
  ),
  magnify: () => (
    <Svg>
      <circle {...stroke} cx="10.5" cy="10.5" r="6.5" />
      <path {...stroke} d="M15.5 15.5L21 21M8 10.5h5M10.5 8v5" />
    </Svg>
  ),
  cancel: () => (
    <Svg>
      <path {...stroke} d="M6 6l12 12M18 6L6 18" />
    </Svg>
  ),
  confirm: () => (
    <Svg>
      <path {...stroke} d="M4 12.5l5.5 5.5L20 6" />
    </Svg>
  ),
};
