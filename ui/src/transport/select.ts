import { isTauri } from "@tauri-apps/api/core";

import { EchoPtyTransport } from "./echo";
import { TauriPtyTransport, tauriIpc, type TauriIpc } from "./tauri";
import type { PtyTransport } from "./types";

export const ECHO_BANNER =
  "termbay dev echo transport: no shell here, input is echoed back. Tab shows sample completions.\r\n\r\n";

export interface TransportEnvironment {
  /** True inside the Tauri window. */
  isTauri(): boolean;
  ipc?: TauriIpc;
}

/**
 * Inside the Tauri window, talk to the real pty in the core. In a plain
 * browser (`npm run dev`) there is no backend yet, so use the echo transport.
 */
export function createTransport(env: TransportEnvironment = { isTauri }): PtyTransport {
  if (env.isTauri()) return new TauriPtyTransport(env.ipc ?? tauriIpc);
  return new EchoPtyTransport({ banner: ECHO_BANNER, simulateShell: true });
}
