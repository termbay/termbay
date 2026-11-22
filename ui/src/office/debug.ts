// Debug list of the office (task B1 in docs/FASE2-PLANO.md): the picture of
// the office, then one line per change, in the office panel, until the scene
// exists (task C2). Only in builds with VITE_OFFICE_DEBUG=1 (the e2e build);
// release builds leave it out.

import type { AgentView, OfficeEvent, OfficeSnapshot } from "../transport/office";

/** Lines kept in the list. */
export const DEBUG_LINES = 200;

/** Found by e2e/check-release.mjs: must never be in a release build. */
export const OFFICE_DEBUG_MARKER = "termbay-e2e-office-debug";

/** Agent names seen so far, by session and id, to show names on later lines. */
type Names = Map<string, string>;

const key = (session: string, agent: string) => `${session}\u0000${agent}`;

/** One readable line for a change: kind, who, and what. */
export function formatEvent(event: OfficeEvent, names: Names = new Map()): string {
  const who = event.name ?? names.get(key(event.session, event.agent)) ?? event.agent;
  const to = event.to ? `→ ${names.get(key(event.session, event.to)) ?? event.to}` : undefined;
  const what = [event.action, event.target, to, event.text, event.result, event.role, event.color].filter(
    (part): part is string => typeof part === "string" && part.length > 0,
  );
  return [event.kind, who, ...what].join(" · ");
}

/** One readable line for an agent in the picture. */
export function formatAgent(agent: AgentView): string {
  return ["here", agent.name ?? agent.id, agent.activity, agent.bubble].filter(Boolean).join(" · ");
}

/** The list inside the office panel. */
export class OfficeDebugList {
  private readonly list: HTMLOListElement;
  private readonly names: Names = new Map();

  constructor(
    private readonly panel: HTMLElement,
    document: Document = panel.ownerDocument,
  ) {
    this.list = document.createElement("ol");
    this.list.className = "office-debug";
    this.list.dataset.marker = OFFICE_DEBUG_MARKER;
    panel.replaceChildren(this.list);
  }

  /** The picture: one line per agent. Shows the panel if anyone is there. */
  snapshot(snapshot: OfficeSnapshot): void {
    for (const session of snapshot.sessions) {
      for (const agent of session.agents) {
        if (agent.name) this.names.set(key(session.session, agent.id), agent.name);
        this.line(formatAgent(agent));
      }
    }
  }

  /** A change: one line (text only, never HTML), and the panel shows. */
  event(event: OfficeEvent): void {
    if (event.kind === "start" && event.name) this.names.set(key(event.session, event.agent), event.name);
    this.line(formatEvent(event, this.names));
  }

  toggle(): void {
    this.panel.hidden = !this.panel.hidden;
  }

  private line(text: string): void {
    const item = this.list.ownerDocument.createElement("li");
    item.textContent = text;
    this.list.append(item);
    while (this.list.children.length > DEBUG_LINES) this.list.firstElementChild?.remove();
    item.scrollIntoView?.({ block: "nearest" });
    this.panel.hidden = false;
  }
}
