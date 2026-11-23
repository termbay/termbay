// From office events to what a character does and says (task C1 in
// docs/FASE2-PLANO.md). Pure: no DOM, no PixiJS, so the whole table is tested
// with Vitest. The scene (scene.ts) only reads what this module returns.
//
// The activity follows core::state (decision E5), so a scene built from
// events agrees with the next picture of the office. The bubble text follows
// the same rules as the core's bubbles, and is cleaned again here: the page
// never trusts a text to be short or free of control sequences.

import type { Activity, AgentView, OfficeEvent, OfficeKind } from "../transport/office";

/** Longest text a bubble may show, in characters (decision E10). */
export const BUBBLE_MAX_CHARS = 80;

/**
 * What kind of bubble it is. Each mark has its own glyph, so the bubble never
 * relies on color alone to say what is going on.
 */
export type BubbleMark = "arrive" | "tool" | "message" | "wait" | "done" | "error" | "task";

export const BUBBLE_GLYPHS: Readonly<Record<BubbleMark, string>> = {
  arrive: "+",
  tool: ">",
  message: "@",
  wait: "?",
  done: "\u2713", // check mark
  error: "!",
  task: "#",
};

export interface Bubble {
  readonly mark: BubbleMark;
  readonly glyph: string;
  /** Clean text, at most BUBBLE_MAX_CHARS characters. */
  readonly text: string;
}

/**
 * A short, one-shot movement that goes with the event, on top of the pose
 * that follows from the activity.
 */
export type Cue = "arrive" | "type" | "talk" | "think" | "rest" | "shake" | "note";

/** What an event does to the character it belongs to. */
export interface EventEffect {
  /** The activity after the event; `undefined` keeps the current one. */
  readonly activity: Activity | undefined;
  /** The bubble after the event; `undefined` keeps it, `null` clears it. */
  readonly bubble: Bubble | null | undefined;
  readonly cue: Cue;
  /** Recipient of a message (agent id, or the name as written). */
  readonly to?: string;
}

// ESC-introduced sequences: CSI (ESC [ ... final), OSC/DCS/SOS/PM/APC
// (ESC ] ... BEL or ST), and the two-byte ones (ESC + one character).
// Then the same with the single-byte C1 introducers (CSI 0x9b, OSC 0x9d...).
const CSI = /(?:\u001b\[|\u009b)[0-?]*[ -/]*[@-~]?/g;
const STRING_SEQ = /(?:\u001b[\]PX^_]|[\u0090\u0098\u009d\u009e\u009f])[^\u0007\u001b\u009c]*(?:\u0007|\u001b\\|\u009c)?/g;
const TWO_BYTE = /\u001b[\s\S]?/g;
// Remaining C0 and C1 controls, DEL, and the line and paragraph separators:
// each becomes a space.
const CONTROL = /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/g;
// Invisible characters that can hide, fake or reorder text: soft hyphen,
// Arabic letter mark, Hangul fillers, zero-width space and non-joiner, LRM
// and RLM, bidi embeddings, overrides and isolates, word joiner and invisible
// operators, byte order mark, interlinear annotation and the tag block. They
// are dropped. The zero-width joiner (U+200D) stays: emoji sequences such as
// families need it.
const INVISIBLE =
  /[\u00ad\u061c\u115f\u3164\u200b\u200c\u200e\u200f\u202a-\u202e\u2060-\u2069\ufeff\ufff9-\ufffb\u{e0000}-\u{e007f}]/gu;
/** Combining marks kept after one character; more is "Zalgo" text. */
export const MAX_COMBINING = 2;
const ZALGO = new RegExp(`(\\p{M}{${MAX_COMBINING}})\\p{M}+`, "gu");

/**
 * Bubble-safe text: removes terminal control sequences and control or
 * invisible characters, keeps at most MAX_COMBINING combining marks in a
 * row, collapses whitespace, trims, and cuts to `max`
 * characters (code points, never half of a pair), ending with `…` when cut.
 */
