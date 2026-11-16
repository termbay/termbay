import { Emitter } from "./emitter";
import type { Disposable, PtyExit, PtyOpenOptions, PtySession, PtyTransport } from "./types";

const CR = 0x0d;
const LF = 0x0a;
const DEL = 0x7f;
const BACKSPACE_ERASE = new Uint8Array([0x08, 0x20, 0x08]); // "\b \b"

/**
 * Development-only transport: every byte written comes back as output, with a
 * tiny line discipline so typing looks right in the terminal (Enter becomes
 * CR LF, Backspace erases a cell). It is NOT `termbay serve` and runs no shell.
 */
export class EchoPtyTransport implements PtyTransport {
  private nextId = 1;

  constructor(private readonly options: { banner?: string } = {}) {}

  async open(options: PtyOpenOptions): Promise<EchoPtySession> {
    const session = new EchoPtySession(`echo-${this.nextId++}`, options.cols, options.rows);
    if (this.options.banner) session.emitOutput(new TextEncoder().encode(this.options.banner));
    return session;
  }
}

export class EchoPtySession implements PtySession {
  private readonly data = new Emitter<Uint8Array>({ bufferUntilListener: true });
  private readonly exit = new Emitter<PtyExit>();
  private closed = false;
  /** Total output bytes acknowledged by the UI (observable in tests). */
  ackedBytes = 0;

  constructor(
    readonly id: string,
    public cols: number,
    public rows: number,
  ) {}

  async write(input: Uint8Array): Promise<void> {
    if (this.closed) throw new Error(`pty ${this.id} is closed`);
    this.emitOutput(echo(input));
  }

  async resize(cols: number, rows: number): Promise<void> {
    if (this.closed) return;
    this.cols = cols;
    this.rows = rows;
  }

  ack(bytes: number): void {
    this.ackedBytes += bytes;
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.exit.emit({ exitCode: 0 });
    this.data.clear();
    this.exit.clear();
  }

  onData(listener: (chunk: Uint8Array) => void): Disposable {
    return this.data.on(listener);
  }

  onExit(listener: (exit: PtyExit) => void): Disposable {
    return this.exit.on(listener);
  }

  /** @internal */
  emitOutput(chunk: Uint8Array): void {
    if (chunk.byteLength > 0) this.data.emit(chunk);
  }
}

/** Maps input bytes to what a cooked-mode tty would echo. */
export function echo(input: Uint8Array): Uint8Array {
  const out: number[] = [];
  for (const byte of input) {
    if (byte === CR) out.push(CR, LF);
    else if (byte === DEL) out.push(...BACKSPACE_ERASE);
    else out.push(byte);
  }
  return Uint8Array.from(out);
}
