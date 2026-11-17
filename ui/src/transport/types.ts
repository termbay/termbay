/**
 * Boundary between the terminal UI and a pseudo-terminal living somewhere else.
 *
 * Implementations:
 * - EchoPtyTransport (this folder): in-memory echo, for `npm run dev` and tests.
 * - TauriPtyTransport (T8): `pty_open` / `pty_write` / `pty_resize` / `pty_close`
 *   commands, one Tauri `Channel<ArrayBuffer>` per pty for raw output bytes,
 *   the global `pty-exit` event, and for autocomplete the `pty_complete`
 *   command and the global `pty-shell` event.
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
  /**
   * Working directory: an absolute path to a folder. The core falls back to the
   * user's home, without error, when it is relative, too long, missing or not a
   * folder. Only ever taken from `PtySession.info()`, never from OSC 7 (D23).
   */
  readonly cwd?: string;
}

/** Result of `pty_info`, computed by the core from the OS when asked. */
export interface PtyInfo {
  /** Shell program name, without folder or extension: "zsh", "fish", "bash", "pwsh", "cmd". */
  readonly shell: string;
  /** Current directory of the shell process, or null when unknown. */
  readonly cwd: string | null;
  /** true: a program runs in place of the shell; false: idle shell; null: unknown. */
  readonly busy: boolean | null;
  /** Foreground program name when busy is true (at most 64 chars), else null. */
  readonly foreground: string | null;
}

export interface PtyExit {
  /** Process exit code, or null when killed by a signal / unknown. */
  readonly exitCode: number | null;
}

/**
 * Shell integration became active in a pty (`pty-shell` event): the shell
 * loaded termbay's script and sent its handshake. Only then can `complete`
 * succeed. See docs/AUTOCOMPLETE.md, section 4.
 */
export interface ShellInfo {
  /** "fish" | "zsh" in milestone A; more later. */
  readonly shell: string;
  /** Integration protocol version; the UI understands 1. */
  readonly protocol: number;
}

/** One suggestion returned by `pty_complete`. */
export interface CompletionItem {
  /** Label shown in the menu. */
  readonly text: string;
  /**
   * Insertion plan computed by the core against the `line`/`cursor` the shell
   * reported: either the suffix to type after the cursor, or N x DEL (0x7f)
   * followed by the replacement text.
   */
  readonly insert: string;
  readonly description?: string;
  readonly kind: "shell" | "history";
}

/** Result of `pty_complete`. */
export type CompletionResult =
  | {
      readonly status: "ok";
      /** Command line as the shell saw it when the request arrived. */
      readonly line: string;
      /** Cursor position in `line`. */
      readonly cursor: number;
      /** Start of the word being completed, in `line`. */
      readonly replaceFrom: number;
      readonly items: readonly CompletionItem[];
    }
  /** The shell is not at its prompt, or there is no integration: give the key to the shell. */
  | { readonly status: "unavailable" }
  /** The shell did not answer in time. */
  | { readonly status: "timeout" };

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

  /**
   * Asks the shell for completions of the current command line (`pty_complete`).
   * A new request cancels the previous one.
   */
  complete(): Promise<CompletionResult>;

  /**
   * Fired when shell integration becomes active (`pty-shell`). A listener added
   * after that is called at once. Never fires when autocomplete is disabled.
   */
  onShell(listener: (info: ShellInfo) => void): Disposable;

  /** Asks the core about the shell process (`pty_info`). Computed on demand. */
  info(): Promise<PtyInfo>;
}

export interface PtyTransport {
  open(options: PtyOpenOptions): Promise<PtySession>;
}
