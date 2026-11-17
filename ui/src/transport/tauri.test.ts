import { Channel } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { connectPty, type TerminalLike } from "../terminal/connect";
import { Emitter } from "./emitter";
import {
  PTY_EXIT_EVENT,
  PTY_ID_HEADER,
  PTY_SHELL_EVENT,
  TauriPtyTransport,
  hookChannelEnd,
  type ChannelMessage,
  type PtyExitPayload,
  type PtyShellPayload,
  type TauriIpc,
} from "./tauri";
import { DEFAULT_CONFIG, loadConfig } from "./config";
import type { Disposable } from "./types";

interface Call {
  cmd: string;
  args: unknown;
  options: unknown;
}

/** In-memory stand-in for the Tauri backend: records invokes, drives channels and events. */
class FakeIpc implements TauriIpc {
  readonly calls: Call[] = [];
  readonly channels: Array<(message: ChannelMessage) => void> = [];
  readonly channelEnds: Array<() => void> = [];
  /** When set, `pty_write` calls stay pending until settled by the test. */
  deferWrites = false;
  readonly pendingWrites: Array<{ bytes: Uint8Array; resolve: () => void; reject: (e: unknown) => void }> = [];
  /** Bytes in the order the "core" accepted them (when each write settles successfully). */
  readonly received: Uint8Array[] = [];
  maxWritesInFlight = 0;
  private readonly listeners = new Map<string, Set<(payload: unknown) => void>>();
  nextId = 7;
  failWith: Partial<Record<string, string>> = {};
  /** Answer of `pty_complete`. */
  completion: unknown = { status: "unavailable" };
  /** Answer of `pty_info`. */
  ptyInfo: unknown = { shell: "zsh", cwd: "/home/u", busy: false, foreground: null };
  /** Answer of `get_config`. */
  config: unknown = { autocomplete: { enabled: true, trigger: "ctrl+space", history: false } };
  /** Runs inside `pty_open` before it resolves (e.g. to emit an early exit). */
  onOpen: ((id: number) => void) | undefined;

  async invoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
    this.calls.push({ cmd, args, options });
    const failure = this.failWith[cmd];
    if (failure !== undefined) throw failure; // Tauri rejects with a plain string.
    if (cmd === "pty_write" && this.deferWrites) {
      return new Promise<T>((resolve, reject) => {
        const bytes = args as Uint8Array;
        this.pendingWrites.push({
          bytes,
          resolve: () => {
            this.received.push(bytes);
            resolve(undefined as T);
          },
          reject,
        });
        this.maxWritesInFlight = Math.max(this.maxWritesInFlight, this.pendingWrites.length);
      });
    }
    if (cmd === "pty_complete") return this.completion as T;
    if (cmd === "pty_info") return this.ptyInfo as T;
    if (cmd === "get_config") return this.config as T;
    if (cmd === "pty_open") {
      const id = this.nextId++;
      this.onOpen?.(id);
      return id as T;
    }
    return undefined as T;
  }

  channel(onMessage: (message: ChannelMessage) => void, onEnd: () => void): unknown {
    this.channels.push(onMessage);
    this.channelEnds.push(onEnd);
    return { channelId: this.channels.length - 1 };
  }

  async listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    const set = this.listeners.get(event) ?? new Set();
    this.listeners.set(event, set);
    const h = handler as (payload: unknown) => void;
    set.add(h);
    return () => set.delete(h);
  }

  emit(event: string, payload: unknown): void {
    for (const h of this.listeners.get(event) ?? []) h(payload);
  }

  listenerCount(event: string): number {
    return this.listeners.get(event)?.size ?? 0;
  }

  callsOf(cmd: string): Call[] {
    return this.calls.filter((c) => c.cmd === cmd);
  }
}

class FakeTerminal implements TerminalLike {
  cols = 80;
  rows = 24;
  readonly written: Array<string | Uint8Array> = [];
  private readonly callbacks: Array<() => void> = [];
  private readonly data = new Emitter<string>();
  private readonly binary = new Emitter<string>();
  private readonly resized = new Emitter<{ cols: number; rows: number }>();

