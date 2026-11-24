import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runInThisContext } from "node:vm";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { transformWithEsbuild } from "vite";

// Exercise the actual App and controls without WebView, registry, network or timers.
// Compilation alone cannot catch a conditional accidentally rendered as JSX text.
const source = fileURLToPath(new URL("../src/", import.meta.url));
const require = createRequire(import.meta.url);
const compiled = new Map();
for (const file of readdirSync(source).filter((name) => /\.tsx?$/.test(name))) {
  const filename = path.join(source, file);
  const { code } = await transformWithEsbuild(readFileSync(filename, "utf8"), filename, {
    format: "cjs", jsx: "automatic",
  });
  compiled.set(filename, runInThisContext(`(function(require,module,exports){${code}\n})`, { filename }));
}
const texts = Object.fromEntries(["en", "es", "fr"].map((language) => [
  language, JSON.parse(readFileSync(path.join(source, "i18n", `${language}.json`), "utf8")),
]));
const base = {
  language: "en", menuBar: true, smartHide: false, notificationBadges: true, notificationBounce: true,
  iconSize: 48, magnifiedSize: 77,
  magnification: true, dockPosition: "bottom", genie: true, genieSpeed: "normal",
  dockDensity: "normal", dockAutoHideSpeed: "normal", showStart: true,
  showTrash: true, pinExplorer: true,
  showDownloads: true,
  showOtherWorkspaces: false,
  menuBarIslandStyle: "notch", checkUpdates: false,
  pinned: [], recent: [], shortcutsFolder: null,
};
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
function* elements(node) {
  if (Array.isArray(node)) { for (const child of node) yield* elements(child); }
  else if (React.isValidElement(node)) { yield node; yield* elements(node.props.children); }
}

function fixture(section = "menubar", config = {}, save = async () => {}) {
  const states = [], refs = [], effects = [], timers = new Map(), calls = [], listeners = new Map();
  let stateIndex = 0, refIndex = 0, nextTimer = 0, close;
  const loaded = { ...base, ...config };
  const browser = {
    addEventListener(event, callback) { assert.ok(!listeners.has(event)); listeners.set(event, callback); },
    removeEventListener(event) { listeners.delete(event); },
    setTimeout(callback, delay) { const id = ++nextTimer; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
  };
  globalThis.window = browser;
  const hooks = {
    ...React,
    useState(initial) {
      const index = stateIndex++;
      if (!(index in states)) states[index] = index === 0 ? { ...loaded } : initial === "dock" ? section : initial;
      return [states[index], (value) => { states[index] = typeof value === "function" ? value(states[index]) : value; }];
    },
    useRef(initial) { const index = refIndex++; return refs[index] ??= { current: initial }; },
    useCallback(callback) { return callback; },
    useEffect(callback) { effects.push(callback); },
  };
  const nativeWindow = { onCloseRequested(callback) { close = callback; return Promise.resolve(() => {}); } };
  const mocks = {
    "@tauri-apps/api/app": { getVersion: async () => "0.1.21" },
    "@tauri-apps/api/event": { listen: async () => () => {} },
    "@tauri-apps/api/window": { getCurrentWindow: () => nativeWindow },
    "@tauri-apps/api/core": { invoke: async (command, args) => {
      switch (command) {
        case "get_config": return { ...loaded };
        case "get_autostart": return false;
        case "windows_preferences": return { animations: true };
        case "wallpaper_preview": return null;
        case "default_config": return base;
        case "set_config": calls.push({ ...args.patch }); return save(args.patch);
        default: throw new Error(`Unexpected native call: ${command}`);
      }
    } },
  };
  const modules = new Map();
  function load(filename) {
    if (modules.has(filename)) return modules.get(filename).exports;
    if (filename.endsWith(".json")) return JSON.parse(readFileSync(filename, "utf8"));
    const module = { exports: {} };
    modules.set(filename, module);
    compiled.get(filename)((id) => {
      if (id === "react" && filename === path.join(source, "App.tsx")) return hooks;
      if (id in mocks) return mocks[id];
      if (!id.startsWith(".")) return require(id);
      const resolved = path.resolve(path.dirname(filename), id);
      return load(compiled.has(`${resolved}.tsx`) ? `${resolved}.tsx` : compiled.has(`${resolved}.ts`) ? `${resolved}.ts` : resolved);
    }, module, module.exports);
    return module.exports;
  }
  const { App } = load(path.join(source, "App.tsx"));
  function render() { stateIndex = refIndex = 0; effects.length = 0; return App(); }
  function find(predicate) { const node = [...elements(render())].find(predicate); assert.ok(node); return node; }
  return {
    calls, timers, render,
    external(patch) { Object.assign(loaded, patch); },
    async focus() { listeners.get("focus")?.(); await settle(); },
    async mount() { render(); const cleanup = effects.map((effect) => effect()); await settle(); return () => cleanup.forEach((fn) => fn?.()); },
    change(label, value) { find((node) => node.props.label === label && typeof node.props.onChange === "function").props.onChange(value); },
    button(text) { find((node) => node.type === "button" && node.props.children === text).props.onClick(); },
    body() { return find((node) => node.props.className === "content-body"); },
    fire(delay) { for (const [id, timer] of [...timers]) if (timer.delay === delay) { timers.delete(id); timer.callback(); } },
    async close() { let prevented = false; await close({ preventDefault() { prevented = true; } }); return prevented; },
  };
}

test("each section renders only its own controls, in every language and visibility state", () => {
  const markers = { dock: "size", windows: "genie", menubar: "menuBarShow", system: "autostart" };
  for (const language of Object.keys(texts)) for (const menuBar of [true, false]) {
    for (const section of Object.keys(markers)) {
      const view = fixture(section, { language, menuBar });
      const body = view.body();
      const html = renderToStaticMarkup(body);
      assert.doesNotMatch(html, /section\s*===|\)\s*:\s*section/);
      for (const [other, key] of Object.entries(markers)) {
        const escaped = renderToStaticMarkup(React.createElement("span", null, texts[language][key])).slice(6, -7);
        assert.equal(html.includes(escaped), other === section, `${language}/${section}: ${other}`);
      }
      assert.equal(body.key, section, "navigation resets the scroll container");
    }
  }
});

