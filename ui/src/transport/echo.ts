import { Emitter } from "./emitter";
import type {
  CompletionItem,
  CompletionResult,
  Disposable,
  PtyExit,
  PtyInfo,
  PtyOpenOptions,
  PtySession,
  PtyTransport,
  ShellInfo,
} from "./types";

const CR = 0x0d;
const LF = 0x0a;
const DEL = 0x7f;
const BACKSPACE_ERASE = new Uint8Array([0x08, 0x20, 0x08]); // "\b \b"

/**
 * Development-only transport: every byte written comes back as output, with a
 * tiny line discipline so typing looks right in the terminal (Enter becomes
 * CR LF, Backspace erases a cell). It is NOT `termbay serve` and runs no shell.
 *
 * With `simulateShell`, it also pretends a shell with integration sits at its
 * prompt and answers `complete` from a fixed list, so the autocomplete menu
 * can be developed with `npm run dev`. Without it, `complete` is `unavailable`.
 */
export class EchoPtyTransport implements PtyTransport {
  private nextId = 1;

  constructor(private readonly options: { banner?: string; simulateShell?: boolean } = {}) {}

  async open(options: PtyOpenOptions): Promise<EchoPtySession> {
    const session = new EchoPtySession(`echo-${this.nextId++}`, options.cols, options.rows);
    if (this.options.banner) session.emitOutput(new TextEncoder().encode(this.options.banner));
    if (this.options.simulateShell) session.simulateShell();
    return session;
  }
}

/** Fixed suggestions of the simulated shell: first word, then `git` subcommands. */
const ECHO_COMMANDS: ReadonlyArray<readonly [string, string]> = [
  ["cargo", "Rust package manager"],
  ["cat", "concatenate files"],
  ["cd", "change directory"],
  ["git", "version control"],
  ["grep", "search text"],
  ["ls", "list directory"],
  ["npm", "Node package manager"],
];
const ECHO_GIT: ReadonlyArray<readonly [string, string]> = [
  ["checkout", "switch branches"],
  ["cherry-pick", "apply a commit"],
  ["cherry", "find unapplied commits"],
  ["clone", "clone a repository"],
  ["commit", "record changes"],
  ["status", "show the working tree status"],
];
const ECHO_HISTORY = ["git checkout main", "git status", "ls -la"];
const PASTE_MARKERS = /\x1b\[20[01]~/g;

export class EchoPtySession implements PtySession {
  private readonly data = new Emitter<Uint8Array>({ bufferUntilListener: true });
  private readonly exit = new Emitter<PtyExit>();
  private closed = false;
  private readonly shell = new Emitter<ShellInfo>();
  private shellInfo: ShellInfo | null = null;
  /** Command line of the simulated shell (what was typed since the last Enter). */
  private line = "";
  /** Total output bytes acknowledged by the UI (observable in tests). */
  ackedBytes = 0;

  constructor(
    readonly id: string,
    public cols: number,
    public rows: number,
  ) {}

  async write(input: Uint8Array): Promise<void> {
    if (this.closed) throw new Error(`pty ${this.id} is closed`);
    if (this.shellInfo) this.trackLine(input);
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

  async complete(): Promise<CompletionResult> {
    if (this.closed || !this.shellInfo) return { status: "unavailable" };
    return echoCompletions(this.line);
  }

  onShell(listener: (info: ShellInfo) => void): Disposable {
    if (this.shellInfo) listener(this.shellInfo);
    return this.shell.on(listener);
  }

  async info(): Promise<PtyInfo> {
    if (this.closed) throw new Error(`no pty with id ${this.id}`);
    return { shell: "echo", cwd: null, busy: false, foreground: null };
  }

  /** @internal Acts as if the shell sent its integration handshake. */
  simulateShell(): void {
    this.shellInfo = { shell: "echo", protocol: 1 };
    this.shell.emit(this.shellInfo);
  }

  private trackLine(input: Uint8Array): void {
    const text = new TextDecoder().decode(input).replace(PASTE_MARKERS, "");
    for (const ch of text) {
      if (ch === "\r" || ch === "\n" || ch === "\x03") this.line = "";
      else if (ch === "\x7f") this.line = Array.from(this.line).slice(0, -1).join("");
      else if (ch >= " ") this.line += ch;
    }
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

/** Completions of the simulated shell for a command line, with the cursor at its end. */
export function echoCompletions(line: string): CompletionResult {
  const chars = Array.from(line);
  const cursor = chars.length;
  const replaceFrom = Math.max(0, line.lastIndexOf(" ") + 1);
  const word = line.slice(replaceFrom);
  const replaceFromChars = Array.from(line.slice(0, replaceFrom)).length;
  const words = line.slice(0, replaceFrom).trim().split(/\s+/).filter(Boolean);
  const source = words.length === 0 ? ECHO_COMMANDS : words.length === 1 && words[0] === "git" ? ECHO_GIT : [];

  const items: CompletionItem[] = source
    .filter(([name]) => name.startsWith(word))
    .map(([name, description]) => ({ text: name, insert: name.slice(word.length), description, kind: "shell" }));
  if (line.length > 0) {
    for (const entry of ECHO_HISTORY) {
      if (entry.startsWith(line) && entry !== line) {
        items.push({ text: entry, insert: entry.slice(line.length), kind: "history" });
      }
    }
  }
  return { status: "ok", line, cursor, replaceFrom: replaceFromChars, items };
}
