import "./styles.css";

import { TerminalView } from "./terminal/view";
import { EchoPtyTransport } from "./transport/echo";
import type { PtyTransport } from "./transport/types";

/**
 * Picks the pty transport. Only the dev echo transport exists for now;
 * T8 adds TauriPtyTransport (selected when running inside the Tauri window).
 */
function createTransport(): PtyTransport {
  return new EchoPtyTransport({
    banner: "termbay dev echo transport: no shell here, input is echoed back.\r\n\r\n",
  });
}

async function main(): Promise<void> {
  const container = document.getElementById("terminals");
  if (!container) throw new Error("#terminals not found");

  const view = new TerminalView(container, {}, (kind, reason) => {
    if (kind === "dom" && reason !== undefined) console.warn("[termbay] WebGL unavailable, using DOM renderer:", reason);
  });
  const session = await createTransport().open({ cols: view.term.cols, rows: view.term.rows });
  view.attach(session);
  view.focus();
}

main().catch((error: unknown) => console.error("[termbay] failed to start", error));
