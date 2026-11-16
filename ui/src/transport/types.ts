/**
 * Boundary between the terminal UI and a pseudo-terminal living somewhere else.
 *
 * Implementations:
 * - EchoPtyTransport (this folder): in-memory echo, for `npm run dev` and tests.
 * - TauriPtyTransport (T8): `pty_open` / `pty_write` / `pty_resize` / `pty_close`
 *   commands, one Tauri `Channel<ArrayBuffer>` per pty for raw output bytes,
 *   and the global `pty-exit` event.
 * - WebSocket transport (browser mode, `termbay serve`, phase 2 or later):
 *   same contract over the local 127.0.0.1 server with a session token.
 *
 * The terminal code only depends on these types, never on Tauri or the network.
 */

export interface Disposable {
  dispose(): void;
}

export interface PtyOpenOptions {
  /** Initial size, in character cells. */
  readonly cols: number;
  readonly rows: number;
  /** Working directory. Defaults to the user's home on the core side. */
  readonly cwd?: string;
  /** Extra environment variables, on top of what the core sets (TERM, COLORTERM...). */
  readonly env?: Readonly<Record<string, string>>;
}

export interface PtyExit {
  /** Process exit code, or null when killed by a signal / unknown. */
  readonly exitCode: number | null;
}

export interface PtySession {
  /** Opaque id assigned by the transport (the core's session id for Tauri). */
  readonly id: string;

  /** Send input bytes (keystrokes, paste) to the pty. */
  write(data: Uint8Array): Promise<void>;

  /** Tell the pty its new size in character cells. */
  resize(cols: number, rows: number): Promise<void>;

  /**
   * Flow control (D6 / T10): acknowledge `bytes` of output already rendered.
   * The core pauses reading when too many bytes are pending. Fire and forget.
   */
  ack(bytes: number): void;

  /** Close the pty and kill its process. Idempotent. */
  close(): Promise<void>;

  /**
   * Raw output bytes, in order. Output received before the first listener is
   * attached is buffered and delivered to it, so nothing is lost after `open`.
   */
  onData(listener: (chunk: Uint8Array) => void): Disposable;

  /** Fired once when the process ends (or the session is closed). */
  onExit(listener: (exit: PtyExit) => void): Disposable;
}

export interface PtyTransport {
  open(options: PtyOpenOptions): Promise<PtySession>;
}
