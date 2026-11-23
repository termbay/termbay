import "./styles.css";

import type { ITerminalOptions } from "@xterm/xterm";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { confirmInPage } from "./layout/confirm";
import type { OfficeContentLoader } from "./office/content";
import { OfficePanel, localWidthStore } from "./office/panel";
import { PanelModel } from "./office/panel-model";
import { LayoutController } from "./layout/controller";
import { installShortcuts } from "./layout/shortcuts";
import { LayoutView } from "./layout/view";
import { detectOs } from "./platform/os";
import { TerminalView } from "./terminal/view";
import { DEFAULT_CONFIG, loadConfig } from "./transport/config";
import { singleSubscription, tauriOfficeIpc } from "./transport/office";
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

  const office = startOffice(inTauri, config.office.autoOpen, () => {
    const tab = app.layout.tabs.find((t) => t.id === app.layout.active);
    if (tab) app.paneView(tab.focused)?.focus();
  });
  installShortcuts(window, os, (action) => {
    if (action.type === "toggleOffice") office?.toggle();
    else app.run(action);
  });
}

/**
 * The office panel (task B2, decision E9). The subscription starts now, to
 * notice the first agent; the content is loaded with import() only on the
 * first opening. Outside Tauri there is no office to listen to yet.
 */
function startOffice(inTauri: boolean, autoOpen: boolean, focusTerminal: () => void): OfficePanel | null {
  const root = document.getElementById("app");
  const panel = document.getElementById("office-panel");
  if (!root || !panel) return null;
  const store = localWidthStore(() => window.localStorage);
  // Resubscribing (after the panel missed too many changes) cancels the first subscription.
  const subscribe = singleSubscription(tauriOfficeIpc);
  const office = new OfficePanel({
    root,
    panel,
    model: new PanelModel({ autoOpen, width: store.load() }),
    load: officeContent,
    store,
    resubscribe: subscribe,
    focusTerminal,
  });
  if (inTauri) {
    subscribe(office).catch((error: unknown) => console.error("[termbay] could not subscribe to office events", error));
  }
  return office;
}

/**
 * What the panel shows: the office scene (task C2), loaded with import()
 * on the first opening, with PixiJS behind a second import() (decision E8).
 * Builds with VITE_OFFICE_DEBUG=1 (the e2e build) show the scene and the
 * debug list of tasks B1 and D3 together, with a probe for the e2e specs;
 * release builds never contain them (e2e/check-release.mjs makes sure).
 */
const officeContent: OfficeContentLoader =
  import.meta.env.VITE_OFFICE_DEBUG === "1"
    ? async (host) => (await import("./office/e2e-content")).createE2EContent(host)
    : async (host) => (await import("./office/scene")).loadOfficeContent(host);

main().catch((error: unknown) => console.error("[termbay] failed to start", error));
