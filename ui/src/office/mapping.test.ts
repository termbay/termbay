import { describe, expect, it } from "vitest";

import type { OfficeEvent, OfficeKind } from "../transport/office";
import {
  BUBBLE_GLYPHS,
  BUBBLE_MAX_CHARS,
  EVENT_TABLE,
  cleanText,
  displayName,
  mapEvent,
  snapshotBubble,
  type BubbleMark,
} from "./mapping";

const event = (fields: Partial<OfficeEvent>): OfficeEvent => ({ v: 1, session: "s1", agent: "a1", kind: "tool", ...fields });

describe("cleanText", () => {
  it("keeps plain text and collapses whitespace", () => {
    expect(cleanText("  reading\tapi.ts\r\n")).toBe("reading api.ts");
    expect(cleanText("")).toBe("");
    expect(cleanText(undefined)).toBe("");
    expect(cleanText("\n\n")).toBe("");
  });

  it("removes terminal control sequences whole, not just the ESC", () => {
    expect(cleanText("\u001b[31mred\u001b[0m")).toBe("red");
    expect(cleanText("\u001b[1;32;40mbold\u001b[K")).toBe("bold");
    expect(cleanText("a\u009b2Jb")).toBe("a b");
    // OSC: title change, ended by BEL or by ST.
    expect(cleanText("\u001b]0;evil title\u0007ok")).toBe("ok");
    expect(cleanText("\u001b]8;;https://x.test\u001b\\link\u001b]8;;\u001b\\")).toBe("link");
    // DCS and a two-byte sequence.
    expect(cleanText("\u001bPq#0\u001b\\x")).toBe("x");
    expect(cleanText("\u001bcreset")).toBe("reset");
    // A lone ESC at the end.
    expect(cleanText("end\u001b")).toBe("end");
  });

  it("removes control and invisible characters", () => {
    expect(cleanText("a\u0000b\u0007c\u007fd")).toBe("a b c d");
    expect(cleanText("a\u0085b")).toBe("a b");
    // Bidi override would show "txt.exe" as "exe.txt".
    expect(cleanText("file\u202etxt.exe")).toBe("file txt.exe");
    expect(cleanText("zero\u200bwidth\ufeff")).toBe("zero width");
  });

  it("cuts at 80 characters with an ellipsis", () => {
    expect(BUBBLE_MAX_CHARS).toBe(80);
    const exact = "x".repeat(80);
    expect(cleanText(exact)).toBe(exact);
    const cut = cleanText("y".repeat(81));
    expect(Array.from(cut)).toHaveLength(80);
    expect(cut.endsWith("\u2026")).toBe(true);
    expect(cleanText("abcdef", 4)).toBe("abc\u2026");
  });

  it("counts characters, never splitting a surrogate pair", () => {
    const emoji = "\u{1F600}".repeat(100);
    const cut = cleanText(emoji);
    expect(Array.from(cut)).toHaveLength(80);
    expect(cut).toBe(`${"\u{1F600}".repeat(79)}\u2026`);
  });

  it("cuts after removing sequences, so they do not use up the limit", () => {
    const text = `${"\u001b[31m".repeat(50)}${"z".repeat(80)}`;
    expect(cleanText(text)).toBe("z".repeat(80));
  });
});