test("Bluetooth has no duplicate setting even with an old saved visibility override", () => {
  for (const language of Object.keys(texts)) for (const menuBarBluetooth of [false, true]) {
    const view = fixture("menubar", { language, menuBar: true, menuBarBluetooth });
    assert.doesNotMatch(renderToStaticMarkup(view.body()), /Bluetooth/i);
  }
});

test("retired Windows status-item overrides expose no controls and create no save", async () => {
  for (const language of Object.keys(texts)) for (const old of [null, false, true]) {
    const view = fixture("menubar", { language, menuBarSearch: old, menuBarTaskView: old,
      menuBarWidgets: old, menuBarBatteryPercent: old });
    const unmount = await view.mount();
    try {
      const controls = [...elements(view.body())].filter((node) => "checked" in node.props);
      assert.deepEqual(controls.map((node) => node.props.label), [texts[language].menuBarShow,
        texts[language].menuBarAutoHide, texts[language].menuBarMusic, texts[language].aiUsage,
        texts[language].accessoryBattery]);
      view.fire(60);
      await settle();
      assert.deepEqual(view.calls, []);
      assert.equal(await view.close(), false);
    } finally { unmount(); }
  }
});

test("other workspaces are opt-in and save without changing other window preferences", async () => {
  const view = fixture("windows", { showOtherWorkspaces: undefined });
  const checked = () => [...elements(view.body())].find((node) =>
    node.props.label === texts.en.showOtherWorkspaces && "checked" in node.props).props.checked;
  const unmount = await view.mount();
  try {
    assert.equal(checked(), false);
    view.change(texts.en.showOtherWorkspaces, true);
    view.fire(60);
    await settle();
    assert.equal(checked(), true);
    assert.deepEqual(view.calls, [{ showOtherWorkspaces: true }]);
    view.change(texts.en.showOtherWorkspaces, false);
    view.fire(60);
    await settle();
    assert.equal(checked(), false);
    assert.deepEqual(view.calls.at(-1), { showOtherWorkspaces: false });
  } finally { unmount(); }
});

test("automatic hiding pauses intelligent hiding without discarding its saved choice", async () => {
  for (const language of Object.keys(texts)) for (const smartHide of [false, true]) {
    const view = fixture("dock", { language, autoHide: true, smartHide });
    const unmount = await view.mount();
    const smartToggle = () => [...elements(view.body())].find((node) =>
      node.props.label === texts[language].smartHide && "checked" in node.props);
    try {
      let toggle = smartToggle();
      assert.equal(toggle.props.disabled, true);
      assert.equal(toggle.props.checked, smartHide);
      const button = toggle.type(toggle.props);
      assert.equal(button.props.disabled, true);
      assert.equal(button.props["aria-busy"], undefined);
      button.props.onClick(); // Disabled is not a loading state or a writable control.
      view.fire(60);
      await settle();
      assert.deepEqual(view.calls, []);
      const paused = renderToStaticMarkup(React.createElement("span", null, texts[language].smartHidePausedHint)).slice(6, -7);
      assert.ok(renderToStaticMarkup(view.body()).includes(paused));
      view.change(texts[language].autoHide, false);
      view.fire(60);
      await settle();
      toggle = smartToggle();
      assert.equal(toggle.props.disabled, false);
      assert.equal(toggle.props.checked, smartHide);
      assert.deepEqual(view.calls, [{ autoHide: false }]);
    } finally { unmount(); }
  }
});

