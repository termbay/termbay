import type { Trigger } from "../autocomplete/controller";
import { toError, type TauriIpc } from "./tauri";

/** What the UI reads from the config file (`get_config`). T14 adds fields. */
export interface AppConfig {
  readonly autocomplete: {
    readonly enabled: boolean;
    readonly trigger: Trigger;
    readonly history: boolean;
  };
}

/** Defaults, also used outside Tauri (echo transport) and when the answer is malformed. */
export const DEFAULT_CONFIG: AppConfig = {
  autocomplete: { enabled: false, trigger: "tab", history: true },
};

/**
 * Reads the config through `get_config`. Without `ipc` (plain browser) it
 * returns the defaults. Unknown or wrongly typed fields fall back to defaults;
 * a failing command is reported as an Error.
 */
export async function loadConfig(ipc?: TauriIpc): Promise<AppConfig> {
  if (!ipc) return DEFAULT_CONFIG;
  let raw: unknown;
  try {
    raw = await ipc.invoke<unknown>("get_config");
  } catch (error) {
    throw toError(error);
  }
  const ac = (raw as { autocomplete?: Record<string, unknown> } | null)?.autocomplete ?? {};
  const d = DEFAULT_CONFIG.autocomplete;
  return {
    autocomplete: {
      enabled: typeof ac.enabled === "boolean" ? ac.enabled : d.enabled,
      trigger: ac.trigger === "tab" || ac.trigger === "ctrl+space" ? ac.trigger : d.trigger,
      history: typeof ac.history === "boolean" ? ac.history : d.history,
    },
  };
}
