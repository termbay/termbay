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
}

/**
 * Wires a terminal to a pty session:
 * - keystrokes and paste (`onData`/`onBinary`) become `session.write`;
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

  return {
    dispose: () => {
      for (const d of disposables.splice(0)) d.dispose();
    },
  };
}