  write(data: string | Uint8Array, callback?: () => void): void {
    this.written.push(data);
    if (callback) this.callbacks.push(callback);
  }
  onData(l: (d: string) => void): Disposable {
    return this.data.on(l);
  }
  onBinary(l: (d: string) => void): Disposable {
    return this.binary.on(l);
  }
  onResize(l: (s: { cols: number; rows: number }) => void): Disposable {
    return this.resized.on(l);
  }
  type(text: string) {
    this.data.emit(text);
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.resized.emit({ cols, rows });
  }
  flushRender() {
    for (const cb of this.callbacks.splice(0)) cb();
  }
}

const tick = () => new Promise((r) => setTimeout(r, 0));
const bytes = (s: string) => new TextEncoder().encode(s);

async function setup(exitDrainTimeoutMs?: number) {
  const ipc = new FakeIpc();
  const transport = new TauriPtyTransport(ipc, exitDrainTimeoutMs === undefined ? {} : { exitDrainTimeoutMs });
  const session = await transport.open({ cols: 80, rows: 24 });
  const term = new FakeTerminal();
  const send = (message: ChannelMessage) => ipc.channels[0]?.(message);
  const endChannel = () => ipc.channelEnds[0]?.();
  return { ipc, transport, session, term, send, endChannel };
}

