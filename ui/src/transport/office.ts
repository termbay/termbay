// Office events from the core (task B1 in docs/FASE2-PLANO.md): already
// reduced by termbay-hook (section 3.2), never the raw hook payload.

import { Channel, invoke } from "@tauri-apps/api/core";

/** One reduced hook event, as `core::events::ReducedEvent` serializes it. */
export interface ReducedEvent {
  readonly v: number;
  readonly event: string;
  readonly session: string;
  readonly pane?: string;
  readonly agent_id?: string;
  readonly agent_type?: string;
  readonly tool_use_id?: string;
  readonly task_id?: string;
  readonly tool?: string;
  readonly file?: string;
  readonly description?: string;
  readonly program?: string;
  readonly pattern?: string;
  readonly interrupt?: boolean;
  readonly name?: string;
  readonly subagent_type?: string;
  readonly background?: boolean;
  readonly spawned_agent_id?: string;
  readonly status?: string;
  readonly color?: string;
  readonly to?: string;
  readonly summary?: string;
  readonly message_type?: string;
  readonly success?: boolean;
  readonly resumed_agent_id?: string;
  readonly sender?: string;
  readonly sender_color?: string;
  readonly target?: string;
  readonly target_color?: string;
  readonly teammate_name?: string;
  readonly last_message?: string;
  readonly background_tasks?: number;
  readonly task_subject?: string;
  readonly source?: string;
  readonly reason?: string;
  readonly notification_type?: string;
  readonly task_notification?: boolean;
}

/** The slice of the Tauri API used here; injected by tests. */
export interface OfficeIpc {
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
  channel(onMessage: (event: ReducedEvent) => void): unknown;
}

export const tauriOfficeIpc: OfficeIpc = {
  invoke: (cmd, args) => invoke(cmd, args),
  channel: (onMessage) => new Channel<ReducedEvent>(onMessage),
};

/**
 * Subscribes to office events: `onEvent` gets the recent ones first, oldest
 * first, then each new one as it arrives.
 */
export async function subscribeOffice(ipc: OfficeIpc, onEvent: (event: ReducedEvent) => void): Promise<void> {
  // A new event can come through the channel before the recent ones are
  // returned; hold it until they are delivered, so the order is kept.
  let early: ReducedEvent[] | null = [];
  const channel = ipc.channel((event) => (early ? early.push(event) : onEvent(event)));
  const recent = await ipc.invoke<ReducedEvent[]>("office_subscribe", { events: channel });
  for (const event of recent) onEvent(event);
  const held = early;
  early = null;
  for (const event of held) onEvent(event);
}
