/**
 * Typed wrappers around the Rust IPC surface (`src-tauri/src/commands.rs`).
 *
 * Hand-written for now; once the surface stabilises these will be generated
 * from the Rust definitions with `ts-rs` so the two halves cannot drift apart.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type CaptureMethod =
  | "fullscreen"
  | "active_window"
  | "active_monitor"
  | "window_menu"
  | "monitor_menu"
  | "region"
  | "region_light"
  | "region_transparent"
  | "last_region"
  | "custom_region"
  | "screen_recording"
  | "screen_recording_gif"
  | "scrolling_capture"
  | "auto_capture";

export type PermissionStatus = "granted" | "denied" | "not_required";

export interface Region {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DisplayInfo {
  id: number;
  name: string;
  region: Region;
  scale_factor: number;
  is_primary: boolean;
}

export interface WindowInfo {
  id: number;
  title: string;
  app_name: string;
  region: Region;
  is_minimized: boolean;
  z: number;
  is_focused: boolean;
}

export interface Capabilities {
  windowEnumeration: boolean;
  windowCapture: boolean;
  regionCapture: boolean;
  globalShortcuts: boolean;
  scrollingCapture: boolean;
  screenPermission: PermissionStatus;
}

export interface CaptureOutput {
  path: string | null;
  width: number;
  height: number;
  region: Region;
  preview: string;
  copiedToClipboard: boolean;
  windowTitle: string | null;
}

export interface TaskSettings {
  after_capture: string[];
  after_upload: string[];
  filename_pattern: string;
  output_directory: string | null;
  quality: number;
  image_format: string;
}

/** One entry in the after-capture or after-upload chain. */
export interface TaskInfo {
  id: string;
  /** False when Kestrel does not perform this task yet, so the UI greys it out. */
  implemented: boolean;
  /** True when the task does nothing without "save to file" earlier in the chain. */
  needsSavedFile: boolean;
}

/** After-capture tasks and after-upload tasks, both in pipeline order. */
export const listTasks = () => invoke<[TaskInfo[], TaskInfo[]]>("list_tasks");

/** Replace a workflow's chain, or the defaults when `id` is null. */
export const setTasks = (
  id: string | null,
  afterCapture: string[],
  afterUpload: string[],
) => invoke<AppSettings>("set_tasks", { id, afterCapture, afterUpload });

export interface Workflow {
  id: string;
  name: string;
  shortcut: string | null;
  method: CaptureMethod;
  settings: TaskSettings;
  enabled: boolean;
}

export interface AppSettings {
  version: number;
  workflows: Workflow[];
  defaults: TaskSettings;
}

export interface ShortcutReport {
  workflowId: string;
  name: string;
  accelerator: string;
  registered: boolean;
  error: string | null;
  /**
   * Set when the OS owns this combination. Registration succeeds but the key
   * press never arrives, so this is the only signal the user gets.
   */
  systemConflict: string | null;
}

export const CAPTURE_COMPLETE = "kestrel://capture-complete";
export const CAPTURE_FAILED = "kestrel://capture-failed";
export const SHORTCUTS_CHANGED = "kestrel://shortcuts-changed";

// ── Discovery ───────────────────────────────────────────────────────────
export const listDisplays = () => invoke<DisplayInfo[]>("list_displays");
export const listWindows = () => invoke<WindowInfo[]>("list_windows");
export const platformCapabilities = () => invoke<Capabilities>("platform_capabilities");

// ── Permissions ─────────────────────────────────────────────────────────
export const permissionStatus = () => invoke<PermissionStatus>("permission_status");
export const requestScreenPermission = () =>
  invoke<PermissionStatus>("request_screen_permission");
export const openPermissionSettings = () => invoke<void>("open_permission_settings");

// ── Capture ─────────────────────────────────────────────────────────────
export const captureFullscreen = () => invoke<CaptureOutput>("capture_fullscreen");
export const captureDisplay = (id: number) => invoke<CaptureOutput>("capture_display", { id });
export const captureWindow = (id: number) => invoke<CaptureOutput>("capture_window", { id });
export const captureActiveWindow = () => invoke<CaptureOutput>("capture_active_window");
export const windowThumbnail = (id: number) => invoke<string>("window_thumbnail", { id });
export const displayThumbnail = (id: number) => invoke<string>("display_thumbnail", { id });

