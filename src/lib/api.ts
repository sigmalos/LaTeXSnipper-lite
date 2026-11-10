// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
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
}

export interface Snapshot {
  version: string;
  settings: Settings;
  history: Entry[];
  favorites: Entry[];
  models: Record<Mode, boolean>;
  pandoc: boolean;
  data_dir: string;
}

export interface Recognition {
  text: string;
  score: number;
  mode: Mode;
  flags: string[];
  kind: Kind;
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
  capture: (mode?: Mode) => invoke<Recognition | null>("capture", { mode }),
  downloadModels: (mode: Mode) => invoke<Record<Mode, boolean>>("download_models", { mode }),
  testExternal: (settings: Settings) => invoke<string>("test_external", { settings }),
  exportFile: (path: string, content: string, format: string) => invoke<void>("export_file", { path, content, format }),
  history: (op: ListOp) => invoke<Entry[]>("history_op", op),
  favorites: (op: ListOp) => invoke<Entry[]>("favorite_op", op),
  suspendHotkey: () => invoke<void>("suspend_hotkey"),
  resumeHotkey: () => invoke<void>("resume_hotkey"),
  copy: (text: string) => writeText(text),
  saveDialog: (defaultPath: string, ext: string, label: string) =>
    save({ defaultPath, filters: [{ name: label, extensions: [ext] }] }),
  onProgress: (cb: (p: Progress) => void): Promise<UnlistenFn> => listen<Progress>("model-progress", (e) => cb(e.payload)),
  onHotkey: (cb: () => void): Promise<UnlistenFn> => listen("hotkey-capture", () => cb()),
  onOpenSettings: (cb: () => void): Promise<UnlistenFn> => listen("open-settings", () => cb()),
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
