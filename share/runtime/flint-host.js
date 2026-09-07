#!/usr/bin/env node
// Flint extension host.
//
// Runs one Vicinae / Raycast-style extension command inside Node, with the
// real `react` and the real `@vicinae/api` SDK. The SDK renders to host
// elements ("list", "list-item", "action", ...) and calls back into
// `globalThis.vicinae.client`; this file provides both halves and speaks a
// small newline-delimited JSON protocol on stdio to the Flint process.
//
//   stdout  host -> Flint   {"type": "render" | "toast" | "hud" | "request" | ...}
//   stdin   Flint -> host   first line = JSON config, then
//                           {"type": "invoke" | "response" | "pop" | "quit"}
//
// Extension console output is redirected to stderr so it can never corrupt
// the protocol stream. Config is never on argv (it can contain preferences).
//
// Usage: node flint-host.js

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { spawn } = require("node:child_process");

let cfg = null;

const realStdoutWrite = process.stdout.write.bind(process.stdout);
function send(msg) {
  realStdoutWrite(JSON.stringify(msg) + "\n");
}

for (const level of ["log", "info", "warn", "error", "debug"]) {
  console[level] = (...args) => {
    try {
      process.stderr.write(
        `[ext ${level}] ${args.map((a) => (typeof a === "string" ? a : safeInspect(a))).join(" ")}\n`
      );
    } catch {
      /* ignore */
    }
  };
}
process.stdout.write = (chunk, ...rest) => process.stderr.write(chunk, ...rest);

function safeInspect(value) {
  try {
    return require("node:util").inspect(value, { depth: 3 });
  } catch {
    return String(value);
  }
}

let nextRequest = 1;
const pending = new Map();
function request(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextRequest++;
    pending.set(id, { resolve, reject });
    send({ type: "request", id, method, params: params ?? {} });
  });
}

function reportError(err) {
  const message = err && err.stack ? err.stack : String(err);
  console.error(message);
  send({ type: "error", message: String(err && err.message ? err.message : err) });
}

const notImplemented = (name) => async () => {
  console.error(`${name} is not implemented in Flint`);
  return undefined;
};

function persistStorage(storageDir, storage) {
  try {
    fs.mkdirSync(storageDir, { recursive: true, mode: 0o700 });
    fs.writeFileSync(path.join(storageDir, "local-storage.json"), JSON.stringify(storage), {
      mode: 0o600,
    });
  } catch (err) {
    console.error("storage write failed", err);
  }
}