/** One picker entry, thumbnail included. */
export interface TargetPreview {
  id: number;
  title: string;
  subtitle: string;
  width: number;
  height: number;
  preview: string | null;
}

/**
 * Both of these come from a single screen grab on the Rust side. Fetching a
 * thumbnail per window meant one real capture per window, which made the
 * picker crawl on a busy desktop.
 */
export const listWindowPreviews = () => invoke<TargetPreview[]>("list_window_previews");
export const listDisplayPreviews = () => invoke<TargetPreview[]>("list_display_previews");

// ── Region selection ────────────────────────────────────────────────────
export const beginRegionCapture = () => invoke<void>("begin_region_capture");
export const commitRegionCapture = (region: Region, document?: string) =>
  invoke<CaptureOutput>("commit_region_capture", { region, document });
export const cancelRegionCapture = () => invoke<void>("cancel_region_capture");

// ── Picker ──────────────────────────────────────────────────────────────
export const openWindowPicker = (tab: "windows" | "displays" = "windows") =>
  invoke<void>("open_window_picker", { tab });
export const closeWindowPicker = () => invoke<void>("close_window_picker");

// ── Settings ────────────────────────────────────────────────────────────
export const getSettings = () => invoke<AppSettings>("get_settings");
export const listWorkflows = () => invoke<Workflow[]>("list_workflows");
export const runWorkflow = (id: string) => invoke<CaptureOutput | null>("run_workflow", { id });
export const setWorkflowShortcut = (id: string, accelerator: string | null) =>
  invoke<Workflow[]>("set_workflow_shortcut", { id, accelerator });
export const setWorkflowEnabled = (id: string, enabled: boolean) =>
  invoke<Workflow[]>("set_workflow_enabled", { id, enabled });
export const resetShortcuts = () => invoke<Workflow[]>("reset_shortcuts");
export const shortcutRegistrationReport = () =>
  invoke<ShortcutReport[]>("shortcut_registration_report");
export const setFilenamePattern = (pattern: string) =>
  invoke<AppSettings>("set_filename_pattern", { pattern });
export const setOutputDirectory = (directory: string | null) =>
  invoke<AppSettings>("set_output_directory", { directory });
export const previewFilename = (pattern: string) =>
  invoke<string>("preview_filename", { pattern });

// ── History ─────────────────────────────────────────────────────────────

export interface HistoryEntry {
  id: number;
  /** Unix seconds. */
  createdAt: number;
  filename: string;
  path: string | null;
  width: number;
  height: number;
  windowTitle: string | null;
  url: string | null;
  thumbnailUrl: string | null;
  deletionUrl: string | null;
  destination: string | null;
  ocrText: string | null;
}

export interface HistoryQuery {
  text?: string;
  uploadedOnly?: boolean;
  limit?: number;
  offset?: number;
}

export const historyList = (query?: HistoryQuery) =>
  invoke<HistoryEntry[]>("history_list", { query });
export const historyGet = (id: number) => invoke<HistoryEntry | null>("history_get", { id });
export const historyRemove = (id: number) => invoke<void>("history_remove", { id });
export const historyClear = () => invoke<void>("history_clear");
export const historyCount = () => invoke<number>("history_count");

// ── Destinations ────────────────────────────────────────────────────────

export interface Destination {
  id: string;
  name: string;
  host: string;
  acceptsImage: boolean;
  acceptsText: boolean;
  acceptsFile: boolean;
  shortensUrls: boolean;
}

export interface Uploaded {
  url: string;
  thumbnailUrl: string | null;
  deletionUrl: string | null;
  destination: string;
}

export const listDestinations = () => invoke<Destination[]>("list_destinations");
export const importUploader = (path: string) =>
  invoke<Destination>("import_uploader", { path });
export const removeUploader = (id: string) =>
  invoke<Destination[]>("remove_uploader", { id });
export const setDefaultDestination = (id: string | null) =>
  invoke<void>("set_default_destination", { id });
export const defaultDestination = () => invoke<string | null>("default_destination");
export const uploadLastCapture = (destination?: string) =>
  invoke<Uploaded>("upload_last_capture", { destination });

// ── Pin to screen ───────────────────────────────────────────────────────

