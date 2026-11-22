import "./styles.css";

import type { ITerminalOptions } from "@xterm/xterm";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { confirmInPage } from "./layout/confirm";
import { LayoutController } from "./layout/controller";
import { installShortcuts } from "./layout/shortcuts";
import { LayoutView } from "./layout/view";
import { detectOs } from "./platform/os";
import { TerminalView } from "./terminal/view";
import { DEFAULT_CONFIG, loadConfig } from "./transport/config";
import { createTransport } from "./transport/select";
import { tauriIpc } from "./transport/tauri";

async function main(): Promise<void> {
  const container = document.getElementById("terminals");
  if (!container) throw new Error("#terminals not found");

  const os = detectOs();
  const inTauri = isTauri();
  const options: ITerminalOptions = {};
  // The core uses ConPTY on Windows; xterm.js adjusts reflow for it. The
  // Windows build number is unknown to the webview, so only the backend is set.
  if (inTauri && os === "windows") options.windowsPty = { backend: "conpty" };

  // One quick IPC round trip, so every pane, the first included, uses the configured trigger.
  const config = await loadConfig(inTauri ? tauriIpc : undefined).catch((error: unknown) => {
    console.warn("[termbay] could not read the config, using defaults", error);
    return DEFAULT_CONFIG;
  });

  let controller: LayoutController | null = null;
  const view = new LayoutView(container, {
    newTab: () => controller?.run({ type: "newTab" }),
    activateTab: (id) => controller?.activateTab(id),
    closeTab: (id) => void controller?.requestCloseTab(id),
    focusPane: (id) => controller?.focusPane(id),
    setRatio: (id, ratio) => controller?.setRatio(id, ratio),
  });

  const app = new LayoutController({
    transport: createTransport({ isTauri: () => inTauri }),
    createElement: () => document.createElement("div"),
    createPane: (element) =>
      new TerminalView(
        element,
        options,
        (kind, reason) => {
          if (kind === "dom" && reason !== undefined) console.warn("[termbay] WebGL unavailable, using DOM renderer:", reason);
        },
        os,
      ),
    trigger: config.autocomplete.trigger,
    renderer: view,
    confirm: (message) => confirmInPage(document.body, message),
    closeWindow: () => {
      // D19: the app has a single window; closing it ends the app.
      if (inTauri) getCurrentWindow().close().catch((error: unknown) => console.error("[termbay] could not close the window", error));
      else window.close();
    },
  });
  controller = app;

  const officeDebug = inTauri && import.meta.env.VITE_OFFICE_DEBUG === "1" ? await startOfficeDebug() : null;
  installShortcuts(window, os, (action) => {
    // Until the office panel exists (task B2), the shortcut shows the debug list.
    if (action.type === "toggleOffice" && officeDebug) officeDebug.toggle();
    else app.run(action);
  });
}

/**
 * Debug list of office events (task B1), only in builds with
 * VITE_OFFICE_DEBUG=1. Loaded with import() behind that check, so release
 * builds do not contain it (e2e/check-release.mjs makes sure).
 */
async function startOfficeDebug(): Promise<{ toggle(): void } | null> {
  const panel = document.getElementById("office-panel");
  if (!panel) return null;
  const [{ OfficeDebugList }, { subscribeOffice, tauriOfficeIpc }] = await Promise.all([
    import("./office/debug"),
    import("./transport/office"),
  ]);
  const list = new OfficeDebugList(panel);
  subscribeOffice(tauriOfficeIpc, (event) => list.add(event)).catch((error: unknown) =>
    console.error("[termbay] could not subscribe to office events", error),
  );
  return list;
}

main().catch((error: unknown) => console.error("[termbay] failed to start", error));
