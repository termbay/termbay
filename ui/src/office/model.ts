// State of the scene (task C2): who is in the office, where each character
// is and where it is going, what it is doing and saying. Fed by the picture
// and the changes from subscribeOffice; advanced by `step` on each frame.
// Pure: no DOM, no PixiJS, no timers, so it is tested frame by frame.
//
// The activity and the agents follow core::state, so `agents()` can be
// compared with the core's picture. Where a character stands is the scene's
// own business:
// - working or waiting: at its desk;
// - a subagent that finished: lingers at the desk, then goes for coffee
//   ("back to free" in docs/ARQUITETURA.md);
// - the main agent stays at its desk between turns;
// - when its session ends (session_end), when the core drops it (an end with
//   result "evicted"), or when it is gone from a new picture: leaves by the door.

import type { Activity, OfficeEvent, OfficeSnapshot } from "../transport/office";
import { breakSpot, layoutOffice, route, type OfficeLayout, type Point, type Size } from "./layout";
import { displayName, mapEvent, snapshotBubble, type Bubble, type Cue } from "./mapping";

export const MAIN_AGENT = "main";
/** `result` of the `end` of an agent the core dropped (core::state::EVICTED). */
export const EVICTED = "evicted";
/** Walking speed at full scale, in pixels per second. */
export const WALK_SPEED = 90;
/** How long a finished subagent stays at its desk before going for coffee. */
export const LINGER_MS = 2500;
/** How long a cue (a one-shot movement) lasts. */
export const CUE_MS = 700;
/** How long a bubble takes to pop in. */
export const POP_MS = 150;
/** Animation frames: legs while walking, hands while typing, dots while thinking. */
export const STEP_FRAME_MS = 160;
export const TYPE_FRAME_MS = 180;
export const THINK_FRAME_MS = 400;
/**
 * How long a bubble shows. Waiting and error bubbles stay until the next
 * change: they need the user's eyes.
 */
export const BUBBLE_MS = 6000;
/** Bubbles kept per character, for the history on click (later task). */
export const HISTORY = 20;
/** Same limits as core::state. */
export const MAX_SESSIONS = 16;
export const MAX_AGENTS = 64;

export type Place = "desk" | "coffee" | "out";

/** One character, as the renderer reads it. */
export interface CharacterView {
  /** Unique in the scene: session and agent id. */
  readonly key: string;
  readonly session: string;
  readonly id: string;
  readonly label: string;
  readonly role: string | undefined;
  readonly color: string | undefined;
  readonly main: boolean;
  readonly activity: Activity;
  readonly bubble: Bubble | null;
  /** Time since the bubble changed, in ms (for its pop-in). */
  readonly bubbleAge: number;
  /** Whether the bubble is on screen now (see BUBBLE_MS). */
  readonly bubbleVisible: boolean;
  readonly cue: Cue | null;
  /** 0 when the cue starts, 1 when it ends. */
  readonly cueProgress: number;
  readonly x: number;
  readonly y: number;
  readonly place: Place;
  readonly walking: boolean;
  /** Horizontal direction it last walked: -1 left, 1 right. */
  readonly facing: -1 | 1;
  readonly history: readonly Bubble[];
  /** Pose frames, so the renderer draws only what the model says changed. */
  readonly seated: boolean;
  /** Legs while walking (0 or 1). */
  readonly stepFrame: number;
  /** Hands while typing at the desk (0 or 1). */
  readonly typeFrame: number;
  /** Thinking dots while waiting at the desk (0 to 3). */
  readonly dots: number;
}

interface Character {
  key: string;
  session: string;
  id: string;
  name: string | undefined;
  role: string | undefined;
  color: string | undefined;
  activity: Activity;
  bubble: Bubble | null;
  bubbleAge: number;
  cue: Cue | null;
  cueMs: number;
  x: number;
  y: number;
  facing: -1 | 1;
  path: Point[];
  place: Place;
  dest: Point | null;
  lingerMs: number | null;
  /** Gone from the latest picture: walks out, then is removed. */
  leaving: boolean;
  history: Bubble[];
}

interface Session {
  id: string;
  pane: string | undefined;
  agents: Character[];
  /** Ended: everyone walks out, then the session is removed. */
  closing: boolean;
}

/** What the core knows about an agent, to compare with its picture. */
export interface AgentSummary {
  readonly id: string;
  readonly name?: string;
  readonly activity: Activity;
  readonly bubble?: string;
}

export interface SessionSummary {
  readonly session: string;
  readonly agents: readonly AgentSummary[];
}