test("failed writes remain retryable and prevent closing until saved", async () => {
  let fail = true;
  const view = fixture("menubar", {}, async () => { if (fail) throw new Error("disk busy"); });
  const unmount = await view.mount();
  try {
    view.change(texts.en.menuBarAutoHide, false);
    view.fire(60);
    await settle();
    assert.equal(await view.close(), true);
    assert.equal(view.calls.length, 2);
    assert.ok(renderToStaticMarkup(view.body()).includes(texts.en.saveError));
    fail = false;
    assert.equal(await view.close(), false);
    assert.deepEqual(view.calls, Array(3).fill({ menuBarAutoHide: false }));
    assert.ok(!renderToStaticMarkup(view.body()).includes(texts.en.saveError));
  } finally { unmount(); }
});

test("a queued newer choice wins over an earlier failed write", async () => {
  let rejectFirst;
  const view = fixture("menubar", {}, () => view.calls.length === 1
    ? new Promise((_, reject) => { rejectFirst = reject; }) : Promise.resolve());
  const unmount = await view.mount();
  try {
    view.change(texts.en.menuBarAutoHide, false);
    view.fire(60);
    await settle();
    view.change(texts.en.menuBarAutoHide, true);
    view.change(texts.en.menuBarMusic, false);
    view.fire(60);
    rejectFirst(new Error("disk busy"));
    await settle();
    assert.deepEqual(view.calls, [{ menuBarAutoHide: false }, { menuBarAutoHide: true, menuBarMusic: false }]);
    assert.equal(await view.close(), false);
    assert.equal(view.calls.length, 2, "the obsolete value must not be replayed on close");
  } finally { unmount(); }
});

test("confirming reset cancels its old confirmation timer", async () => {
  const view = fixture("system");
  const unmount = await view.mount();
  try {
    view.button(texts.en.resetButton);
    assert.equal([...view.timers.values()].filter((timer) => timer.delay === 4000).length, 1);
    view.button(texts.en.resetConfirm);
    assert.equal([...view.timers.values()].filter((timer) => timer.delay === 4000).length, 0);
    await settle();
    view.button(texts.en.resetButton);
    assert.equal([...view.timers.values()].filter((timer) => timer.delay === 4000).length, 1);
  } finally { unmount(); }
});

test("dock choices save only the selected preferences and preserve icon size", async () => {
  const view = fixture("dock", { autoHide: true, iconSize: 64 });
  const unmount = await view.mount();
  try {
    view.change(texts.en.showStart, false);
    view.change(texts.en.pinExplorer, false);
    view.change(texts.en.showTrash, false);
    view.change(texts.en.showDownloads, false);
    view.change(texts.en.dockDensity, "compact");
    view.change(texts.en.dockAutoHideSpeed, "fast");
    view.fire(60);
    await settle();
    assert.deepEqual(view.calls, [{ showStart: false, pinExplorer: false, showTrash: false, showDownloads: false,
      dockDensity: "compact", dockAutoHideSpeed: "fast" }]);
    const controls = [...elements(view.body())];
    for (const label of [texts.en.showStart, texts.en.pinExplorer, texts.en.showTrash, texts.en.showDownloads]) {
      assert.equal(controls.find((node) => node.props.label === label && "checked" in node.props).props.checked, false);
    }
    assert.equal(controls.find((node) => node.props.label === texts.en.size && "value" in node.props).props.value, 64);
    const html = renderToStaticMarkup(view.body());
    assert.ok(html.includes('checked="" value="compact"'));
    assert.ok(html.includes('checked="" value="fast"'));
  } finally { unmount(); }
});

test("returning to Settings reflects dock removals without overwriting local edits", async () => {
  let finishSave;
  const view = fixture("dock", {}, () => new Promise((resolve) => { finishSave = resolve; }));
  const unmount = await view.mount();
  const checked = (label) => [...elements(view.body())]
    .find((node) => node.props.label === label && "checked" in node.props).props.checked;
  try {
    view.external({ showDownloads: false });
    assert.equal(checked(texts.en.showDownloads), true);
    await view.focus();
    assert.equal(checked(texts.en.showDownloads), false);
    view.change(texts.en.showDownloads, true);
    await view.focus(); // Flushes the unsaved change before requesting another snapshot.
    view.change(texts.en.showTrash, false);
    finishSave();
    await settle();
    assert.equal(checked(texts.en.showDownloads), true);
    assert.equal(checked(texts.en.showTrash), false);
    assert.deepEqual(view.calls, [{ showDownloads: true }]);
  } finally { unmount(); }
});
