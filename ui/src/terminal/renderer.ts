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

  private fallBack(reason: unknown): void {
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