/** Bubbles that stay until the next change. */
const sticky = (bubble: Bubble) => bubble.mark === "wait" || bubble.mark === "error";

function bubbleVisible(c: Character): boolean {
  return c.bubble !== null && (sticky(c.bubble) || c.bubbleAge < BUBBLE_MS);
}

/**
 * The core's picture in the shape of `OfficeModel.agents()`, to compare the
 * two (the e2e test does): bubbles cleaned as the scene shows them.
 */
export function summarize(snapshot: OfficeSnapshot): SessionSummary[] {
  return snapshot.sessions.slice(0, MAX_SESSIONS).map((s) => ({
    session: s.session,
    agents: s.agents.slice(0, MAX_AGENTS).map((a) => {
      const bubble = snapshotBubble(a);
      return {
        id: a.id,
        ...(a.name !== undefined ? { name: a.name } : {}),
        activity: a.activity,
        ...(bubble ? { bubble: bubble.text } : {}),
      };
    }),
  }));
}

const keyOf =(session: string, agent: string) => `${session}\u0000${agent}`;

export class OfficeModel {
  private sessions: Session[] = [];
  private size: Size;
  /** Scene time: only advances in `step`, so poses freeze while hidden. */
  private time = 0;
  private current: OfficeLayout;

  constructor(size: Size = { width: 320, height: 480 }) {
    this.size = size;
    this.current = layoutOffice(size, []);
  }

  get layout(): OfficeLayout {
    return this.current;
  }

  /** The panel changed size: everyone is placed right where they belong. */
  resize(size: Size): void {
    this.size = size;
    this.retarget(true);
  }

  /** A picture of the office: becomes the truth, without walking in. */
  snapshot(snapshot: OfficeSnapshot): void {
    const seen = new Set<string>();
    const next: Session[] = [];
    for (const view of snapshot.sessions.slice(0, MAX_SESSIONS)) {
      seen.add(view.session);
      const session = this.find(view.session) ?? this.newSession(view.session);
      session.closing = false;
      session.pane = view.pane;
      const ids = new Set<string>();
      const agents: Character[] = [];
      for (const agent of view.agents.slice(0, MAX_AGENTS)) {
        if (ids.has(agent.id)) continue;
        ids.add(agent.id);
        let c = session.agents.find((a) => a.id === agent.id);
        const fresh = !c;
        c ??= this.newCharacter(session.id, agent.id);
        c.leaving = false;
        c.name = agent.name;
        c.role = agent.role;
        c.color = agent.color;
        c.activity = agent.activity;
        c.lingerMs = null;
        this.setBubble(c, snapshotBubble(agent));
        if (fresh) c.dest = null; // placed directly by retarget
        agents.push(c);
      }
      // Gone from the picture: walk out.
      for (const c of session.agents) if (!ids.has(c.id)) agents.push(Object.assign(c, { leaving: true }));
      session.agents = agents;
      next.push(session);
    }
    for (const session of this.sessions) {
      if (!seen.has(session.id)) next.push(Object.assign(session, { closing: true }));
    }
    this.sessions = next;
    this.retarget(false, true);
  }

  /** One change: the character arrives if new, then does what the event says. */
  event(event: OfficeEvent): void {
    // The session is gone from the core (closed by Claude Code or evicted):
    // everyone walks out by the door, then the session is removed.
    if (event.kind === "session_end") {
      const gone = this.find(event.session);
      if (gone) {
        gone.closing = true;
        this.retarget(false);
      }
      return;
    }
    const session = this.find(event.session) ?? this.openSession(event.session);
    session.closing = false;
    if (event.pane) session.pane = event.pane;
    let c = session.agents.find((a) => a.id === event.agent);
    if (!c) {
      c = this.newCharacter(session.id, event.agent);
      c.x = this.current.door.x;
      c.y = this.current.door.y;
      c.dest = { ...this.current.door };
      // Comes in through the door, so it walks the corridor to its desk.
      c.place = "out";
      this.addAgent(session, c);
    }
    c.leaving = false;
    if (event.kind === "start") {
      if (event.name !== undefined) c.name = event.name;
      if (event.role !== undefined) c.role = event.role;
      if (event.color !== undefined) c.color = event.color;
    }
    const effect = mapEvent(event, (id) => {
      const other = session.agents.find((a) => a.id === id);
      return other ? displayName(other) : undefined;
    });
    if (effect.activity) c.activity = effect.activity;
    if (effect.bubble !== undefined) this.setBubble(c, effect.bubble);
    c.cue = effect.cue;
    c.cueMs = 0;
    if (event.kind === "end") {
      // The core dropped this subagent to make room: it leaves at once.
      if (c.id !== MAIN_AGENT && event.result === EVICTED) c.leaving = true;
      // A finished subagent lingers, then goes for coffee; the main agent
      // (Stop, the end of a turn) stays at its desk.
      else if (c.id !== MAIN_AGENT) c.lingerMs = LINGER_MS;
    } else {
      c.lingerMs = null;
    }
    this.retarget(false);
  }

