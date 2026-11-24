// Typed wrappers around the Rust commands in backend/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Entry, Kind, Mode } from "./format";

export interface Settings {
  mode: Mode;
  backend: "builtin" | "external";
  theme: "auto" | "light" | "dark";
  language: "zh" | "en";
  hotkey: string;
  history_reverse: boolean;
  pdf_dpi: number;
  external_base_url: string;
  external_model: string;
  external_api_key: string;
  external_timeout_sec: number;
  external_custom_prompt: string;
  onboarding_done: boolean;
  /** The model id chosen for each part of local recognition. */
  local_models: LocalModels;
}

/** The four parts of local recognition (backend `Slot`). */
export type Slot = "formula_det" | "formula_rec" | "text_det" | "text_rec";
export type LocalModels = Record<Slot, string>;

export interface Snapshot {
  version: string;
  settings: Settings;
  history: Entry[];
  favorites: Entry[];
  models: Record<Mode, boolean>;
  model_files: ModelInfo[];
  pandoc: boolean;
  capture_supported: boolean;
  selfcheck: boolean;
  data_dir: string;
  /** Release notes of the version this launch updated to, shown once. */
  changelog: { version: string; notes: string } | null;
}

/** Self-update state, as the Settings button shows it. */
export type UpdateView = { installs_on_quit: boolean } & (
  | { phase: "idle" | "checking" | "up_to_date" }
  | { phase: "available" | "ready"; version: string }
  | { phase: "downloading"; version: string; done: number; total: number | null }
  | { phase: "failed"; error: string }
);

export interface Recognition {
  text: string;
  score: number;
  mode: Mode;
  flags: string[];
  kind: Kind;
}

/** A background capture waiting in the result window. */
export interface PendingView {
  result: Recognition;
  theme: Settings["theme"];
  language: Settings["language"];
}

/** Sent by the result window once its result is copied and saved to history. */
export interface ConfirmedResult {
  text: string;
  kind: Kind;
  history: Entry[];
}

export interface ModelInfo {
  id: string;
  /** The part this model can fill. */
  slot: Slot;
  /** Display name in that part's dropdown. */
  name: string;
  installed: boolean;
  downloading: boolean;
  download_bytes: number;
  used_by: Mode[];
}

export interface ModelsState {
  models: ModelInfo[];
  modes: Record<Mode, boolean>;
}

export interface Progress {
  model: string;
  downloaded: number;
  total: number;
}

type ListOp = { op: "add"; text: string; kind: Kind; name?: string } | { op: "remove" | "rename"; index: number; name?: string } | { op: "clear" };

export const api = {
  bootstrap: () => invoke<Snapshot>("bootstrap"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  recognizeImage: (data: string, mode?: Mode) => invoke<Recognition>("recognize_image", { data, mode }),
  recognizeFile: (path: string, mode?: Mode) => invoke<Recognition>("recognize_file", { path, mode }),
  /** Null when cancelled, or when the result went to the result window instead. */
  capture: (mode?: Mode) => invoke<Recognition | null>("capture", { mode }),
  takeResult: () => invoke<PendingView | null>("take_result"),
  presentResult: () => invoke<void>("present_result"),
  dismissResult: () => invoke<void>("dismiss_result"),
  openScreenSettings: () => invoke<void>("open_screen_settings"),
  updateStatus: () => invoke<UpdateView>("update_status"),
  checkUpdate: () => invoke<UpdateView>("check_update"),
  /** Downloads if needed, installs and restarts; does not return on success. */
  applyUpdate: () => invoke<void>("apply_update"),
  dismissChangelog: () => invoke<void>("dismiss_changelog"),
  resultConfirmed: (r: ConfirmedResult) => emit("result-confirmed", r),
  downloadModels: (mode: Mode) => invoke<ModelsState>("download_models", { mode }),
  listModels: () => invoke<ModelsState>("list_models"),
  downloadModel: (model: string) => invoke<ModelsState>("download_model", { model }),
  deleteModel: (model: string) => invoke<ModelsState>("delete_model", { model }),
  frontendReady: (ok: boolean, detail: string) => invoke<void>("frontend_ready", { ok, detail }),
  testExternal: (settings: Settings) => invoke<string>("test_external", { settings }),
  exportFile: (path: string, content: string, format: string) => invoke<void>("export_file", { path, content, format }),
  history: (op: ListOp) => invoke<Entry[]>("history_op", op),
  favorites: (op: ListOp) => invoke<Entry[]>("favorite_op", op),
  suspendHotkey: () => invoke<void>("suspend_hotkey"),
  resumeHotkey: () => invoke<void>("resume_hotkey"),
  /** Native appearance for window chrome, menus and form controls; null = follow system. */
  setWindowTheme: (theme: "light" | "dark" | null) => getCurrentWindow().setTheme(theme),
  copy: (text: string) => writeText(text),
  saveDialog: (defaultPath: string, ext: string, label: string) =>
    save({ defaultPath, filters: [{ name: label, extensions: [ext] }] }),
  onProgress: (cb: (p: Progress) => void): Promise<UnlistenFn> => listen<Progress>("model-progress", (e) => cb(e.payload)),
  onModelsState: (cb: (s: ModelsState) => void): Promise<UnlistenFn> =>
    listen<ModelsState>("models-state", (e) => cb(e.payload)),
  onHotkey: (cb: () => void): Promise<UnlistenFn> => listen("hotkey-capture", () => cb()),
  onResultReady: (cb: () => void): Promise<UnlistenFn> => listen("result-ready", () => cb()),
  onResultConfirmed: (cb: (r: ConfirmedResult) => void): Promise<UnlistenFn> =>
    listen<ConfirmedResult>("result-confirmed", (e) => cb(e.payload)),
  onOpenSettings: (cb: () => void): Promise<UnlistenFn> => listen("open-settings", () => cb()),
  onUpdateStatus: (cb: (u: UpdateView) => void): Promise<UnlistenFn> =>
    listen<UpdateView>("update-status", (e) => cb(e.payload)),
  onDrop: async (cb: (paths: string[]) => void, hover: (on: boolean) => void): Promise<void> => {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "over" || p.type === "enter") hover(true);
      else if (p.type === "leave") hover(false);
      else if (p.type === "drop") {
        hover(false);
        cb(p.paths);
      }
    });
  },
};
