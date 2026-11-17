import type { Disposable } from "../transport/types";

export type RendererKind = "webgl" | "dom";

/** The subset of `WebglAddon` we rely on. */
export interface WebglAddonLike extends Disposable {
  onContextLoss(listener: () => void): Disposable;
}

export interface RendererHost<A extends WebglAddonLike> {
  loadAddon(addon: A): void;
}

/**
 * Tries the WebGL renderer and falls back to xterm.js's built-in DOM renderer
 * when WebGL is unavailable or its context is lost (some WebKitGTK setups).
 */
export class RendererController<A extends WebglAddonLike = WebglAddonLike> implements Disposable {
  private addon: A | null = null;
  private subscription: Disposable | null = null;
  private current: RendererKind = "dom";
  /** WebGL failed once (unsupported or context lost): stay on the DOM renderer. */
  private unavailable = false;

  constructor(
    private readonly host: RendererHost<A>,
    private readonly createWebgl: () => A,
    private readonly onChange: (kind: RendererKind, reason?: unknown) => void = () => {},
  ) {}

  get kind(): RendererKind {
    return this.current;
  }

  /** Activates WebGL if possible. Returns the renderer in use. */
  start(): RendererKind {
    if (this.unavailable || this.addon) return this.current;
    try {
      const addon = this.createWebgl();
      this.host.loadAddon(addon);
      this.addon = addon;
      this.subscription = addon.onContextLoss(() => this.fallBack("webgl context lost"));
      this.current = "webgl";
      this.onChange("webgl");
    } catch (error) {
      this.fallBack(error);
    }
    return this.current;
  }

  /**
   * Releases the WebGL context (hidden tab: browsers cap live contexts at
   * about 16). xterm.js goes back to the DOM renderer; `start` brings WebGL back.
   */
  stop(): void {
    if (!this.addon) return;
    this.subscription?.dispose();
    this.subscription = null;
    const addon = this.addon;
    this.addon = null;
    try {
      addon.dispose();
    } catch {
      // Already broken; nothing else to do.
    }
    this.current = "dom";
  }

  private fallBack(reason: unknown): void {
    this.unavailable = true;
    this.subscription?.dispose();
    this.subscription = null;
    const addon = this.addon;
    this.addon = null;
    // Disposing the addon makes xterm.js switch back to the DOM renderer.
    try {
      addon?.dispose();
    } catch {
      // Already broken; nothing else to do.
    }
    this.current = "dom";
    this.onChange("dom", reason);
  }

  dispose(): void {
    this.subscription?.dispose();
    this.addon?.dispose();
    this.subscription = null;
    this.addon = null;
  }
}