describe("TauriPtyTransport", () => {
  it("opens the pty with the size and an output channel", async () => {
    const { ipc, session } = await setup();
    const [open] = ipc.callsOf("pty_open");
    expect(open?.args).toEqual({ cols: 80, rows: 24, onData: { channelId: 0 } });
    expect(session.id).toBe("7");
    expect(ipc.listenerCount(PTY_EXIT_EVENT)).toBe(1);
  });

  it("listens to pty-exit once for many sessions", async () => {
    const { ipc, transport } = await setup();
    await transport.open({ cols: 80, rows: 24 });
    expect(ipc.listenerCount(PTY_EXIT_EVENT)).toBe(1);
    await transport.dispose();
    expect(ipc.listenerCount(PTY_EXIT_EVENT)).toBe(0);
  });

  it("rejects with an Error carrying the command's message", async () => {
    const ipc = new FakeIpc();
    ipc.failWith.pty_open = "no shell found";
    await expect(new TauriPtyTransport(ipc).open({ cols: 80, rows: 24 })).rejects.toThrow("no shell found");
  });

  it("turns typing into pty_write with the raw bytes and the id header", async () => {
    const { ipc, session, term } = await setup();
    connectPty(term, session);
    term.type("ls ção\r");
    await tick();

    const writes = ipc.callsOf("pty_write");
    expect(writes).toHaveLength(1);
    expect(writes[0]?.args).toEqual(bytes("ls ção\r"));
    expect(writes[0]?.args).toBeInstanceOf(Uint8Array);
    expect(writes[0]?.options).toEqual({ headers: { [PTY_ID_HEADER]: "7" } });
  });

  it("turns terminal resizes into pty_resize", async () => {
    const { ipc, session, term } = await setup();
    term.cols = 100;
    term.rows = 30;
    connectPty(term, session);
    term.resize(132, 43);
    await tick();

    expect(ipc.callsOf("pty_resize").map((c) => c.args)).toEqual([
      { id: 7, cols: 100, rows: 30 },
      { id: 7, cols: 132, rows: 43 },
    ]);
  });

  it("writes channel messages to the terminal, including output sent before attaching", async () => {
    const { session, term, send } = await setup();
    send(bytes("early ").buffer);
    connectPty(term, session);
    send(bytes("prompt$ ").buffer);

    expect(term.written).toEqual([bytes("early "), bytes("prompt$ ")]);
  });

  it("acks rendered bytes with one pty_ack per render pass", async () => {
    const { ipc, session, term, send } = await setup();
    connectPty(term, session);
    send(bytes("abc").buffer);
    send(bytes("de").buffer);
    await tick();
    expect(ipc.callsOf("pty_ack")).toHaveLength(0);

    term.flushRender();
    await tick();
    expect(ipc.callsOf("pty_ack").map((c) => c.args)).toEqual([{ id: 7, bytes: 5 }]);
  });

  it("fires onExit for its own pty-exit only", async () => {
    const { ipc, session, term, endChannel } = await setup();
    connectPty(term, session);
    const onExit = vi.fn();
    session.onExit(onExit);
    endChannel();

    ipc.emit(PTY_EXIT_EVENT, { id: 8, exitCode: 1 } satisfies PtyExitPayload);
    expect(onExit).not.toHaveBeenCalled();

    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 0 } satisfies PtyExitPayload);
    expect(onExit).toHaveBeenCalledExactlyOnceWith({ exitCode: 0 });
    expect(term.written.at(-1)).toContain("[process exited with code 0]");

    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 0 } satisfies PtyExitPayload);
    expect(onExit).toHaveBeenCalledOnce();
  });

  it("delivers an exit that arrived before pty_open resolved", async () => {
    const ipc = new FakeIpc();
    ipc.onOpen = (id) => {
      ipc.channelEnds[0]?.();
      ipc.emit(PTY_EXIT_EVENT, { id, exitCode: 127 } satisfies PtyExitPayload);
    };
    const session = await new TauriPtyTransport(ipc).open({ cols: 80, rows: 24 });
    const term = new FakeTerminal();
    connectPty(term, session);
    expect(term.written.at(-1)).toContain("[process exited with code 127]");
    await session.resize(10, 10);
    expect(ipc.callsOf("pty_resize")).toHaveLength(0);
  });

  it("calls pty_close once, however many times close is called", async () => {
    const { ipc, session } = await setup();
    const onExit = vi.fn();
    session.onExit(onExit);

    await Promise.all([session.close(), session.close()]);
    await session.close();

    expect(ipc.callsOf("pty_close").map((c) => c.args)).toEqual([{ id: 7 }]);
    expect(onExit).toHaveBeenCalledExactlyOnceWith({ exitCode: null });
    await expect(session.write(bytes("x"))).rejects.toThrow("closed");

    // A late pty-exit from the core for the closed pty is ignored.
    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 0 } satisfies PtyExitPayload);
    expect(onExit).toHaveBeenCalledOnce();
  });

  it("reports write errors from the core as Error", async () => {
    const { ipc, session } = await setup();
    ipc.failWith.pty_write = "pty 7 not found";
    await expect(session.write(bytes("x"))).rejects.toThrow("pty 7 not found");
  });

  it("accepts channel messages as Uint8Array or number arrays", async () => {
    const { session, send } = await setup();
    const chunks: Uint8Array[] = [];
    session.onData((c) => chunks.push(c));
    send(Uint8Array.of(1, 2));
    send([3]);
    send(new ArrayBuffer(0));
    expect(chunks.map((c) => Array.from(c))).toEqual([[1, 2], [3]]);
  });

  it("delivers writes in the order they were made, one request at a time", async () => {
    const { ipc, session } = await setup();
    ipc.deferWrites = true;
    const done = [session.write(bytes("a")), session.write(bytes("b")), session.write(bytes("c"))];

    // Settle whatever is in flight, newest first, as an out-of-order backend would.
    for (let round = 0; round < 10 && ipc.received.length < 3; round++) {
      await tick();
      for (const w of ipc.pendingWrites.splice(0).reverse()) w.resolve();
    }
    await Promise.all(done);

    expect(ipc.received.map((b) => new TextDecoder().decode(b))).toEqual(["a", "b", "c"]);
    expect(ipc.maxWritesInFlight).toBe(1);
  });

  it("keeps the write queue going after a failed write", async () => {
    const { ipc, session } = await setup();
    ipc.deferWrites = true;
    const first = session.write(bytes("a"));
    const second = session.write(bytes("b"));
    await tick();
    ipc.pendingWrites.shift()?.reject("pty busy");
    await expect(first).rejects.toThrow("pty busy");
    await tick();
    ipc.pendingWrites.shift()?.resolve();
    await expect(second).resolves.toBeUndefined();
    expect(ipc.received).toEqual([bytes("b")]);
  });

  it("rejects writes queued before close that had not been sent yet", async () => {
    const { ipc, session } = await setup();
    ipc.deferWrites = true;
    const first = session.write(bytes("a"));
    const queued = session.write(bytes("b"));
    await tick();
    const closed = session.close();
    ipc.pendingWrites.shift()?.resolve();
    await first;
    await expect(queued).rejects.toThrow("closed");
    await closed;
    expect(ipc.callsOf("pty_write")).toHaveLength(1);
  });

  it("waits for the channel to finish before reporting a pty-exit that overtook the last chunk", async () => {
    const { ipc, session, send, endChannel } = await setup();
    const events: string[] = [];
    session.onData((c) => events.push(`data:${new TextDecoder().decode(c)}`));
    session.onExit((e) => events.push(`exit:${e.exitCode}`));

    send(bytes("first").buffer);
    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 0 } satisfies PtyExitPayload);
    expect(events).toEqual(["data:first"]);

    send(bytes("x".repeat(2048)).buffer); // a large chunk that came through a slower fetch
    endChannel();
    expect(events).toEqual(["data:first", `data:${"x".repeat(2048)}`, "exit:0"]);
  });

  it("reports the exit after a short timeout if the channel never signals its end", async () => {
    const { ipc, session } = await setup(5);
    const onExit = vi.fn();
    session.onExit(onExit);
    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 2 } satisfies PtyExitPayload);
    expect(onExit).not.toHaveBeenCalled();
    await new Promise((r) => setTimeout(r, 20));
    expect(onExit).toHaveBeenCalledExactlyOnceWith({ exitCode: 2 });
  });

  it("reports the exit at once when the channel already ended", async () => {
    const { ipc, session, endChannel } = await setup();
    const onExit = vi.fn();
    session.onExit(onExit);
    endChannel();
    ipc.emit(PTY_EXIT_EVENT, { id: 7, exitCode: 0 } satisfies PtyExitPayload);
    expect(onExit).toHaveBeenCalledOnce();
  });
});

