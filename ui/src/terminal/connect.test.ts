import { describe, expect, it, vi } from "vitest";
import { Emitter } from "../transport/emitter";
import { EchoPtyTransport } from "../transport/echo";
import type { Disposable } from "../transport/types";
import { binaryStringToBytes, connectPty, type TerminalLike } from "./connect";

/** Records what would be drawn; completes writes asynchronously like xterm.js. */
class FakeTerminal implements TerminalLike {
  cols = 80;
  rows = 24;
  readonly written: Array<string | Uint8Array> = [];
  private readonly pendingCallbacks: Array<() => void> = [];
  private readonly data = new Emitter<string>();
  private readonly binary = new Emitter<string>();
  private readonly resized = new Emitter<{ cols: number; rows: number }>();

  write(data: string | Uint8Array, callback?: () => void): void {
    this.written.push(data);
    if (callback) this.pendingCallbacks.push(callback);
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
  sendBinary(text: string) {
    this.binary.emit(text);
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.resized.emit({ cols, rows });
  }
  flushRender() {
    for (const cb of this.pendingCallbacks.splice(0)) cb();
  }
  text(): string {
    const dec = new TextDecoder();
    return this.written.map((w) => (typeof w === "string" ? w : dec.decode(w))).join("");
  }
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("connectPty with the echo transport", () => {
  it("writes the echoed input to the terminal", async () => {
    const term = new FakeTerminal();
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    connectPty(term, session);

    term.type("echo ção\r");
    await tick();

    expect(term.text()).toBe("echo ção\r\n");
  });

  it("acknowledges output only after the terminal rendered it", async () => {
    const term = new FakeTerminal();
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    connectPty(term, session);

    term.type("abc");
    await tick();
    expect(session.ackedBytes).toBe(0);
    term.flushRender();
    expect(session.ackedBytes).toBe(3);
  });

  it("sends binary input byte for byte", async () => {
    const term = new FakeTerminal();
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const write = vi.spyOn(session, "write");
    connectPty(term, session);

    term.sendBinary("\x1b[M\xff!");
    await tick();
    expect(write).toHaveBeenCalledWith(Uint8Array.of(0x1b, 0x5b, 0x4d, 0xff, 0x21));
  });

  it("syncs the initial size and forwards resizes", async () => {
    const term = new FakeTerminal();
    term.cols = 100;
    term.rows = 30;
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    connectPty(term, session);
    await tick();
    expect([session.cols, session.rows]).toEqual([100, 30]);

    term.resize(132, 43);
    await tick();
    expect([session.cols, session.rows]).toEqual([132, 43]);
  });

  it("prints a notice when the process exits", async () => {
    const term = new FakeTerminal();
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    connectPty(term, session);
    await session.close();
    expect(term.text()).toContain("[process exited with code 0]");
  });

  it("stops forwarding after dispose and reports write errors", async () => {
    const term = new FakeTerminal();
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const onError = vi.fn();
    const connection = connectPty(term, session, { onError });

    await session.close();
    term.type("x");
    await tick();
    expect(onError).toHaveBeenCalledOnce();

    connection.dispose();
    term.type("y");
    await tick();
    expect(onError).toHaveBeenCalledOnce();
  });
});

describe("binaryStringToBytes", () => {
  it("keeps the low byte of each code unit", () => {
    expect(Array.from(binaryStringToBytes("\x00\x7f\x80\xff"))).toEqual([0, 0x7f, 0x80, 0xff]);
  });
});
