/**
 * Reading an image out of a paste or a drop.
 *
 * Shared by the overlay and the editor, because "paste the thing on my
 * clipboard" should mean the same in both and a second implementation is a
 * second set of edge cases.
 *
 * The result is a base64 PNG, which is what `Shape::Image` carries. Carrying
 * pixels rather than a path is deliberate — see the shape's own comment — and
 * it also means a paste from a browser, which has no path at all, works the
 * same as a paste from a file manager.
 */

/** The largest image accepted from a paste, before it is scaled down. */
const MAX_EDGE = 2000;

export interface PastedImage {
  /** A `data:image/png;base64,...` URL. */
  data: string;
  width: number;
  height: number;
}

/**
 * Pull an image out of a clipboard or drag event.
 *
 * Returns `null` when the event carries no image — text, a file that is not an
 * image, or nothing. That is not an error: pasting text onto a screenshot is a
 * reasonable thing to try and the right answer is to do nothing visible.
 */
export async function imageFromEvent(
  event: ClipboardEvent | DragEvent,
): Promise<PastedImage | null> {
  const transfer =
    "clipboardData" in event ? event.clipboardData : (event as DragEvent).dataTransfer;
  if (!transfer) return null;

  for (const item of Array.from(transfer.items ?? [])) {
    if (item.kind !== "file" || !item.type.startsWith("image/")) continue;
    const file = item.getAsFile();
    if (file) return await fromBlob(file);
  }

  for (const file of Array.from(transfer.files ?? [])) {
    if (file.type.startsWith("image/")) return await fromBlob(file);
  }

  return null;
}

/**
 * Decode a blob and re-encode it as a PNG.
 *
 * Re-encoding rather than passing the original bytes through: the clipboard
 * hands over TIFF on macOS and BMP on Windows about as often as PNG, and the
 * document format says PNG. Converting here means the renderer never has to
 * guess, and a document written on one platform opens on another.
 */
async function fromBlob(blob: Blob): Promise<PastedImage | null> {
  const url = URL.createObjectURL(blob);
  try {
    const element = await load(url);

    // A pasted photo can be far larger than the screenshot it is going onto,
    // and every copy of the document would carry those pixels. Scaling here
    // costs nothing visible and keeps the document a sane size.
    const scale = Math.min(1, MAX_EDGE / Math.max(element.width, element.height));
    const width = Math.max(1, Math.round(element.width * scale));
    const height = Math.max(1, Math.round(element.height * scale));

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(element, 0, 0, width, height);

    return { data: canvas.toDataURL("image/png"), width, height };
  } catch {
    return null;
  } finally {
    URL.revokeObjectURL(url);
  }
}

function load(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const element = new Image();
    element.onload = () => resolve(element);
    element.onerror = () => reject(new Error("could not decode the pasted image"));
    element.src = src;
  });
}

/**
 * Where a pasted image should land.
 *
 * Centred on `at`, at its own size, but never wider than `bounds` — a 4000px
 * paste onto a 800px selection should be visible, not one corner of itself.
 */
export function placeAt(
  image: PastedImage,
  at: { x: number; y: number },
  bounds: { width: number; height: number },
): { x: number; y: number; width: number; height: number } {
  const scale = Math.min(1, bounds.width / image.width, bounds.height / image.height);
  const width = Math.max(1, Math.round(image.width * scale));
  const height = Math.max(1, Math.round(image.height * scale));

  return {
    x: Math.round(at.x - width / 2),
    y: Math.round(at.y - height / 2),
    width,
    height,
  };
}


/**
 * The clipboard image, measured so it can be placed.
 *
 * `imageFromEvent` covers drops and any paste the webview does deliver; this
 * covers the case it does not, which on the overlay is most of them.
 */
export async function imageFromClipboard(
  read: () => Promise<string | null>,
): Promise<PastedImage | null> {
  const data = await read();
  if (!data) return null;

  try {
    const element = await load(data);
    return { data, width: element.width, height: element.height };
  } catch {
    return null;
  }
}
