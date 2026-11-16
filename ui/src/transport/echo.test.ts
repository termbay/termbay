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