export interface Pinned {
  label: string;
  path: string;
  width: number;
  height: number;
}

export const pinLastCapture = () => invoke<Pinned>("pin_last_capture");
export const closePin = (label: string) => invoke<void>("close_pin", { label });

// ── Recording ───────────────────────────────────────────────────────────

export interface RecordingStatus {
  active: boolean;
  paused: boolean;
  /** Seconds recorded so far, excluding pauses. */
  elapsed: number;
  output: string | null;
}

export interface FfmpegStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  installHint: string | null;
}

export const ffmpegStatus = () => invoke<FfmpegStatus>("ffmpeg_status");

// ── Video tools ─────────────────────────────────────────────────────────

export type ConvertTarget = "mp4" | "webm" | "mkv" | "gif" | "mp3";

export interface ConvertSettings {
  target: ConvertTarget;
  crf: number;
  /** Null keeps the source frame rate. */
  fps: number | null;
  /** Null keeps the source size; height follows to preserve the aspect ratio. */
  width: number | null;
  mute: boolean;
}

export const defaultConvertSettings = (): ConvertSettings => ({
  target: "mp4",
  crf: 23,
  fps: null,
  width: null,
  mute: false,
});

/** Returns the path written, which is never the source. */
export const convertVideo = (path: string, settings: ConvertSettings) =>
  invoke<string>("convert_video", { path, settings });

export const videoThumbnail = (path: string, atSeconds: number, width?: number) =>
  invoke<string>("video_thumbnail", { path, atSeconds, width });
export const recordingStatus = () => invoke<RecordingStatus>("recording_status");
export const startRecording = (gif = false) =>
  invoke<RecordingStatus>("start_recording", { gif });
export const stopRecording = () => invoke<string>("stop_recording");
export const cancelRecording = () => invoke<void>("cancel_recording");
export const setRecordingPaused = (paused: boolean) =>
  invoke<RecordingStatus>("set_recording_paused", { paused });

export const RECORDING_CHANGED = "kestrel://recording-changed";

export const onRecordingChanged = (
  handler: (status: RecordingStatus) => void,
): Promise<UnlistenFn> =>
  listen<RecordingStatus>(RECORDING_CHANGED, (event) => handler(event.payload));

// ── Tools ───────────────────────────────────────────────────────────────

