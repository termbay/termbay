import { describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { EchoPtyTransport } from "../transport/echo";
import { attachAutocomplete, AutocompleteSession } from "./attach";
import type { AutocompleteController } from "./controller";

const enc = (s: string) => new TextEncoder().encode(s);

describe("attachAutocomplete", () => {
  it("registers no key handler and creates nothing while the shell integration is off", async () => {
    // Without `simulateShell`, the echo session never fires onShell, like a
    // pty opened with `[autocomplete] enabled = false`.
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const setKeyHandler = vi.fn();
    const host = { term: {} as Terminal, container: {} as HTMLElement, setKeyHandler };
    const attached = attachAutocomplete(host, session);

    const out: Uint8Array[] = [];
    session.onData((c) => out.push(c));
    await attached.session.write(enc("\t"));

    expect(setKeyHandler).not.toHaveBeenCalled();
    expect(out).toEqual([enc("\t")]);
    attached.dispose();
  });

  it("ignores an integration protocol it does not understand", async () => {
    const session = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    vi.spyOn(session, "onShell").mockImplementation((listener) => {
      listener({ shell: "zsh", protocol: 2 });
      return { dispose: () => {} };
    });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const setKeyHandler = vi.fn();
    attachAutocomplete({ term: {} as Terminal, container: {} as HTMLElement, setKeyHandler }, session);
    expect(setKeyHandler).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });
});

describe("AutocompleteSession", () => {
  it("routes writes through the controller once there is one", async () => {
    const inner = await new EchoPtyTransport().open({ cols: 80, rows: 24 });
    const gated = new AutocompleteSession(inner);
    const input = vi.fn();
    gated.controller = { input } as unknown as AutocompleteController;
    await gated.write(enc("a"));
    expect(input).toHaveBeenCalledWith(enc("a"));
    expect(gated.id).toBe(inner.id);
  });
});
