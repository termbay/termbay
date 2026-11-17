import "./styles.css";

import type { ITerminalOptions } from "@xterm/xterm";
import { isTauri } from "@tauri-apps/api/core";

import { detectOs } from "./platform/os";
import { TerminalView } from "./terminal/view";
import { createTransport } from "./transport/select";

async function main(): Promise<void> {
  const container = document.getElementById("terminals");
  if (!container) throw new Error("#terminals not found");

  const os = detectOs();
  const inTauri = isTauri();
  const options: ITerminalOptions = {};
  // The core uses ConPTY on Windows; xterm.js adjusts reflow for it. The
  // Windows build number is unknown to the webview, so only the backend is set.
  if (inTauri && os === "windows") options.windowsPty = { backend: "conpty" };

  const view = new TerminalView(
    container,
    options,
    (kind, reason) => {
      if (kind === "dom" && reason !== undefined) console.warn("[termbay] WebGL unavailable, using DOM renderer:", reason);
    },
    os,
  );
  view.focus();

  try {
    const session = await createTransport({ isTauri: () => inTauri }).open({ cols: view.term.cols, rows: view.term.rows });
    view.attach(session);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    view.term.write(`[termbay] could not start the shell: ${message}\r\n`);
    throw error;
  }
}

main().catch((error: unknown) => console.error("[termbay] failed to start", error));