export interface DecodedQr {
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface FileHash {
  algorithm: string;
  digest: string;
}

export interface Analysis {
  width: number;
  height: number;
  uniqueColours: number;
  uniqueColoursCapped: boolean;
  hasTransparency: boolean;
  dominant: string[];
  averageLuminance: number;
}

export const scanQrCode = () => invoke<DecodedQr[]>("scan_qr_code");
export const generateQrCode = (text: string, moduleSize?: number) =>
  invoke<string>("generate_qr_code", { text, moduleSize });
export const hashFile = (path: string) => invoke<FileHash[]>("hash_file", { path });
export const compareHash = (expected: string, actual: string) =>
  invoke<boolean>("compare_hash", { expected, actual });
export const analyzeLastCapture = () => invoke<Analysis>("analyze_last_capture");

// ── OCR ─────────────────────────────────────────────────────────────────

export interface OcrModelStatus {
  installed: boolean;
  directory: string;
  downloadSizeMb: number;
}

export interface OcrLine {
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Recognised {
  text: string;
  lines: OcrLine[];
}

// ── Colour picker ───────────────────────────────────────────────────────

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** One colour in every notation, so the panel never has to ask again. */
export interface Swatch {
  rgb: Rgb;
  hex: string;
  /** Hue 0–360, saturation and lightness 0–100. */
  hsl: [number, number, number];
  hsv: [number, number, number];
  cmyk: [number, number, number, number];
  luminance: number;
  /** Black or white, whichever stays readable on this colour. */
  contrasting: Rgb;
}

export const pickColor = (x: number, y: number, radius?: number) =>
  invoke<Swatch>("pick_color", { x, y, radius });
export const parseColor = (text: string) => invoke<Swatch>("parse_color", { text });

// ── Image comparer ──────────────────────────────────────────────────────

export interface ImageComparison {
  comparedWidth: number;
  comparedHeight: number;
  sizesDiffer: boolean;
  changedPixels: number;
  totalPixels: number;
  differencePercent: number;
  maxChannelDelta: number;
  bounds: { x: number; y: number; width: number; height: number } | null;
  /** The diff picture as a data URL. */
  preview: string;
}

export const compareImages = (first: string, second: string, tolerance?: number) =>
  invoke<ImageComparison>("compare_images", { first, second, tolerance });

export const ocrStatus = () => invoke<OcrModelStatus>("ocr_status");
export const ocrInstall = () => invoke<OcrModelStatus>("ocr_install");
export const ocrLastCapture = () => invoke<Recognised>("ocr_last_capture");

// ── Events ──────────────────────────────────────────────────────────────
export const onCaptureComplete = (handler: (output: CaptureOutput) => void): Promise<UnlistenFn> =>
  listen<CaptureOutput>(CAPTURE_COMPLETE, (event) => handler(event.payload));

export const onCaptureFailed = (handler: (message: string) => void): Promise<UnlistenFn> =>
  listen<string>(CAPTURE_FAILED, (event) => handler(event.payload));

export const UPLOAD_COMPLETE = "kestrel://upload-complete";

export const onUploadComplete = (handler: (uploaded: Uploaded) => void): Promise<UnlistenFn> =>
  listen<Uploaded>(UPLOAD_COMPLETE, (event) => handler(event.payload));

export const onShortcutsChanged = (
  handler: (reports: ShortcutReport[]) => void,
): Promise<UnlistenFn> =>
  listen<ShortcutReport[]>(SHORTCUTS_CHANGED, (event) => handler(event.payload));

// ── Shortcut formatting ─────────────────────────────────────────────────

export const isMac = () =>
  typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

/**
 * Render an accelerator the way the current platform writes it.
 * `CmdOrCtrl+Shift+2` becomes `⌘⇧2` on macOS and `Ctrl+Shift+2` elsewhere.
 */
export function formatShortcut(accelerator: string | null): string {
  if (!accelerator) return "";
  if (!isMac()) {
    return accelerator.replace(/CmdOrCtrl|CommandOrControl/g, "Ctrl").replace(/\+/g, "+");
  }
  return accelerator
    .split("+")
    .map((part) => {
      switch (part) {
        case "CmdOrCtrl":
        case "CommandOrControl":
        case "Cmd":
        case "Command":
          return "⌘";
        case "Shift":
          return "⇧";
        case "Alt":
        case "Option":
          return "⌥";
        case "Ctrl":
        case "Control":
          return "⌃";
        default:
          return part;
      }
    })
    .join("");
}

/**
 * Turn a keydown into a Tauri accelerator string, or `null` when the user has
 * only pressed modifiers so far (so a recorder can keep waiting).
 *
 * Rejects modifier-less keys: a global shortcut bound to plain `A` would
 * swallow that letter system-wide.
 */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
  const parts: string[] = [];
  if (event.metaKey || event.ctrlKey) parts.push("CmdOrCtrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");

  const key = event.key;
  const isModifier = ["Meta", "Control", "Alt", "Shift"].includes(key);
  if (isModifier || parts.length === 0) return null;

  let code: string;
  if (/^[a-z]$/i.test(key)) {
    code = key.toUpperCase();
  } else if (/^[0-9]$/.test(key)) {
    code = key;
  } else if (/^F\d{1,2}$/.test(key)) {
    code = key;
  } else {
    const named: Record<string, string> = {
      " ": "Space",
      Enter: "Enter",
      Tab: "Tab",
      Backspace: "Backspace",
      Delete: "Delete",
      ArrowUp: "Up",
      ArrowDown: "Down",
      ArrowLeft: "Left",
      ArrowRight: "Right",
      Home: "Home",
      End: "End",
      PageUp: "PageUp",
      PageDown: "PageDown",
      ",": "Comma",
      ".": "Period",
      "/": "Slash",
      ";": "Semicolon",
      "'": "Quote",
      "[": "BracketLeft",
      "]": "BracketRight",
      "\\": "Backslash",
      "-": "Minus",
      "=": "Equal",
      "`": "Backquote",
    };
    const mapped = named[key];
    if (!mapped) return null;
    code = mapped;
  }

  parts.push(code);
  return parts.join("+");
}
