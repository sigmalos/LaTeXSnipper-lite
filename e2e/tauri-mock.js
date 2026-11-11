// Injected before the app loads. Emulates the Tauri IPC bridge with an
// in-memory backend that mirrors backend/src/{commands,store}.rs semantics.
(() => {
  const seed = window.__MOCK_SEED__ || {};
  const callbacks = new Map();
  const listeners = new Map(); // event -> [callbackId]
  let nextId = 1;

  const ALL_MODELS = ["mathcraft-formula-det", "mathcraft-formula-rec", "mathcraft-text-det", "mathcraft-text-rec"];
  const REQUIRED = {
    formula: ["mathcraft-formula-rec"],
    mixed: ALL_MODELS,
    text: ["mathcraft-text-det", "mathcraft-text-rec"],
  };
  const SIZES = {
    "mathcraft-formula-det": 67531517,
    "mathcraft-formula-rec": 108795631,
    "mathcraft-text-det": 4533780,
    "mathcraft-text-rec": 14102746,
  };

  const state = {
    settings: {
      mode: "formula",
      backend: "builtin",
      theme: "light",
      language: "zh",
      hotkey: "CommandOrControl+F",
      history_reverse: true,
      pdf_dpi: 200,
      external_base_url: "http://localhost:11434/v1",
      external_model: "",
      external_api_key: "",
      external_timeout_sec: 60,
      external_custom_prompt: "",
      onboarding_done: true,
      ...(seed.settings || {}),
    },
    history: seed.history || [],
    favorites: seed.favorites || [],
    // Seeds may describe ready recognition types ({formula: true, ...}); derive the files.
    installed: new Set(
      seed.installedModels ??
        (seed.models ? ALL_MODELS.filter((id) => Object.entries(seed.models).some(([m, on]) => on && REQUIRED[m].includes(id))) : ALL_MODELS),
    ),
    downloading: new Set(),
    // ids whose next download fails once (or every download when downloadFails is true)
    failOnce: new Set(seed.failDownloads || []),
    clipboard: "",
    hotkeySuspended: false,
    windowTheme: "unset",
    calls: [],
    // queue of scripted outcomes for capture / recognize: {text, score?, flags?} | null | {error}
    outcomes: seed.outcomes || [],
    savePath: seed.savePath ?? "/tmp/out.file",
    exported: [],
  };
  window.__mock = state;

  function modes() {
    const out = {};
    for (const [m, ids] of Object.entries(REQUIRED)) out[m] = ids.every((id) => state.installed.has(id));
    return out;
  }
  function modelsState() {
    return {
      models: ALL_MODELS.map((id) => ({
        id,
        installed: state.installed.has(id),
        downloading: state.downloading.has(id),
        download_bytes: SIZES[id],
        used_by: Object.keys(REQUIRED).filter((m) => REQUIRED[m].includes(id)),
      })),
      modes: modes(),
    };
  }
  async function downloadOne(id) {
    if (state.downloading.has(id)) throw `DOWNLOAD_IN_PROGRESS:${id}`;
    state.downloading.add(id);
    try {
      const total = SIZES[id];
      for (let i = 1; i <= 4; i++) {
        await new Promise((r) => setTimeout(r, seed.downloadDelay ?? 30));
        emit("model-progress", { model: id, downloaded: Math.round((total * i) / 4), total });
      }
      if (seed.downloadFails || state.failOnce.delete(id)) throw "network unreachable";
      state.installed.add(id);
    } finally {
      state.downloading.delete(id);
    }
  }

  const kindFor = (m) => (m === "formula" ? "mathcraft" : m === "mixed" ? "mathcraft_mixed" : "mathcraft_text");

  function emit(event, payload) {
    for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, id, payload });
  }
  state.emit = emit;

  function nextOutcome(mode) {
    const o = state.outcomes.length ? state.outcomes.shift() : { text: "x^{2}" };
    if (o === null) return null;
    if (o.error) throw o.error;
    if (state.settings.backend === "builtin") {
      const missing = REQUIRED[mode].filter((id) => !state.installed.has(id));
      if (missing.length) throw `MODELS_MISSING:${missing.join(",")}`;
    }
    return { text: o.text, score: o.score ?? 0.97, mode, flags: o.flags || [], kind: kindFor(mode) };
  }

  function listOp(list, args, isHistory) {
    const { op, index, text, kind, name } = args;
    if (op === "add") {
      const t = (text || "").trim();
      if (!t) throw "empty content";
      const i = list.findIndex((e) => e.text === t);
      if (isHistory) {
        const prev = i >= 0 ? list.splice(i, 1)[0].name : "";
        list.push({ text: t, name: prev, kind: kind || "mathcraft" });
      } else if (i < 0) list.push({ text: t, name: name || "", kind: kind || "mathcraft" });
    } else if (op === "remove") list.splice(index, 1);
    else if (op === "rename") list[index].name = (name || "").trim();
    else if (op === "clear") list.length = 0;
    return structuredClone(list);
  }

  const handlers = {
    bootstrap: () => ({
      version: "4.0.0",
      settings: structuredClone(state.settings),
      history: structuredClone(state.history),
      favorites: structuredClone(state.favorites),
      models: modes(),
      model_files: modelsState().models,
      pandoc: !!seed.pandoc,
      selfcheck: false,
      data_dir: "/Users/demo/Library/Application Support/LaTeXSnipper",
    }),
    save_settings: ({ settings }) => {
      if (settings.hotkey === "CommandOrControl+Alt+Q") throw "HOTKEY_INVALID:already registered";
      state.settings = structuredClone(settings);
      return settings;
    },
    capture: ({ mode }) => nextOutcome(mode || state.settings.mode),
    recognize_image: ({ data, mode }) => {
      if (!/^data:image\/png;base64,/.test(data) && !/^data:image\//.test(data)) throw "invalid image data";
      return nextOutcome(mode || state.settings.mode);
    },
    recognize_file: ({ mode }) => nextOutcome(mode || state.settings.mode),
    download_models: async ({ mode }) => {
      for (const id of REQUIRED[mode]) if (!state.installed.has(id)) await downloadOne(id);
      return modelsState();
    },
    list_models: () => modelsState(),
    delete_model: ({ model }) => {
      if (!SIZES[model]) throw `unknown model ${model}`;
      if (state.downloading.has(model)) throw `DOWNLOAD_IN_PROGRESS:${model}`;
      if (seed.deleteFails) throw "Operation not permitted (os error 1)";
      state.installed.delete(model);
      return modelsState();
    },
    frontend_ready: () => undefined,
    download_model: async ({ model }) => {
      if (!SIZES[model]) throw `unknown model ${model}`;
      await downloadOne(model);
      return modelsState();
    },
    test_external: ({ settings }) => {
      if (!settings.external_model) throw "external model name is required";
      return "OK (3 models)";
    },
    export_file: ({ path, content, format }) => {
      state.exported.push({ path, content, format });
    },
    suspend_hotkey: () => {
      state.hotkeySuspended = true;
    },
    resume_hotkey: () => {
      state.hotkeySuspended = false;
    },
    history_op: (a) => listOp(state.history, a, true),
    favorite_op: (a) => listOp(state.favorites, a, false),
    "plugin:clipboard-manager|write_text": ({ text }) => {
      state.clipboard = text;
    },
    "plugin:dialog|save": () => state.savePath,
    "plugin:window|set_theme": ({ value }) => {
      state.windowTheme = value;
    },
    "plugin:event|listen": ({ event, handler }) => {
      if (!listeners.has(event)) listeners.set(event, []);
      listeners.get(event).push(handler);
      return handler;
    },
    "plugin:event|unlisten": ({ event, eventId }) => {
      listeners.set(event, (listeners.get(event) || []).filter((id) => id !== eventId));
    },
  };

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback(cb, once) {
      const id = nextId++;
      callbacks.set(id, (p) => {
        if (once) callbacks.delete(id);
        return cb(p);
      });
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    convertFileSrc: (p) => p,
    async invoke(cmd, args = {}) {
      state.calls.push({ cmd, args: structuredClone(args) });
      const h = handlers[cmd];
      if (!h) throw `mock: unhandled command ${cmd}`;
      await new Promise((r) => setTimeout(r, 5));
      return h(args);
    },
  };
})();