describe("hookChannelEnd", () => {
  it("runs onEnd after the channel's own cleanup", () => {
    const order: string[] = [];
    const channel = {
      cleanupCallback() {
        order.push("cleanup");
      },
    };
    expect(hookChannelEnd(channel, () => order.push("end"))).toBe(true);
    channel.cleanupCallback();
    expect(order).toEqual(["cleanup", "end"]);
  });

  it("reports when the channel has no cleanup hook", () => {
    expect(hookChannelEnd({}, () => {})).toBe(false);
  });

  it("fires on the real Channel only after the end marker and every earlier message", () => {
    // Minimal stand-in for what the Tauri webview injects; restored afterwards.
    const g = globalThis as { window?: unknown };
    const previous = g.window;
    let deliver: (raw: unknown) => void = () => {};
    g.window = {
      __TAURI_INTERNALS__: {
        transformCallback: (cb: (raw: unknown) => void) => {
          deliver = cb;
          return 1;
        },
        unregisterCallback: () => {},
      },
    };
    try {
      const events: string[] = [];
      const channel = new Channel<string>((m) => events.push(m));
      expect(hookChannelEnd(channel, () => events.push("end"))).toBe(true);

      deliver({ index: 0, message: "a" });
      deliver({ index: 2, end: true }); // end marker overtakes message 1
      expect(events).toEqual(["a"]);
      deliver({ index: 1, message: "b" });
      expect(events).toEqual(["a", "b", "end"]);
    } finally {
      g.window = previous;
    }
  });
});

