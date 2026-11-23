// The office from the core (tasks B1 and D3 in docs/FASE2-PLANO.md): a
// picture of who is there (decision E5), then each change in the office's
// own format (decision E4). Built by core::state from reduced hook events.

import { Channel, invoke } from "@tauri-apps/api/core";

/** What happened, in the office's terms (`core::state::Kind`). */
export type OfficeKind = "start" | "tool" | "message" | "wait" | "end" | "error" | "task";

/** One change in the office (`core::state::OfficeEvent`). */
export interface OfficeEvent {
  readonly v: number;
  readonly session: string;
  readonly pane?: string;
  /** "main" or the subagent's id. */
  readonly agent: string;
  readonly kind: OfficeKind;
  readonly name?: string;
  readonly role?: string;
  readonly color?: string;
  readonly action?: string;
  readonly target?: string;
  /** Agent id of the recipient when known, otherwise the name as written. */
  readonly to?: string;
  readonly text?: string;
  readonly result?: string;
  readonly task?: string;
}

export type Activity = "working" | "waiting" | "idle";

export interface AgentView {
  readonly id: string;
  readonly name?: string;
  readonly role?: string;
  readonly color?: string;
  readonly activity: Activity;
  readonly bubble?: string;
}

export interface TaskView {
  readonly id: string;
  readonly subject?: string;
  readonly done: boolean;
}

export interface SessionView {
  readonly session: string;
  readonly pane?: string;
  /** The main agent first, then the others in order of arrival. */
  readonly agents: readonly AgentView[];
  readonly tasks: readonly TaskView[];
}

/** The whole office (`core::state::OfficeSnapshot`). */
export interface OfficeSnapshot {
  readonly v: number;
  readonly sessions: readonly SessionView[];
}

/** The slice of the Tauri API used here; injected by tests. */
export interface OfficeIpc {
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
  channel(onMessage: (event: OfficeEvent) => void): unknown;
}

export const tauriOfficeIpc: OfficeIpc = {
  invoke: (cmd, args) => invoke(cmd, args),
  channel: (onMessage) => new Channel<OfficeEvent>(onMessage),
};

export interface OfficeListener {
  /** The picture of the office, once, before any change. */
  snapshot(snapshot: OfficeSnapshot): void;
  /** Each change, in order. */
  event(event: OfficeEvent): void;
}

/**
 * Subscribes to the office: `listener.snapshot` gets the picture first, then
 * `listener.event` each change as it arrives. Once `signal` is aborted the
 * listener gets nothing more from this subscription.
 */
export async function subscribeOffice(ipc: OfficeIpc, listener: OfficeListener, signal?: AbortSignal): Promise<void> {
  // A change can come through the channel before the picture is returned;
  // hold it until the picture is delivered, so nothing is applied out of order.
  let early: OfficeEvent[] | null = [];
  const channel = ipc.channel((event) => {
    if (signal?.aborted) return;
    if (early) early.push(event);
    else listener.event(event);
  });
  const snapshot = await ipc.invoke<OfficeSnapshot>("office_subscribe", { events: channel });
  if (signal?.aborted) return;
  listener.snapshot(snapshot);
  const held = early;
  early = null;
  for (const event of held) listener.event(event);
}

/**
 * One office subscription at a time: each call cancels the previous one
 * before subscribing again, so a listener never gets a change twice. The
 * core keeps sending to a cancelled channel until the page goes away; its
 * messages are dropped here.
 */
export function singleSubscription(ipc: OfficeIpc): (listener: OfficeListener) => Promise<void> {
  let current: AbortController | null = null;
  return (listener) => {
    current?.abort();
    const controller = new AbortController();
    current = controller;
    return subscribeOffice(ipc, listener, controller.signal);
  };
}