  /** Advances the scene by `dt` ms. */
  step(dt: number): void {
    const ms = Math.max(0, dt);
    this.time += ms;
    let placesChanged = false;
    let removed = false;
    for (const session of this.sessions) {
      for (const c of session.agents) {
        c.bubbleAge += ms;
        if (c.cue) {
          c.cueMs += ms;
          if (c.cueMs >= CUE_MS) c.cue = null;
        }
        if (c.lingerMs !== null) {
          c.lingerMs -= ms;
          if (c.lingerMs <= 0) {
            c.lingerMs = null;
            placesChanged = true;
          }
        }
        this.walk(c, ms);
        if (c.place === "out" && c.path.length === 0) removed = true;
      }
    }
    if (removed) {
      for (const session of this.sessions) session.agents = session.agents.filter((c) => c.place !== "out" || c.path.length > 0);
      this.sessions = this.sessions.filter((s) => !(s.closing && s.agents.length === 0));
    }
    if (placesChanged || removed) this.retarget(false);
  }

  /** Whether anything will change on screen without a new event. */
  busy(): boolean {
    return this.nextChangeIn() < Number.POSITIVE_INFINITY;
  }

  /**
   * In how many ms the picture changes next without a new event: 0 while
   * someone walks, a cue plays or a bubble pops in; otherwise the next typing
   * or thinking frame, the end of a linger or of a bubble; `Infinity` when
   * the office is still. The frame loop sleeps until then, so a character
   * waiting for hours costs 2.5 frames a second, not 30.
   */
  nextChangeIn(): number {
    let next = Number.POSITIVE_INFINITY;
    const until = (period: number) => period - (this.time % period);
    for (const session of this.sessions) {
      for (const c of session.agents) {
        if (c.path.length > 0 || c.cue !== null || (c.bubble && c.bubbleAge < POP_MS)) return 0;
        if (c.lingerMs !== null) next = Math.min(next, c.lingerMs);
        // A bubble still has to go away.
        if (c.bubble && bubbleVisible(c) && !sticky(c.bubble)) next = Math.min(next, BUBBLE_MS - c.bubbleAge);
        if (c.place === "desk") {
          if (c.activity === "working") next = Math.min(next, until(TYPE_FRAME_MS));
          if (c.activity === "waiting") next = Math.min(next, until(THINK_FRAME_MS));
        }
      }
    }
    return Math.max(0, next);
  }

  /** Characters in drawing order (top to bottom, so nearer ones cover farther ones). */
  characters(): CharacterView[] {
    const out: CharacterView[] = [];
    for (const session of this.sessions) {
      for (const c of session.agents) {
        out.push({
          key: c.key,
          session: c.session,
          id: c.id,
          label: displayName(c),
          role: c.role,
          color: c.color,
          main: c.id === MAIN_AGENT,
          activity: c.activity,
          bubble: c.bubble,
          bubbleAge: c.bubbleAge,
          bubbleVisible: bubbleVisible(c),
          cue: c.cue,
          cueProgress: c.cue ? Math.min(1, c.cueMs / CUE_MS) : 1,
          x: c.x,
          y: c.y,
          place: c.place,
          walking: c.path.length > 0,
          facing: c.facing,
          history: c.history,
          seated: c.place === "desk" && c.path.length === 0,
          stepFrame: c.path.length > 0 ? Math.floor(this.time / STEP_FRAME_MS) % 2 : 0,
          typeFrame: c.place === "desk" && c.path.length === 0 && c.activity === "working" ? Math.floor(this.time / TYPE_FRAME_MS) % 2 : 0,
          dots: c.place === "desk" && c.path.length === 0 && c.activity === "waiting" ? Math.floor(this.time / THINK_FRAME_MS) % 4 : 0,
        });
      }
    }
    return out.sort((a, b) => a.y - b.y);
  }

  /** The office as the core sees it (sessions and agents still there). */
  agents(): SessionSummary[] {
    return this.sessions
      .filter((s) => !s.closing)
      .map((s) => ({
        session: s.id,
        agents: s.agents
          .filter((c) => !c.leaving)
          .map((c) => ({
            id: c.id,
            ...(c.name !== undefined ? { name: c.name } : {}),
            activity: c.activity,
            ...(c.bubble ? { bubble: c.bubble.text } : {}),
          })),
      }));
  }

