// Debug list of office events (task B1 in docs/FASE2-PLANO.md): one line per
// event in the office panel, until the scene exists (task C2). Only in builds
// with VITE_OFFICE_DEBUG=1 (the e2e build); release builds leave it out.

import type { ReducedEvent } from "../transport/office";

/** Lines kept in the list. */
export const DEBUG_LINES = 200;

/** Found by e2e/check-release.mjs: must never be in a release build. */
export const OFFICE_DEBUG_MARKER = "termbay-e2e-office-debug";

/** One readable line: event, who, and what (all fields already reduced). */
export function formatEvent(event: ReducedEvent): string {
  const who = event.teammate_name ?? event.agent_type ?? (event.agent_id ? "agent" : "main");
  const what = [
    event.tool,
    event.file ?? event.program ?? event.pattern,
    event.description,
    event.to ? `→ ${event.to}` : undefined,
    event.summary,
    event.name,
    event.task_subject,
    event.status,
    event.last_message,
  ].filter((part): part is string => typeof part === "string" && part.length > 0);
  return [event.event, who, ...what].join(" · ");
}

/** The list inside the office panel. */
export class OfficeDebugList {
  private readonly list: HTMLOListElement;

  constructor(
    private readonly panel: HTMLElement,
    document: Document = panel.ownerDocument,
  ) {
    this.list = document.createElement("ol");
    this.list.className = "office-debug";
    this.list.dataset.marker = OFFICE_DEBUG_MARKER;
    panel.replaceChildren(this.list);
  }

  /** Adds a line (text only, never HTML) and shows the panel. */
  add(event: ReducedEvent): void {
    const item = this.list.ownerDocument.createElement("li");
    item.textContent = formatEvent(event);
    this.list.append(item);
    while (this.list.children.length > DEBUG_LINES) this.list.firstElementChild?.remove();
    item.scrollIntoView?.({ block: "nearest" });
    this.panel.hidden = false;
  }

  toggle(): void {
    this.panel.hidden = !this.panel.hidden;
  }
}
