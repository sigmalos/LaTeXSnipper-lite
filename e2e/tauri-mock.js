// Injected before the app loads. Emulates the Tauri IPC bridge with an
// in-memory backend that mirrors backend/src/{commands,store}.rs semantics.
(() => {
  const seed = window.__MOCK_SEED__ || {};
  const callbacks = new Map();
  const listeners = new Map(); // event -> [callbackId]
  let nextId = 1;

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
    models: seed.models || { formula: true, mixed: true, text: true },
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

  const kindFor = (m) => (m === "formula" ? "mathcraft" : m === "mixed" ? "mathcraft_mixed" : "mathcraft_text");

  function emit(event, payload) {
    for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, id, payload });
  }
  state.emit = emit;

  function nextOutcome(mode) {
    const o = state.outcomes.length ? state.outcomes.shift() : { text: "x^{2}" };
    if (o === null) return null;
    if (o.error) throw o.error;
    if (!state.models[mode] && state.settings.backend === "builtin") throw "MODELS_MISSING:mathcraft-formula-rec";
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
      models: { ...state.models },
      pandoc: !!seed.pandoc,
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
      for (let i = 1; i <= 4; i++) {
        emit("model-progress", { model: "mathcraft-formula-rec", downloaded: i * 25 * 1048576, total: 100 * 1048576 });
        await new Promise((r) => setTimeout(r, 30));
      }
      if (seed.downloadFails) throw "network unreachable";
      state.models = { formula: true, mixed: true, text: true };
      return { ...state.models, [mode]: true };
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