describe("TauriPtyTransport autocomplete", () => {
  it("calls pty_complete with the pty id and returns the core's answer", async () => {
    const { ipc, session } = await setup();
    const answer = {
      status: "ok",
      line: "git ch",
      cursor: 6,
      replaceFrom: 4,
      items: [{ text: "checkout", insert: "eckout", description: "switch branches", kind: "shell" }],
    };
    ipc.completion = answer;
    await expect(session.complete()).resolves.toEqual(answer);
    expect(ipc.callsOf("pty_complete").map((c) => c.args)).toEqual([{ id: 7 }]);
  });

  it("sends pty_complete only after the writes made before it reached the core", async () => {
    const { ipc, session } = await setup();
    ipc.deferWrites = true;
    void session.write(bytes("ch"));
    const result = session.complete();
    await tick();
    expect(ipc.callsOf("pty_complete")).toHaveLength(0);
    ipc.pendingWrites[0]?.resolve();
    await expect(result).resolves.toEqual({ status: "unavailable" });
    expect(ipc.calls.map((c) => c.cmd).slice(-2)).toEqual(["pty_write", "pty_complete"]);
  });

  it("rejects with an Error when pty_complete fails", async () => {
    const { ipc, session } = await setup();
    ipc.failWith.pty_complete = "unknown command";
    await expect(session.complete()).rejects.toThrow("unknown command");
  });

  it("answers unavailable without calling the core once closed", async () => {
    const { ipc, session } = await setup();
    await session.close();
    await expect(session.complete()).resolves.toEqual({ status: "unavailable" });
    expect(ipc.callsOf("pty_complete")).toHaveLength(0);
  });

  it("delivers pty-shell to its own session only, and to listeners added later", async () => {
    const { ipc, transport, session } = await setup();
    const other = await transport.open({ cols: 80, rows: 24 });
    expect(ipc.listenerCount(PTY_SHELL_EVENT)).toBe(1);
    const mine = vi.fn();
    const theirs = vi.fn();
    session.onShell(mine);
    other.onShell(theirs);

    ipc.emit(PTY_SHELL_EVENT, { id: 7, shell: "zsh", protocol: 1 } satisfies PtyShellPayload);
    expect(mine).toHaveBeenCalledWith({ shell: "zsh", protocol: 1 });
    expect(theirs).not.toHaveBeenCalled();

    const late = vi.fn();
    session.onShell(late);
    expect(late).toHaveBeenCalledWith({ shell: "zsh", protocol: 1 });
  });

  it("delivers a pty-shell that arrived before pty_open resolved", async () => {
    const ipc = new FakeIpc();
    ipc.onOpen = (id) => ipc.emit(PTY_SHELL_EVENT, { id, shell: "fish", protocol: 1 } satisfies PtyShellPayload);
    const session = await new TauriPtyTransport(ipc).open({ cols: 80, rows: 24 });
    const onShell = vi.fn();
    session.onShell(onShell);
    expect(onShell).toHaveBeenCalledWith({ shell: "fish", protocol: 1 });
  });

  it("stops listening to pty-shell on dispose", async () => {
    const { ipc, transport } = await setup();
    await transport.dispose();
    expect(ipc.listenerCount(PTY_SHELL_EVENT)).toBe(0);
  });
});

describe("milestone 2 contract (FASE1-PLANO 6.3)", () => {
  it("sends pty_open without cwd, and with cwd when given, never env", async () => {
    const ipc = new FakeIpc();
    const transport = new TauriPtyTransport(ipc);
    await transport.open({ cols: 80, rows: 24 });
    await transport.open({ cols: 100, rows: 30, cwd: "/tmp/x" });
    const [a, b] = ipc.callsOf("pty_open").map((c) => c.args as Record<string, unknown>);
    expect(Object.keys(a ?? {}).sort()).toEqual(["cols", "onData", "rows"]);
    expect(b).toMatchObject({ cols: 100, rows: 30, cwd: "/tmp/x" });
    expect(b).not.toHaveProperty("env");
  });

  it("turns the pty limit error into an Error", async () => {
    const ipc = new FakeIpc();
    ipc.failWith.pty_open = "too many terminals are open (limit 64)";
    await expect(new TauriPtyTransport(ipc).open({ cols: 80, rows: 24 })).rejects.toThrow(
      new Error("too many terminals are open (limit 64)"),
    );
  });

  it("calls pty_info with the numeric id and returns the answer", async () => {
    const ipc = new FakeIpc();
    const session = await new TauriPtyTransport(ipc).open({ cols: 80, rows: 24 });
    await expect(session.info()).resolves.toEqual({ shell: "zsh", cwd: "/home/u", busy: false, foreground: null });
    expect(ipc.callsOf("pty_info")[0]?.args).toEqual({ id: 7 });
    ipc.failWith.pty_info = "no pty with id 7";
    await expect(session.info()).rejects.toThrow(new Error("no pty with id 7"));
  });

  it("loads the config with get_config, defaults outside Tauri and for bad fields", async () => {
    const ipc = new FakeIpc();
    await expect(loadConfig(ipc)).resolves.toEqual({ autocomplete: { enabled: true, trigger: "ctrl+space", history: false } });
    expect(ipc.callsOf("get_config")[0]?.args).toBeUndefined();
    await expect(loadConfig()).resolves.toEqual(DEFAULT_CONFIG);
    ipc.config = { autocomplete: { enabled: "yes", trigger: "alt" } };
    await expect(loadConfig(ipc)).resolves.toEqual(DEFAULT_CONFIG);
    ipc.failWith.get_config = "boom";
    await expect(loadConfig(ipc)).rejects.toThrow(new Error("boom"));
  });
});
