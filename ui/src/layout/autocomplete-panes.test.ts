// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { EchoPtySession, EchoPtyTransport } from "../transport/echo";
import { TerminalView } from "../terminal/view";

const settle = () => new Promise((r) => setTimeout(r, 20));
const transport = new EchoPtyTransport({ simulateShell: true });

async function pane(trigger?: "tab" | "ctrl+space") {
  const el = document.createElement("div");
  document.body.append(el);
  const view = new TerminalView(el, {}, () => {}, "linux");
  const session = (await transport.open({ cols: 80, rows: 24 })) as EchoPtySession;
  const complete = vi.spyOn(session, "complete");
  const written: string[] = [];
  const write = session.write.bind(session);
  session.write = (data) => {
    written.push(new TextDecoder().decode(data));
    return write(data);
  };
  const attached = view.attach(session, trigger ? { trigger } : {});
  const menu = () => el.querySelector<HTMLElement>(".termbay-autocomplete");
  return { el, view, session, complete, written, attached, menu };
}

function keydown(view: TerminalView, key: string, init: KeyboardEventInit = {}) {
  view.term.textarea?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
}

describe("autocomplete with several panes (B6)", () => {
  it("Tab in pane A asks only A and opens only A's menu; moving the focus closes it", async () => {
    const a = await pane();
    const b = await pane();
    a.view.focus();
    keydown(a.view, "Tab", { code: "Tab", keyCode: 9 });
    await settle();
    expect(a.complete).toHaveBeenCalledOnce();
    expect(b.complete).not.toHaveBeenCalled();
    expect(a.menu()?.hidden).toBe(false);
    expect(b.menu()?.hidden ?? true).toBe(true);

    b.view.focus(); // blur of A's textarea
    a.view.term.textarea?.dispatchEvent(new FocusEvent("blur"));
    expect(a.menu()?.hidden).toBe(true);
    a.attached.dispose();
    b.attached.dispose();
  });

  it("disposing a pane's attachment removes its menu and key handler before close", async () => {
    const a = await pane();
    a.view.focus();
    keydown(a.view, "Tab", { code: "Tab", keyCode: 9 });
    await settle();
    a.attached.dispose();
    expect(a.menu()).toBeNull();
    await a.session.close();
    keydown(a.view, "Tab", { code: "Tab", keyCode: 9 });
    await settle();
    expect(a.complete).toHaveBeenCalledOnce();
  });

  it('trigger "ctrl+space" lets Tab go to the shell', async () => {
    const a = await pane("ctrl+space");
    a.view.focus();
    keydown(a.view, "Tab", { code: "Tab", keyCode: 9 });
    await settle();
    expect(a.complete).not.toHaveBeenCalled();
    expect(a.written).toContain("\t");
    keydown(a.view, " ", { code: "Space", keyCode: 32, ctrlKey: true });
    await settle();
    expect(a.complete).toHaveBeenCalledOnce();
    a.attached.dispose();
  });
});

describe("TerminalView pane hooks", () => {
  it("reports sanitized OSC 0 titles and OSC 7 labels, and after exit only Enter acts", async () => {
    const a = await pane();
    const titles: Array<string | null> = [];
    const labels: Array<string | null> = [];
    a.view.onTitle((t) => titles.push(t));
    a.view.onCwdLabel((l) => labels.push(l));
    await new Promise<void>((r) => a.view.term.write("\x1b]0;vim ‮main\x07\x1b]7;file://h/tmp/evil%20repo\x07", r));
    expect(titles).toEqual(["vim main"]);
    expect(labels).toEqual(["evil repo"]);

    const onEnter = vi.fn();
    a.view.setExited(onEnter);
    a.written.length = 0;
    keydown(a.view, "x", { code: "KeyX", keyCode: 88 });
    keydown(a.view, "Enter", { code: "Enter", keyCode: 13 });
    expect(onEnter).toHaveBeenCalledOnce();
    expect(a.written).toEqual([]);
    a.attached.dispose();
  });
});
