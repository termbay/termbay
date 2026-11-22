// State of the office panel (task B2 in docs/FASE2-PLANO.md, decision E9):
// open or closed, whether it already opened by itself, and its width. Pure,
// so it is tested without a DOM; ui/src/office/panel.ts draws it.

import type { OfficeSnapshot } from "../transport/office";

/** Narrowest panel, in CSS pixels. */
export const MIN_PANEL_WIDTH = 240;
/** Widest panel, in CSS pixels, on large screens. */
export const MAX_PANEL_WIDTH = 1200;
/** Width left to the terminals, so they stay usable next to the panel. */
export const MIN_TERMINAL_WIDTH = 320;
/** Width before the user drags the divider. */
export const DEFAULT_PANEL_WIDTH = 360;

/**
 * Fits `width` to the window: at least MIN_PANEL_WIDTH, and at most what
 * leaves MIN_TERMINAL_WIDTH to the terminals (never above MAX_PANEL_WIDTH).
 * In a window too narrow for both, the terminals win and the panel gets
 * what is left (possibly less than the minimum, never below zero).
 */
export function clampWidth(width: number, viewport: number): number {
  const wanted = Number.isFinite(width) ? width : DEFAULT_PANEL_WIDTH;
  const max = Math.min(MAX_PANEL_WIDTH, Math.max(0, viewport - MIN_TERMINAL_WIDTH));
  return Math.round(Math.min(Math.max(wanted, MIN_PANEL_WIDTH), max));
}

/** True when the picture of the office has at least one agent. */
export function hasAgents(snapshot: OfficeSnapshot): boolean {
  return snapshot.sessions.some((session) => session.agents.length > 0);
}

export interface PanelModelOptions {
  /** `[office] auto_open`: open by itself on the first agent event. */
  readonly autoOpen: boolean;
  /** Width remembered from an earlier run, if any. */
  readonly width?: number | undefined;
}

export class PanelModel {
  private isOpen = false;
  /** An agent was seen in this window: later agents never reopen the panel. */
  private sawAgent = false;
  private preferred: number;

  constructor(private readonly options: PanelModelOptions) {
    const width = options.width;
    this.preferred = width !== undefined && Number.isFinite(width) && width > 0 ? Math.round(width) : DEFAULT_PANEL_WIDTH;
  }

  get open(): boolean {
    return this.isOpen;
  }

  /** Width the user chose (not fitted to the window). */
  get width(): number {
    return this.preferred;
  }

  /** Width to draw in a window `viewport` pixels wide. */
  widthFor(viewport: number): number {
    return clampWidth(this.preferred, viewport);
  }

  /**
   * An agent showed up. Opens the panel the first time only, when
   * `auto_open` is on; returns true when this call opened it.
   */
  agentSeen(): boolean {
    if (this.sawAgent) return false;
    this.sawAgent = true;
    if (!this.options.autoOpen || this.isOpen) return false;
    this.isOpen = true;
    return true;
  }

  /** The shortcut: opens or closes. Returns the new state. */
  toggle(): boolean {
    this.isOpen = !this.isOpen;
    return this.isOpen;
  }

  /** The close button. */
  close(): void {
    this.isOpen = false;
  }

  /** The divider moved: keeps the width fitted to the window and returns it. */
  resize(width: number, viewport: number): number {
    this.preferred = clampWidth(width, viewport);
    return this.preferred;
  }
}
