/**
 * Typed wrappers around the Rust IPC surface (`src-tauri/src/commands.rs`).
 *
 * These types are hand-written for now; once the surface stabilises they will
 * be generated from the Rust definitions with `ts-rs` so the two halves can
 * never drift apart. See docs/00-PLAN.md §3.
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
}

export interface Capabilities {
  window_enumeration: boolean;
  window_capture: boolean;
  region_capture: boolean;
  global_shortcuts: boolean;
  scrolling_capture: boolean;
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
  filename_pattern: string;
  quality: number;
}

export interface Workflow {
  id: string;
  name: string;
  shortcut: string | null;
  method: CaptureMethod;
  settings: TaskSettings;
  enabled: boolean;
}

export const CAPTURE_COMPLETE = "kestrel://capture-complete";
export const CAPTURE_FAILED = "kestrel://capture-failed";

export const listDisplays = () => invoke<DisplayInfo[]>("list_displays");
export const listWindows = () => invoke<WindowInfo[]>("list_windows");
export const platformCapabilities = () => invoke<Capabilities>("platform_capabilities");
export const listWorkflows = () => invoke<Workflow[]>("list_workflows");

export const capture = (method: CaptureMethod) =>
  invoke<CaptureOutput>("capture", { method });

export const captureRegion = (region: Region) =>
  invoke<CaptureOutput>("capture_region", { region });

export const previewFilename = (pattern: string) =>
  invoke<string>("preview_filename", { pattern });

export const onCaptureComplete = (handler: (output: CaptureOutput) => void): Promise<UnlistenFn> =>
  listen<CaptureOutput>(CAPTURE_COMPLETE, (event) => handler(event.payload));

export const onCaptureFailed = (handler: (message: string) => void): Promise<UnlistenFn> =>
  listen<string>(CAPTURE_FAILED, (event) => handler(event.payload));

/**
 * Render an accelerator the way the current platform writes it.
 * `CmdOrCtrl+Shift+2` becomes `⌘⇧2` on macOS and `Ctrl+Shift+2` elsewhere.
 */
export function formatShortcut(accelerator: string | null): string {
  if (!accelerator) return "";
  const isMac = navigator.userAgent.includes("Mac");
  if (!isMac) {
    return accelerator.replace(/CmdOrCtrl|CommandOrControl/g, "Ctrl");
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