export function cleanText(text: string | undefined | null, max = BUBBLE_MAX_CHARS): string {
  if (!text || max <= 0) return "";
  const plain = text
    .replace(STRING_SEQ, " ")
    .replace(CSI, " ")
    .replace(TWO_BYTE, " ")
    .replace(CONTROL, " ")
    .replace(INVISIBLE, "")
    .replace(ZALGO, "$1")
    .replace(/\s+/g, " ")
    .trim();
  const chars = Array.from(plain);
  if (chars.length <= max) return plain;
  return `${chars
    .slice(0, max - 1)
    .join("")
    .trimEnd()}\u2026`;
}

/** A bubble, or `null` when the text is empty after cleaning. */
export function bubble(mark: BubbleMark, text: string | undefined): Bubble | null {
  const clean = cleanText(text);
  return clean ? { mark, glyph: BUBBLE_GLYPHS[mark], text: clean } : null;
}

/** Joins the present parts with `sep`, skipping the empty ones. */
function join(sep: string, ...parts: (string | undefined)[]): string {
  return parts.filter((p): p is string => typeof p === "string" && p.trim().length > 0).join(sep);
}

/** Looks up a readable name for an agent id; returns `undefined` if unknown. */
export type NameOf = (agent: string) => string | undefined;

const noNames: NameOf = () => undefined;

type Mapper = (event: OfficeEvent, nameOf: NameOf) => EventEffect;

/**
 * The table: one row per kind. Each row says the activity after the event,
 * the bubble and the cue. `satisfies` makes a new kind a compile error until
 * it has a row.
 */
export const EVENT_TABLE = {
  // Arrives (or comes back) and sits at the desk. A new agent has no bubble
  // yet; a known one coming back keeps its last one, as in the core.
  start: () => ({ activity: "working", bubble: undefined, cue: "arrive" }),
  // "reading api.ts", "running tests".
  tool: (e) => ({
    activity: "working",
    bubble: bubble("tool", join(" ", e.action ?? "working", e.target)),
    cue: "type",
  }),
  // A message (or a call to a new agent): says the summary, or who it is for.
  message: (e, nameOf) => {
    const toName = e.to ? (nameOf(e.to) ?? e.to) : undefined;
    const text = e.text ?? (toName ? `to ${toName}` : "message");
    return { activity: undefined, bubble: bubble("message", text), cue: "talk", ...(e.to ? { to: e.to } : {}) };
  },
  // Waiting for permission, for the user, or idle in a team.
  wait: (e) => ({
    activity: "waiting",
    bubble: bubble("wait", join(": ", e.action ?? "waiting", e.target)),
    cue: "think",
  }),
  // Back to free: shows what it reported, or nothing.
  end: (e) => ({ activity: "idle", bubble: e.result ? bubble("done", e.result) : null, cue: "rest" }),
  // A short error bubble; the activity does not change (as in the core).
  error: (e) => ({
    activity: undefined,
    bubble: bubble("error", join(" ", e.action, e.result ?? "failed")),
    cue: "shake",
  }),
  // A note on the task board: "task created: write the tests".
  task: (e) => ({
    activity: undefined,
    bubble: bubble("task", join(": ", join(" ", "task", e.action), e.text)),
    cue: "note",
  }),
} as const satisfies Record<OfficeKind, Mapper>;

/** What `event` does to its character. Unknown kinds change nothing. */
export function mapEvent(event: OfficeEvent, nameOf: NameOf = noNames): EventEffect {
  const row = (EVENT_TABLE as Partial<Record<string, Mapper>>)[event.kind];
  return row ? row(event, nameOf) : { activity: undefined, bubble: undefined, cue: "rest" };
}

/** The mark that goes with an activity, for bubbles that come in a picture. */
const ACTIVITY_MARK: Readonly<Record<Activity, BubbleMark>> = { working: "tool", waiting: "wait", idle: "done" };

/** The bubble of an agent in a picture of the office. */
export function snapshotBubble(agent: AgentView): Bubble | null {
  return bubble(ACTIVITY_MARK[agent.activity] ?? "tool", agent.bubble);
}

/** The name shown under a character. */
export function displayName(agent: {
  readonly id: string;
  readonly name?: string | undefined;
  readonly role?: string | undefined;
}): string {
  return cleanText(agent.name ?? (agent.id === "main" ? "main" : (agent.role ?? "agent")), 24);
}
