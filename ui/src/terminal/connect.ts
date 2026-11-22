import type { Disposable, PtySession } from "../transport/types";

/**
 * The subset of the xterm.js `Terminal` used to talk to a pty. Kept small so
 * the wiring can be tested without a DOM.
 */
export interface TerminalLike {
  readonly cols: number;
  readonly rows: number;
  write(data: string | Uint8Array, callback?: () => void): void;
  onData(listener: (data: string) => void): Disposable;
  onBinary(listener: (data: string) => void): Disposable;
  onResize(listener: (size: { cols: number; rows: number }) => void): Disposable;
}

const encoder = new TextEncoder();

/** xterm.js `onBinary` delivers one byte per UTF-16 code unit (mouse reports). */
export function binaryStringToBytes(data: string): Uint8Array {
  const bytes = new Uint8Array(data.length);
  for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i) & 0xff;
  return bytes;
}

export interface ConnectOptions {
  /** Called for transport errors (write/resize failures). Defaults to console.error. */
  onError?: (error: unknown) => void;
  /** Input typed before the session existed (see `bufferEarlyInput`), sent first. */
  earlyInput?: readonly Uint8Array[];
}

/** Most input kept while a pane waits for its shell; anything beyond is dropped. */
export const EARLY_INPUT_LIMIT = 64 * 1024;

/** Keystrokes typed into a terminal before its pty session exists. */
export interface EarlyInput extends Disposable {
  /** Stops collecting and returns what was typed, in order. */
  take(): Uint8Array[];
}

/**
 * Collects a terminal's input until `take()`. A new pane gets the focus at
 * once, but its shell starts later (more so on Windows, where ConPTY is
 * slow): without this, whatever the user types in between is lost.
 */
export function bufferEarlyInput(term: Pick<TerminalLike, "onData" | "onBinary">, limit = EARLY_INPUT_LIMIT): EarlyInput {
  let chunks: Uint8Array[] = [];
  let size = 0;
  const keep = (bytes: Uint8Array) => {
    if (size + bytes.byteLength > limit) return;
    chunks.push(bytes);
    size += bytes.byteLength;
  };
  const listeners: Disposable[] = [
    term.onData((data) => keep(encoder.encode(data))),
    term.onBinary((data) => keep(binaryStringToBytes(data))),
  ];
  const dispose = () => {
    for (const d of listeners.splice(0)) d.dispose();
  };
  return {
    take: () => {
      dispose();
      const taken = chunks;
      chunks = [];
      size = 0;
      return taken;
    },
    dispose: () => {
      dispose();
      chunks = [];
      size = 0;
    },
  };
}

/**
 * Wires a terminal to a pty session:
 * - keystrokes and paste (`onData`/`onBinary`) become `session.write`,
 *   after any `earlyInput` typed while the session was starting;
 * - pty output becomes `term.write`, and is acknowledged once rendered (flow control);
 * - terminal resizes become `session.resize`;
 * - pty exit prints a short notice.
 */
export function connectPty(term: TerminalLike, session: PtySession, options: ConnectOptions = {}): Disposable {
  const onError = options.onError ?? ((error: unknown) => console.error("[termbay] pty error", error));
  const send = (bytes: Uint8Array) => {
    session.write(bytes).catch(onError);
  };

  const disposables: Disposable[] = [
    term.onData((data) => send(encoder.encode(data))),
    term.onBinary((data) => send(binaryStringToBytes(data))),
    term.onResize(({ cols, rows }) => {
      session.resize(cols, rows).catch(onError);
    }),
    session.onData((chunk) => {
      term.write(chunk, () => session.ack(chunk.byteLength));
    }),
    session.onExit(({ exitCode }) => {
      const code = exitCode === null ? "unknown" : String(exitCode);
      term.write(`\r\n[process exited with code ${code}]\r\n`);
    }),
  ];

  // The terminal may have been fitted before the session existed.
  session.resize(term.cols, term.rows).catch(onError);
  // Writes are queued in order, so this goes before any later keystroke.
  for (const bytes of options.earlyInput ?? []) send(bytes);

  return {
    dispose: () => {
      for (const d of disposables.splice(0)) d.dispose();
    },
  };
}
