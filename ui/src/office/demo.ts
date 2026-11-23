// Demo mode of the office: a recorded sequence in the office's own format
// (decision E4), played on a timer, to work on the scene without running
// claude. A session where the main agent calls two subagents in parallel,
// they work, ask for permission, talk, and finish; then the session ends.
// Invented names and files: nothing from a real session.

import type { OfficeEvent } from "../transport/office";

export interface DemoStep {
  /** Wait before this event, in ms. */
  readonly after: number;
  readonly event: OfficeEvent;
}

const S = "demo-session";
const ev = (agent: string, fields: Omit<OfficeEvent, "v" | "session" | "agent">): OfficeEvent => ({
  v: 1,
  session: S,
  pane: "demo-pane",
  agent,
  ...fields,
});

export const DEMO: readonly DemoStep[] = [
  { after: 0, event: ev("main", { kind: "start" }) },
  { after: 600, event: ev("main", { kind: "tool", action: "task" }) },
  { after: 1200, event: ev("main", { kind: "tool", action: "reading", target: "README.md" }) },
  { after: 1500, event: ev("main", { kind: "task", action: "created", task: "1", text: "login screen" }) },
  { after: 900, event: ev("main", { kind: "message", to: "ana", text: "build the login API" }) },
  { after: 300, event: ev("a-ana", { kind: "start", name: "ana", role: "backend" }) },
  { after: 700, event: ev("main", { kind: "message", to: "bruno", text: "write the login tests" }) },
  { after: 300, event: ev("a-bruno", { kind: "start", name: "bruno", role: "qa" }) },
  { after: 1500, event: ev("a-ana", { kind: "tool", action: "reading", target: "api.ts" }) },
  { after: 800, event: ev("a-bruno", { kind: "tool", action: "searching", target: "login" }) },
  { after: 1600, event: ev("a-ana", { kind: "tool", action: "writing", target: "auth.ts" }) },
  { after: 900, event: ev("a-bruno", { kind: "tool", action: "writing", target: "login.test.ts" }) },
  { after: 1400, event: ev("a-bruno", { kind: "wait", action: "permission", target: "npm" }) },
  { after: 2500, event: ev("a-bruno", { kind: "tool", action: "running", target: "Run the tests" }) },
  { after: 1200, event: ev("a-bruno", { kind: "error", action: "Bash", result: "failed" }) },
  { after: 1000, event: ev("a-bruno", { kind: "message", to: "a-ana", text: "token check fails" }) },
  { after: 1200, event: ev("a-ana", { kind: "tool", action: "writing", target: "auth.ts" }) },
  { after: 1500, event: ev("a-ana", { kind: "end", result: "login API ready" }) },
  { after: 900, event: ev("a-bruno", { kind: "tool", action: "running", target: "Run the tests" }) },
  { after: 1800, event: ev("a-bruno", { kind: "end", result: "12 tests pass" }) },
  { after: 1200, event: ev("main", { kind: "task", action: "completed", task: "1", text: "login screen" }) },
  { after: 1200, event: ev("main", { kind: "end" }) },
  { after: 2500, event: ev("main", { kind: "wait", action: "user" }) },
  { after: 4000, event: ev("a-bruno", { kind: "end", result: "other" }) },
  { after: 0, event: ev("a-ana", { kind: "end", result: "other" }) },
  { after: 0, event: ev("main", { kind: "end", result: "other" }) },
  { after: 0, event: ev("main", { kind: "session_end", result: "other" }) },
];

export interface DemoTimers {
  set(callback: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

const browserTimers: DemoTimers = {
  set: (callback, ms) => setTimeout(callback, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

/**
 * Plays `steps` into `onEvent`, `speed` times faster, looping when `loop`
 * is set. Returns a function that stops it.
 */
export function playDemo(
  onEvent: (event: OfficeEvent) => void,
  { steps = DEMO, speed = 1, loop = false, timers = browserTimers } = {} as {
    steps?: readonly DemoStep[];
    speed?: number;
    loop?: boolean;
    timers?: DemoTimers;
  },
): () => void {
  let i = 0;
  let handle: unknown = null;
  let stopped = false;
  const next = () => {
    if (stopped) return;
    if (i >= steps.length) {
      if (!loop || steps.length === 0) return;
      i = 0;
    }
    const step = steps[i] as DemoStep;
    handle = timers.set(() => {
      i++;
      onEvent(step.event);
      next();
    }, step.after / speed);
  };
  next();
  return () => {
    stopped = true;
    if (handle !== null) timers.clear(handle);
  };
}
