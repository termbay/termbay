import { describe, expect, it } from "vitest";
import { EchoPtyTransport, echo } from "./echo";

const enc = (s: string) => new TextEncoder().encode(s);
const dec = (b: Uint8Array) => new TextDecoder().decode(b);

describe("EchoPtyTransport", () => {
  it("returns what it receives", async () => {
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const chunks: Uint8Array[] = [];
    session.onData((c) => chunks.push(c));

    const input = enc("hello, ção 🚀");
    await session.write(input);

    expect(chunks).toHaveLength(1);
    expect(chunks[0]).toEqual(input);
  });

  it("buffers output emitted before the first listener", async () => {
    const session = await new EchoPtyTransport({ banner: "hi\r\n" }).open({ cols: 80, rows: 24 });
    await session.write(enc("a"));
    const out: string[] = [];
    session.onData((c) => out.push(dec(c)));
    expect(out).toEqual(["hi\r\n", "a"]);
  });

  it("applies a minimal line discipline for Enter and Backspace", () => {
    expect(dec(echo(enc("ls\r")))).toBe("ls\r\n");
    expect(Array.from(echo(Uint8Array.of(0x7f)))).toEqual([0x08, 0x20, 0x08]);
  });

  it("tracks size, acks and closes once", async () => {
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    await session.resize(120, 40);
    expect([session.cols, session.rows]).toEqual([120, 40]);

    session.ack(10);
    session.ack(5);
    expect(session.ackedBytes).toBe(15);

    const exits: Array<number | null> = [];
    session.onExit((e) => exits.push(e.exitCode));
    await session.close();
    await session.close();
    expect(exits).toEqual([0]);
    await expect(session.write(enc("x"))).rejects.toThrow(/closed/);
  });

  it("gives each session its own id", async () => {
    const transport = new EchoPtyTransport();
    const a = await transport.open({ cols: 80, rows: 24 });
    const b = await transport.open({ cols: 80, rows: 24 });
    expect(a.id).not.toBe(b.id);
  });
});

describe("EchoPtyTransport with a simulated shell", () => {
  it("reports no integration and no completions by default", async () => {
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const shells: unknown[] = [];
    session.onShell((info) => shells.push(info));
    expect(shells).toEqual([]);
    expect(await session.complete()).toEqual({ status: "unavailable" });
  });

  it("announces the integration and completes from a fixed list, tracking the typed line", async () => {
    const session = await new EchoPtyTransport({ simulateShell: true }).open({ cols: 80, rows: 24 });
    const shells: unknown[] = [];
    session.onShell((info) => shells.push(info));
    expect(shells).toEqual([{ shell: "echo", protocol: 1 }]);

    await session.write(enc("git chx\x7f"));
    const result = await session.complete();
    expect(result.status).toBe("ok");
    if (result.status !== "ok") return;
    expect([result.line, result.cursor, result.replaceFrom]).toEqual(["git ch", 6, 4]);
    expect(result.items.map((i) => [i.text, i.insert, i.kind])).toEqual([
      ["checkout", "eckout", "shell"],
      ["cherry-pick", "erry-pick", "shell"],
      ["cherry", "erry", "shell"],
      ["git checkout main", "eckout main", "history"],
    ]);

    await session.write(enc("\r"));
    const empty = await session.complete();
    expect(empty.status === "ok" && empty.line).toBe("");
  });
});

describe("EchoPtySession.info", () => {
  it("answers the fixed echo info", async () => {
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    await expect(session.info()).resolves.toEqual({ shell: "echo", cwd: null, busy: false, foreground: null });
  });
});
