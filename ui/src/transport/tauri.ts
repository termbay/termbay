import { Channel, invoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { Emitter } from "./emitter";
import type {
  CompletionResult,
  Disposable,
  PtyExit,
  PtyInfo,
  PtyOpenOptions,
  PtySession,
  PtyTransport,
  ShellInfo,
} from "./types";

/** Header carrying the pty id on `pty_write`, whose body is the raw input bytes. */
export const PTY_ID_HEADER = "termbay-pty-id";
export const PTY_EXIT_EVENT = "pty-exit";
export const PTY_SHELL_EVENT = "pty-shell";

/** Payload of the global `pty-exit` event. */
export interface PtyExitPayload {
  readonly id: number;
  readonly exitCode: number | null;
}

/** Payload of the global `pty-shell` event (shell integration is active). */
export interface PtyShellPayload {
  readonly id: number;
  readonly shell: string;
  readonly protocol: number;
}

/** Output message as delivered by a Tauri `Channel` (raw bytes arrive as ArrayBuffer). */
export type ChannelMessage = ArrayBuffer | Uint8Array | number[];

/**
 * The slice of the Tauri API used by the transport. Injected so tests can run
 * in plain Node without `window.__TAURI_INTERNALS__`.
 */
export interface TauriIpc {
  invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T>;
  /**
   * Creates the object passed as `onData` to `pty_open` (a `Channel`).
   * `onEnd` runs once the channel delivered its last message, if the
   * implementation can tell; otherwise the exit falls back to a timeout.
   */
  channel(onMessage: (message: ChannelMessage) => void, onEnd: () => void): unknown;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

export const tauriIpc: TauriIpc = {
  invoke: (cmd, args, options) => invoke(cmd, args, options),
  channel: (onMessage, onEnd) => {
    const channel = new Channel<ChannelMessage>(onMessage);
    hookChannelEnd(channel, onEnd);
    return channel;
  },
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
};

/**
 * The public `Channel` API does not say when the backend ended the channel.
 * Internally (@tauri-apps/api 2.12) `cleanupCallback` runs exactly once, after
 * the `{ end: true }` marker AND every earlier message were delivered in order,
 * so we wrap it. If a future version drops that method, `onEnd` never fires and
 * the session's exit timeout takes over.
 */
export function hookChannelEnd(channel: object, onEnd: () => void): boolean {
  const target = channel as { cleanupCallback?: unknown };
  const original = target.cleanupCallback;
  if (typeof original !== "function") return false;
  target.cleanupCallback = function (this: unknown, ...args: unknown[]) {
    try {
      return (original as (...a: unknown[]) => unknown).apply(this, args);
    } finally {
      onEnd();
    }
  };
  return true;
}

/**
 * How long a `pty-exit` waits for the output channel to finish delivering
 * before being reported anyway (large chunks arrive through async fetches).
 */
export const EXIT_DRAIN_TIMEOUT_MS = 500;

/** Output of one pty: buffered bytes plus "the channel delivered everything". */
export class OutputStream {
  readonly data = new Emitter<Uint8Array>({ bufferUntilListener: true });
  private ended = false;
  private endListeners: Array<() => void> = [];

  push(message: ChannelMessage): void {
    const chunk = toBytes(message);
    if (chunk.byteLength > 0) this.data.emit(chunk);
  }

  end(): void {
    if (this.ended) return;
    this.ended = true;
    for (const listener of this.endListeners.splice(0)) listener();
  }

  /** Runs `listener` once the channel ended (at once if it already has). */
  whenEnded(listener: () => void): void {
    if (this.ended) listener();
    else this.endListeners.push(listener);
  }
}

/** Commands reject with a plain string; turn it into an Error with that message. */
export function toError(error: unknown): Error {
  if (error instanceof Error) return error;
  return new Error(typeof error === "string" ? error : JSON.stringify(error));
}

function toBytes(message: ChannelMessage): Uint8Array {
  if (message instanceof Uint8Array) return message;
  if (message instanceof ArrayBuffer) return new Uint8Array(message);
  return Uint8Array.from(message);
}

/**
 * Pty transport over the commands of the Tauri app (`app/src-tauri`):
 * `pty_open` with a binary `Channel` for output, `pty_write` with a raw body,
 * `pty_resize`, `pty_ack`, `pty_close`, `pty_complete`, `pty_info`, and the `pty-exit`
 * and `pty-shell` events.
 */
export class TauriPtyTransport implements PtyTransport {
  private readonly sessions = new Map<number, TauriPtySession>();
  /** Exits that arrived before `pty_open` resolved (a shell that dies at once). */
  private readonly earlyExits = new Map<number, PtyExit>();
  /** Handshakes that arrived before `pty_open` resolved. */
  private readonly earlyShells = new Map<number, ShellInfo>();
  /** Ids closed by us whose `pty-exit` may still arrive; it is ignored. */
  private readonly retired = new Set<number>();
  private exitSubscription: Promise<() => void> | null = null;
  private shellSubscription: Promise<() => void> | null = null;

  constructor(
    private readonly ipc: TauriIpc = tauriIpc,
    private readonly options: { exitDrainTimeoutMs?: number } = {},
  ) {}

  async open(options: PtyOpenOptions): Promise<PtySession> {
    await Promise.all([this.subscribeToExits(), this.subscribeToShells()]);

    const output = new OutputStream();
    const onData = this.ipc.channel(
      (message) => output.push(message),
      () => output.end(),
    );

    let id: number;
    try {
      id = await this.ipc.invoke<number>("pty_open", {
        cols: options.cols,
        rows: options.rows,
        onData,
        ...(options.cwd !== undefined && { cwd: options.cwd }),
      });
    } catch (error) {
      throw toError(error);
    }

    const session = new TauriPtySession(
      id,
      this.ipc,
      output,
      (fromCore) => {
        this.sessions.delete(id);
        this.earlyShells.delete(id);
        if (!fromCore) this.retired.add(id);
      },
      this.options.exitDrainTimeoutMs ?? EXIT_DRAIN_TIMEOUT_MS,
    );
    this.sessions.set(id, session);
    const earlyShell = this.earlyShells.get(id);
    if (earlyShell) {
      this.earlyShells.delete(id);
      session.handleShell(earlyShell);
    }
    const early = this.earlyExits.get(id);
    if (early) {
      this.earlyExits.delete(id);
      session.handleCoreExit(early);
    }
    return session;
  }

  private subscribeToExits(): Promise<() => void> {
    this.exitSubscription ??= this.ipc.listen<PtyExitPayload>(PTY_EXIT_EVENT, ({ id, exitCode }) => {
      const exit: PtyExit = { exitCode };
      const session = this.sessions.get(id);
      if (session) session.handleCoreExit(exit);
      else if (this.retired.has(id)) this.retired.delete(id);
      else this.earlyExits.set(id, exit);
    });
    return this.exitSubscription;
  }

  private subscribeToShells(): Promise<() => void> {
    this.shellSubscription ??= this.ipc.listen<PtyShellPayload>(PTY_SHELL_EVENT, ({ id, shell, protocol }) => {
      const info: ShellInfo = { shell, protocol };
      const session = this.sessions.get(id);
      if (session) session.handleShell(info);
      else if (!this.retired.has(id)) this.earlyShells.set(id, info);
    });
    return this.shellSubscription;
  }

  /** Stops listening for `pty-exit` and `pty-shell`. Open sessions are not closed. */
  async dispose(): Promise<void> {
    const subscriptions = [this.exitSubscription, this.shellSubscription];
    this.exitSubscription = null;
    this.shellSubscription = null;
    for (const subscription of subscriptions) if (subscription) (await subscription)();
  }
}

export class TauriPtySession implements PtySession {
  readonly id: string;
  private readonly exit = new Emitter<PtyExit>();
  private exitInfo: PtyExit | null = null;
  private readonly shell = new Emitter<ShellInfo>();
  private shellInfo: ShellInfo | null = null;
  private closing: Promise<void> | null = null;
  private pendingAck = 0;
  private ackScheduled = false;
  /** Tail of the write queue; never rejects, so one failure does not block the next write. */
  private writeTail: Promise<void> = Promise.resolve();
  private exitReported = false;
  private drainTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    readonly ptyId: number,
    private readonly ipc: TauriIpc,
    private readonly output: OutputStream,
    /** Called once when the session ends; `fromCore` is false when ended by `close`. */
    private readonly forget: (fromCore: boolean) => void,
    private readonly exitDrainTimeoutMs = EXIT_DRAIN_TIMEOUT_MS,
  ) {
    this.id = String(ptyId);
  }

  private get data(): Emitter<Uint8Array> {
    return this.output.data;
  }

  /**
   * Each `pty_write` is its own request, so two in flight could reach the core
   * out of order. Writes are queued: the next one starts after the previous
   * one settled, whether it succeeded or failed.
   */
  write(bytes: Uint8Array): Promise<void> {
    if (this.closing) return Promise.reject(new Error(`pty ${this.id} is closed`));
    const result = this.writeTail.then(async () => {
      if (this.closing) throw new Error(`pty ${this.id} is closed`);
      try {
        await this.ipc.invoke<void>("pty_write", bytes, { headers: { [PTY_ID_HEADER]: this.id } });
      } catch (error) {
        throw toError(error);
      }
    });
    this.writeTail = result.catch(() => {});
    return result;
  }

  /**
   * `pty_complete`, sent after the writes already queued have reached the core,
   * so the shell sees every key typed before the request.
   */
  complete(): Promise<CompletionResult> {
    if (this.closing || this.exited) return Promise.resolve({ status: "unavailable" });
    return this.writeTail.then(async () => {
      if (this.closing || this.exited) return { status: "unavailable" } as const;
      try {
        return await this.ipc.invoke<CompletionResult>("pty_complete", { id: this.ptyId });
      } catch (error) {
        throw toError(error);
      }
    });
  }

  /** A listener added after the handshake is called at once. */
  onShell(listener: (info: ShellInfo) => void): Disposable {
    if (this.shellInfo) listener(this.shellInfo);
    return this.shell.on(listener);
  }

  /** @internal `pty-shell` from the core. */
  handleShell(info: ShellInfo): void {
    if (this.exited) return;
    this.shellInfo = info;
    this.shell.emit(info);
  }

  async info(): Promise<PtyInfo> {
    try {
      return await this.ipc.invoke<PtyInfo>("pty_info", { id: this.ptyId });
    } catch (error) {
      throw toError(error);
    }
  }

  async resize(cols: number, rows: number): Promise<void> {
    if (this.closing || this.exited) return;
    try {
      await this.ipc.invoke<void>("pty_resize", { id: this.ptyId, cols, rows });
    } catch (error) {
      throw toError(error);
    }
  }

  /** Coalesces the acks of one render pass into a single `pty_ack`. */
  ack(bytes: number): void {
    if (this.closing || this.exited || bytes <= 0) return;
    this.pendingAck += bytes;
    if (this.ackScheduled) return;
    this.ackScheduled = true;
    queueMicrotask(() => {
      this.ackScheduled = false;
      const total = this.pendingAck;
      this.pendingAck = 0;
      if (total === 0 || this.closing || this.exited) return;
      this.ipc.invoke<void>("pty_ack", { id: this.ptyId, bytes: total }).catch(() => {
        // Fire and forget: the pty may be gone already.
      });
    });
  }

  close(): Promise<void> {
    this.closing ??= this.ipc
      .invoke<void>("pty_close", { id: this.ptyId })
      .catch((error: unknown) => {
        throw toError(error);
      })
      // If the core's pty-exit is already waiting for the channel, it will not come again.
      .finally(() => this.handleExit({ exitCode: null }, this.exitReported));
    return this.closing;
  }

  onData(listener: (chunk: Uint8Array) => void): Disposable {
    return this.data.on(listener);
  }

  /** A listener added after the exit (e.g. the shell died during `open`) is called at once. */
  onExit(listener: (exit: PtyExit) => void): Disposable {
    if (this.exitInfo) {
      listener(this.exitInfo);
      return { dispose: () => {} };
    }
    return this.exit.on(listener);
  }

  private get exited(): boolean {
    return this.exitInfo !== null;
  }

  /**
   * @internal `pty-exit` from the core. The core sends it after the channel's
   * end marker, but large chunks reach JS through async fetches and may land
   * after the event; so the exit waits until the channel delivered everything,
   * or `exitDrainTimeoutMs` at most.
   */
  handleCoreExit(exit: PtyExit): void {
    if (this.exitReported || this.exited) return;
    this.exitReported = true;
    const deliver = () => {
      clearTimeout(this.drainTimer);
      this.handleExit(exit, true);
    };
    this.drainTimer = setTimeout(deliver, this.exitDrainTimeoutMs);
    this.output.whenEnded(deliver);
  }

  /** @internal Fires `onExit` once and releases the session. */
  handleExit(exit: PtyExit, fromCore = true): void {
    clearTimeout(this.drainTimer);
    if (this.exited) return;
    this.exitInfo = exit;
    this.exit.emit(exit);
    this.exit.clear();
    this.shell.clear();
    this.data.clear();
    this.forget(fromCore);
  }
}