describe("mapEvent: the whole table", () => {
  it("has one row per kind", () => {
    const kinds: OfficeKind[] = ["start", "tool", "message", "wait", "end", "error", "task"];
    expect(Object.keys(EVENT_TABLE).sort()).toEqual([...kinds].sort());
  });

  it("start: arrives working, keeps its bubble", () => {
    expect(mapEvent(event({ kind: "start", name: "ana", role: "backend" }))).toEqual({
      activity: "working",
      bubble: undefined,
      cue: "arrive",
    });
  });

  it("tool: works, with the action and the target", () => {
    expect(mapEvent(event({ kind: "tool", action: "reading", target: "api.ts" }))).toEqual({
      activity: "working",
      bubble: { mark: "tool", glyph: ">", text: "reading api.ts" },
      cue: "type",
    });
    expect(mapEvent(event({ kind: "tool", action: "report" })).bubble?.text).toBe("report");
    expect(mapEvent(event({ kind: "tool" })).bubble?.text).toBe("working");
  });

  it("message: talks, with the summary or the recipient's name", () => {
    expect(mapEvent(event({ kind: "message", to: "a2", text: "API done" }))).toEqual({
      activity: undefined,
      bubble: { mark: "message", glyph: "@", text: "API done" },
      cue: "talk",
      to: "a2",
    });
    const names = (id: string) => (id === "a2" ? "bruno" : undefined);
    expect(mapEvent(event({ kind: "message", to: "a2" }), names).bubble?.text).toBe("to bruno");
    expect(mapEvent(event({ kind: "message", to: "reviewer" }), names).bubble?.text).toBe("to reviewer");
    const bare = mapEvent(event({ kind: "message" }));
    expect(bare.bubble?.text).toBe("message");
    expect(bare).not.toHaveProperty("to");
  });

  it("wait: waits, with the reason and what for", () => {
    expect(mapEvent(event({ kind: "wait", action: "permission", target: "Bash" }))).toEqual({
      activity: "waiting",
      bubble: { mark: "wait", glyph: "?", text: "permission: Bash" },
      cue: "think",
    });
    expect(mapEvent(event({ kind: "wait", action: "user" })).bubble?.text).toBe("user");
    expect(mapEvent(event({ kind: "wait" })).bubble?.text).toBe("waiting");
  });

  it("end: free again, with the report or no bubble", () => {
    expect(mapEvent(event({ kind: "end", result: "API ready" }))).toEqual({
      activity: "idle",
      bubble: { mark: "done", glyph: "\u2713", text: "API ready" },
      cue: "rest",
    });
    expect(mapEvent(event({ kind: "end" }))).toEqual({ activity: "idle", bubble: null, cue: "rest" });
  });

  it("error: a short error bubble, activity unchanged", () => {
    expect(mapEvent(event({ kind: "error", action: "Bash", result: "failed" }))).toEqual({
      activity: undefined,
      bubble: { mark: "error", glyph: "!", text: "Bash failed" },
      cue: "shake",
    });
    expect(mapEvent(event({ kind: "error", result: "interrupted" })).bubble?.text).toBe("interrupted");
    expect(mapEvent(event({ kind: "error" })).bubble?.text).toBe("failed");
  });

  it("task: a note, activity unchanged", () => {
    expect(mapEvent(event({ kind: "task", action: "created", task: "1", text: "write the tests" }))).toEqual({
      activity: undefined,
      bubble: { mark: "task", glyph: "#", text: "task created: write the tests" },
      cue: "note",
    });
    expect(mapEvent(event({ kind: "task", action: "completed" })).bubble?.text).toBe("task completed");
  });

  it("an unknown kind (newer core) changes nothing", () => {
    expect(mapEvent(event({ kind: "dance" as OfficeKind }))).toEqual({ activity: undefined, bubble: undefined, cue: "rest" });
  });

  it("cleans and cuts every text that reaches a bubble", () => {
    const long = "w".repeat(200);
    const dirty = "\u001b[2J\u001b]0;x\u0007ok\u202e";
    const cases: Partial<OfficeEvent>[] = [
      { kind: "tool", action: "reading", target: long },
      { kind: "message", text: long },
      { kind: "message", to: long },
      { kind: "wait", action: "permission", target: long },
      { kind: "end", result: long },
      { kind: "error", action: long },
      { kind: "task", action: "created", text: long },
    ];
    for (const fields of cases) {
      const text = mapEvent(event(fields)).bubble?.text ?? "";
      expect(Array.from(text).length, fields.kind).toBe(BUBBLE_MAX_CHARS);
      expect(text.endsWith("\u2026"), fields.kind).toBe(true);
    }
    expect(mapEvent(event({ kind: "tool", action: "reading", target: dirty })).bubble?.text).toBe("reading ok");
    expect(mapEvent(event({ kind: "end", result: dirty })).bubble?.text).toBe("ok");
    // Only control characters: no bubble at all.
    expect(mapEvent(event({ kind: "end", result: "\u001b[0m\u0007" })).bubble).toBeNull();
  });

  it("gives every mark a glyph that is not empty", () => {
    for (const mark of Object.keys(BUBBLE_GLYPHS) as BubbleMark[]) expect(BUBBLE_GLYPHS[mark].length).toBeGreaterThan(0);
    expect(new Set(Object.values(BUBBLE_GLYPHS)).size).toBe(Object.keys(BUBBLE_GLYPHS).length);
  });
});

describe("snapshotBubble and displayName", () => {
  it("marks a picture's bubble by the activity", () => {
    expect(snapshotBubble({ id: "main", activity: "working", bubble: "reading api.ts" })).toEqual({
      mark: "tool",
      glyph: ">",
      text: "reading api.ts",
    });
    expect(snapshotBubble({ id: "a1", activity: "waiting", bubble: "permission: Bash" })?.mark).toBe("wait");
    expect(snapshotBubble({ id: "a1", activity: "idle", bubble: "done" })?.mark).toBe("done");
    expect(snapshotBubble({ id: "a1", activity: "idle" })).toBeNull();
  });

  it("names a character by name, then main, role or agent", () => {
    expect(displayName({ id: "a1", name: "ana", role: "backend" })).toBe("ana");
    expect(displayName({ id: "main" })).toBe("main");
    expect(displayName({ id: "a1", role: "reviewer" })).toBe("reviewer");
    expect(displayName({ id: "a1" })).toBe("agent");
    expect(displayName({ id: "a1", name: "\u001b[31m" + "n".repeat(40) })).toHaveLength(24);
  });
});