function boot() {
  const RUNTIME = cfg.runtime;
  const EXT_DIR = cfg.extensionDir;
  const ENTRY = cfg.entry;
  const MODE = cfg.mode || "view";
  const STORAGE_DIR = cfg.storageDir;

  const SHARED = {
    react: "react",
    "react/jsx-runtime": "react/jsx-runtime",
    "react/jsx-dev-runtime": "react/jsx-dev-runtime",
    "@vicinae/api": "@vicinae/api",
    "@raycast/api": "@vicinae/api",
    "react-reconciler": "react-reconciler",
    "react-reconciler/constants": "react-reconciler/constants",
  };
  const runtimeParent = {
    id: path.join(RUNTIME, "index.js"),
    filename: path.join(RUNTIME, "index.js"),
    paths: Module._nodeModulePaths(RUNTIME),
  };
  const originalResolve = Module._resolveFilename;
  Module._resolveFilename = function (req, parent, ...rest) {
    const alias = SHARED[req];
    if (alias) {
      return originalResolve.call(this, alias, runtimeParent, ...rest);
    }
    return originalResolve.call(this, req, parent, ...rest);
  };

  const React = require("react");
  const Reconciler = require("react-reconciler");
  const { DefaultEventPriority, NoEventPriority } = require("react-reconciler/constants");

  const storageFile = path.join(STORAGE_DIR, "local-storage.json");
  let storage = {};
  try {
    storage = JSON.parse(fs.readFileSync(storageFile, "utf8")) || {};
  } catch {
    storage = {};
  }

  const client = {
    UI: {
      async showToast(id, title, message, style) {
        send({ type: "toast", id, title, message, style });
      },
      async hideToast(id) {
        send({ type: "hideToast", id });
      },
      async showHud(title) {
        send({ type: "hud", title });
      },
      async closeMainWindow() {
        send({ type: "closeWindow" });
      },
      async setSearchText(text) {
        send({ type: "setSearchText", text: text ?? "" });
      },
      async getSelectedText() {
        const res = await request("ui.getSelectedText");
        if (typeof res === "string") return res;
        return res && typeof res.text === "string" ? res.text : "";
      },
      async popToRoot() {
        send({ type: "popToRoot" });
      },
      async sendDesktopNotification(payload) {
        const args = [payload?.title ?? "Flint", payload?.body ?? ""];
        spawn("notify-send", args, { stdio: "ignore", detached: true }).unref();
      },
      async confirmAlert(options) {
        const res = await request("ui.confirmAlert", options);
        if (typeof res === "boolean") return res;
        return Boolean(res && res.confirmed);
      },
    },
    Storage: {
      async get(key) {
        return storage[key];
      },
      async set(key, value) {
        storage[key] = value;
        persistStorage(STORAGE_DIR, storage);
      },
      async remove(key) {
        delete storage[key];
        persistStorage(STORAGE_DIR, storage);
      },
      async list() {
        return { ...storage };
      },
      async clear() {
        storage = {};
        persistStorage(STORAGE_DIR, storage);
      },
    },
    Clipboard: {
      async copy(content, options) {
        await request("clipboard.copy", { content, options: options ?? {} });
      },
      async paste(content) {
        await request("clipboard.paste", { content });
      },
      async readContent() {
        const res = await request("clipboard.read");
        return res ?? { text: "" };
      },
      async clear() {
        await request("clipboard.copy", { content: { text: "" }, options: {} });
      },
    },
    Application: {
      async open(target, appId) {
        await request("app.open", { target: String(target), appId: appId ?? null });
      },
      async runInTerminal(options) {
        await request("app.runInTerminal", options ?? {});
      },
      async list() {
        return [];
      },
      async getDefault() {
        return null;
      },
      async showInFileBrowser(p) {
        await request("app.open", { target: path.dirname(String(p)), appId: null });
      },
    },
    Command: {
      launchCommand: notImplemented("launchCommand"),
      openCommandPreferences: notImplemented("openCommandPreferences"),
      openExtensionPreferences: notImplemented("openExtensionPreferences"),
      updateCommandMetadata: async () => {},
    },
    OAuth: {
      authorize: async () => {
        throw new Error("OAuth for extensions is not supported by Flint yet");
      },
      getTokens: async () => null,
      setTokens: async () => {},
      removeTokens: async () => {},
    },
    FileSearch: {
      async search() {
        return [];
      },
    },
    WindowManagement: {
      getActiveWindow: notImplemented("WindowManagement.getActiveWindow"),
      focusWindow: notImplemented("WindowManagement.focusWindow"),
    },
    Wallpaper: { set: notImplemented("Wallpaper.set") },
    BrowserExtension: { focusTab: notImplemented("BrowserExtension.focusTab") },
  };

  const navigationContext = React.createContext({ push() {}, pop() {} });

  globalThis.vicinae = {
    client,
    navigationContext,
    preferences: cfg.preferences || {},
    environ: {
      raycastVersion: "1.80.0",
      vicinaeVersion: { major: 0, minor: 28, patch: 0, tag: "flint" },
      ownerOrAuthorName: cfg.author || "",
      extensionName: cfg.extensionName || "",
      commandName: cfg.commandName || "",
      commandMode: MODE,
      assetsPath: path.join(EXT_DIR, "assets"),
      supportPath: STORAGE_DIR,
      isDevelopment: false,
      appearance: "dark",
      theme: "dark",
      textSize: "medium",
      launchType: "userInitiated",
      canAccess: () => false,
    },
  };

  let nextNode = 1;
  const nodes = new Map();

  function makeInstance(type, props) {
    const inst = { id: nextNode++, type, props: {}, fns: {}, children: [] };
    nodes.set(inst.id, inst);
    applyProps(inst, props);
    return inst;
  }

  function applyProps(inst, props) {
    const plain = {};
    const fns = {};
    for (const [key, value] of Object.entries(props || {})) {
      if (key === "children") continue;
      if (typeof value === "function") {
        fns[key] = value;
        plain[key] = { $cb: [inst.id, key] };
      } else if (value !== undefined && !React.isValidElement(value)) {
        plain[key] = value;
      }
    }
    inst.props = plain;
    inst.fns = fns;
  }

  const container = { children: [] };
  let flushScheduled = false;
  function scheduleFlush() {
    if (flushScheduled) return;
    flushScheduled = true;
    setImmediate(() => {
      flushScheduled = false;
      flush();
    });
  }

  function serialize(inst) {
    if (inst.text !== undefined) {
      return { type: "#text", text: inst.text };
    }
    return {
      id: inst.id,
      type: inst.type,
      props: inst.props,
      children: inst.children.map(serialize),
    };
  }

  let navDepth = 1;
  function flush() {
    send({
      type: "render",
      depth: navDepth,
      root: container.children.map(serialize),
    });
  }

  let currentPriority = NoEventPriority;
  const hostConfig = {
    supportsMutation: true,
    supportsPersistence: false,
    supportsHydration: false,
    isPrimaryRenderer: true,
    noTimeout: -1,
    scheduleTimeout: setTimeout,
    cancelTimeout: clearTimeout,
    scheduleMicrotask: queueMicrotask,

    getRootHostContext: () => ({}),
    getChildHostContext: (ctx) => ctx,
    getPublicInstance: (inst) => inst,
    prepareForCommit: () => null,
    resetAfterCommit: () => scheduleFlush(),
    shouldSetTextContent: () => false,
    clearContainer: (c) => {
      c.children = [];
    },

    createInstance: (type, props) => makeInstance(type, props),
    createTextInstance: (text) => ({ text }),
    appendInitialChild: (parent, child) => parent.children.push(child),
    finalizeInitialChildren: () => false,
    prepareUpdate: () => true,
    commitUpdate: (inst, type, oldProps, newProps) => {
      applyProps(inst, newProps);
    },
    commitTextUpdate: (inst, _old, next) => {
      inst.text = next;
    },
    commitMount: () => {},

    appendChild: (parent, child) => parent.children.push(child),
    appendChildToContainer: (c, child) => c.children.push(child),
    insertBefore: (parent, child, before) => {
      const i = parent.children.indexOf(before);
      parent.children.splice(i < 0 ? parent.children.length : i, 0, child);
    },
    insertInContainerBefore: (c, child, before) => {
      const i = c.children.indexOf(before);
      c.children.splice(i < 0 ? c.children.length : i, 0, child);
    },
    removeChild: (parent, child) => {
      parent.children = parent.children.filter((c) => c !== child);
    },
    removeChildFromContainer: (c, child) => {
      c.children = c.children.filter((x) => x !== child);
    },
    detachDeletedInstance: (inst) => {
      if (inst && inst.id) nodes.delete(inst.id);
    },
    hideInstance: () => {},
    unhideInstance: () => {},
    hideTextInstance: () => {},
    unhideTextInstance: () => {},
    resetTextContent: () => {},

    getCurrentUpdatePriority: () => currentPriority,
    setCurrentUpdatePriority: (p) => {
      currentPriority = p;
    },
    resolveUpdatePriority: () =>
      currentPriority !== NoEventPriority ? currentPriority : DefaultEventPriority,
    getCurrentEventPriority: () => DefaultEventPriority,
    shouldAttemptEagerTransition: () => false,
    trackSchedulerEvent: () => {},
    resolveEventType: () => null,
    resolveEventTimeStamp: () => -1.1,
    requestPostPaintCallback: () => {},
    maySuspendCommit: () => false,
    preloadInstance: () => true,
    startSuspendingCommit: () => {},
    suspendInstance: () => {},
    waitForCommitToBeReady: () => null,
    NotPendingTransition: null,
    HostTransitionContext: React.createContext(null),
    resetFormInstance: () => {},
    getInstanceFromNode: () => null,
    beforeActiveInstanceBlur: () => {},
    afterActiveInstanceBlur: () => {},
    prepareScopeUpdate: () => {},
    getInstanceFromScope: () => null,
    preparePortalMount: () => {},
  };

  const reconciler = Reconciler(hostConfig);
  const root = reconciler.createContainer(
    container,
    0,
    null,
    false,
    null,
    "flint",
    (err) => reportError(err),
    (err) => reportError(err),
    (err) => reportError(err),
    null
  );

  let navApi = { push() {}, pop() {} };

  function Root({ initial }) {
    const [stack, setStack] = React.useState([{ key: 1, element: initial }]);
    const push = React.useCallback((element) => {
      setStack((s) => [...s, { key: s[s.length - 1].key + 1, element }]);
    }, []);
    const pop = React.useCallback(() => {
      setStack((s) => (s.length > 1 ? s.slice(0, -1) : s));
    }, []);
    navApi = { push, pop };
    navDepth = stack.length;
    const value = React.useMemo(() => ({ push, pop }), [push, pop]);
    return React.createElement(
      navigationContext.Provider,
      { value },
      stack.map((frame) =>
        React.createElement(
          "nav-frame",
          { key: frame.key },
          React.createElement(ErrorBoundary, null, frame.element)
        )
      )
    );
  }

  class ErrorBoundary extends React.Component {
    constructor(props) {
      super(props);
      this.state = { error: null };
    }
    static getDerivedStateFromError(error) {
      return { error };
    }
    componentDidCatch(error) {
      reportError(error);
    }
    render() {
      if (this.state.error) {
        return React.createElement("detail", {
          markdown: `# Extension crashed\n\n${String(this.state.error && this.state.error.message)}`,
        });
      }
      return this.props.children;
    }
  }

  function handle(msg) {
    switch (msg.type) {
      case "invoke": {
        const inst = nodes.get(msg.node);
        const fn = inst && inst.fns[msg.prop];
        if (typeof fn !== "function") return;
        try {
          const result = fn(...(msg.args || []));
          if (result && typeof result.then === "function") {
            result.catch(reportError);
          }
        } catch (err) {
          reportError(err);
        }
        return;
      }
      case "response": {
        const p = pending.get(msg.id);
        if (!p) return;
        pending.delete(msg.id);
        if (msg.error) p.reject(new Error(msg.error));
        else p.resolve(msg.result);
        return;
      }
      case "pop":
        navApi.pop();
        return;
      case "quit":
        process.exit(0);
        return;
      default:
        return;
    }
  }

  stdinHandle = handle;

  let mod;
  try {
    mod = require(ENTRY);
  } catch (err) {
    reportError(err);
    send({ type: "done" });
    return;
  }
  const Command = mod && (mod.default || mod);
  const launchProps = {
    arguments: cfg.arguments || {},
    launchType: "userInitiated",
    launchContext: undefined,
    fallbackText: cfg.fallbackText,
  };

  if (MODE === "no-view") {
    Promise.resolve()
      .then(() => (typeof Command === "function" ? Command(launchProps) : undefined))
      .catch(reportError)
      .finally(() => {
        setTimeout(() => send({ type: "done" }), 50);
      });
    return;
  }

  if (typeof Command !== "function") {
    reportError(new Error("Extension command has no default export"));
    send({ type: "done" });
    return;
  }
  const element = React.createElement(Root, {
    initial: React.createElement(Command, launchProps),
  });
  reconciler.updateContainer(element, root, null, () => {
    send({ type: "ready" });
  });
}

let stdinHandle = null;
let buffered = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffered += chunk;
  let nl;
  while ((nl = buffered.indexOf("\n")) >= 0) {
    const line = buffered.slice(0, nl).trim();
    buffered = buffered.slice(nl + 1);
    if (!line) continue;
    if (!cfg) {
      try {
        cfg = JSON.parse(line);
      } catch (err) {
        reportError(err);
        send({ type: "done" });
        return;
      }
      send({ type: "hello", pid: process.pid });
      try {
        boot();
      } catch (err) {
        reportError(err);
        send({ type: "done" });
      }
      continue;
    }
    if (!stdinHandle) continue;
    try {
      stdinHandle(JSON.parse(line));
    } catch (err) {
      console.error("bad message from flint", err);
    }
  }
});
process.stdin.on("end", () => process.exit(0));
process.on("uncaughtException", reportError);
process.on("unhandledRejection", reportError);