  private find(id: string): Session | undefined {
    return this.sessions.find((s) => s.id === id);
  }

  private newSession(id: string): Session {
    return { id, pane: undefined, agents: [], closing: false };
  }

  private openSession(id: string): Session {
    const open = this.sessions.filter((s) => !s.closing);
    if (open.length >= MAX_SESSIONS && open[0]) open[0].closing = true;
    const session = this.newSession(id);
    this.sessions.push(session);
    return session;
  }

  private newCharacter(session: string, id: string): Character {
    return {
      key: keyOf(session, id),
      session,
      id,
      name: undefined,
      role: undefined,
      color: undefined,
      activity: "working",
      bubble: null,
      bubbleAge: 0,
      cue: null,
      cueMs: 0,
      x: 0,
      y: 0,
      facing: 1,
      path: [],
      place: "desk",
      dest: null,
      lingerMs: null,
      leaving: false,
      history: [],
    };
  }

  /**
   * Main first, then in order of arrival. Over the limit, a character already
   * leaving goes first (the core evicted it and sent its end before this
   * arrival), and only then the oldest one, so the scene keeps the same
   * agents as the core.
   */
  private addAgent(session: Session, c: Character): void {
    if (session.agents.length >= MAX_AGENTS) {
      let i = session.agents.findIndex((a) => a.leaving);
      if (i < 0) i = session.agents.findIndex((a) => a.id !== MAIN_AGENT);
      if (i >= 0) session.agents.splice(i, 1);
    }
    if (c.id === MAIN_AGENT) session.agents.unshift(c);
    else session.agents.push(c);
  }

  private setBubble(c: Character, bubble: Bubble | null): void {
    if (bubble?.text === c.bubble?.text && bubble?.mark === c.bubble?.mark) return;
    c.bubble = bubble;
    c.bubbleAge = 0;
    if (bubble) {
      c.history.push(bubble);
      if (c.history.length > HISTORY) c.history.shift();
    }
  }

  private placeFor(session: Session, c: Character): Place {
    if (session.closing || c.leaving) return "out";
    if (c.id !== MAIN_AGENT && c.activity === "idle" && c.lingerMs === null) return "coffee";
    return "desk";
  }

  /**
   * Recomputes the layout and where everyone should be. `snap` places them
   * there at once (resize); `placeNew` does it only for characters that have
   * no place yet (from a picture).
   */
  private retarget(snap: boolean, placeNew = false): void {
    const open = this.sessions.filter((s) => !s.closing);
    const deskOwners = open.map((s) => s.agents.filter((c) => !c.leaving));
    this.current = layoutOffice(
      this.size,
      deskOwners.map((owners) => owners.length),
    );
    const layout = this.current;
    let coffee = 0;
    for (const session of this.sessions) {
      const zone = open.indexOf(session);
      for (const c of session.agents) {
        const place = this.placeFor(session, c);
        let dest: Point;
        if (place === "out") dest = layout.door;
        else if (place === "coffee") dest = breakSpot(layout, coffee++);
        else dest = layout.seats[zone]?.[deskOwners[zone]?.indexOf(c) ?? 0] ?? layout.door;

        const fresh = c.dest === null;
        if (snap || (placeNew && fresh)) {
          c.x = dest.x;
          c.y = dest.y;
          c.path = [];
        } else if (!c.dest || c.dest.x !== dest.x || c.dest.y !== dest.y || c.place !== place) {
          // A desk that moved because someone arrived: a short direct step.
          const direct = c.place === place && place === "desk" && c.path.length === 0;
          c.path = route(layout, { x: c.x, y: c.y }, dest, direct);
        }
        c.place = place;
        c.dest = { x: dest.x, y: dest.y };
      }
    }
  }

  private walk(c: Character, ms: number): void {
    let budget = (WALK_SPEED * this.current.scale * ms) / 1000;
    while (budget > 0 && c.path.length > 0) {
      const next = c.path[0] as Point;
      const dx = next.x - c.x;
      const dy = next.y - c.y;
      const dist = Math.hypot(dx, dy);
      if (Math.abs(dx) > 0.01) c.facing = dx < 0 ? -1 : 1;
      if (dist <= budget) {
        c.x = next.x;
        c.y = next.y;
        c.path.shift();
        budget -= dist;
      } else {
        c.x += (dx / dist) * budget;
        c.y += (dy / dist) * budget;
        budget = 0;
      }
    }
  }
}
