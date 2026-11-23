// The PixiJS renderer of the office (task C2, decision E8). Loaded only
// through scene.ts's dynamic import, so PixiJS stays out of the main bundle.
// Placeholder art, all drawn in code (shapes and names) until task C4: no
// image files, so nothing to credit yet.
//
// PixiJS never runs its own loop here: the application starts stopped and
// Ticker.system (used by PixiJS's scheduler and pointer events) is stopped
// and only advanced from our frames, so frames stop when the loop stops.
//
// Layers, bottom to top: the room (floor, corridor, coffee corner), people
// and desks sorted by depth (so a desk covers whoever sits behind it, and a
// passer-by in front covers the desk), names, then bubbles.

import { Application, BitmapText, Container, Graphics, Ticker } from "pixi.js";
// The app's Content-Security-Policy has no 'unsafe-eval' (tauri.conf.json),
// and PixiJS builds shader and uniform code with new Function unless this
// module swaps in its eval-free versions. Without it, every renderer fails
// to start in the app ("does not allow unsafe-eval").
import "pixi.js/unsafe-eval";

import type { SceneFrame, SceneRenderer } from "./controller";
import { DESK_FRONT, SEAT_OFFSET, type OfficeLayout, type Point, type Size } from "./layout";
import { lookFor, type Look } from "./look";
import type { BubbleMark } from "./mapping";
import { POP_MS, type CharacterView } from "./model";

const FONT = "Menlo, Consolas, 'DejaVu Sans Mono', monospace";
const FONT_SIZE = 10;

const COLORS = {
  background: 0x1b1e24,
  floor: 0x2a2e37,
  tile: 0x30343e,
  zoneLine: 0x3a3f4b,
  corridor: 0x22252c,
  corridorLine: 0x3a3f4b,
  desk: 0x8a6a4a,
  deskFront: 0x6b4f35,
  laptop: 0x9aa0aa,
  laptopEdge: 0x5a5f6b,
  door: 0x6b4f35,
  counter: 0x5a5f6b,
  machine: 0x15171b,
  light: 0xd9534f,
  plant: 0x4f9a5a,
  pot: 0x8a5a3b,
  legs: 0x2f3540,
  eyes: 0x15171b,
  crown: 0xe0b340,
  cup: 0xf4f1e8,
  label: 0xc8ccd4,
  bubbleText: 0x15171b,
  bubbleEdge: 0x15171b,
} as const;

/** Bubble background by mark; the glyph says the same thing without color. */
const BUBBLE_FILL: Readonly<Record<BubbleMark, number>> = {
  arrive: 0xf4f1e8,
  tool: 0xf4f1e8,
  message: 0xdbe8f6,
  wait: 0xf6ecc2,
  done: 0xd8eed8,
  error: 0xf4c7c3,
  task: 0xf1e3f6,
};

const BUBBLE_MAX_WIDTH = 160;
const BUBBLE_PAD = 4;
/** Figure height in blocks, from the feet (or seat) to the top of the head. */
const FIGURE_TOP = 16;

const text = (tint: number, wrap = 0): BitmapText => {
  const t = new BitmapText({
    text: "",
    // One white font for every text, tinted: a single glyph atlas.
    style: { fontFamily: FONT, fontSize: FONT_SIZE, fill: 0xffffff, ...(wrap > 0 ? { wordWrap: true, wordWrapWidth: wrap } : {}) },
  });
  t.tint = tint;
  return t;
};

/** One character on screen: its figure, name and bubble. */
class CharacterSprite {
  readonly figure = new Graphics();
  readonly label = text(COLORS.label);
  readonly bubble = new Container();
  private readonly bubbleBack = new Graphics();
  private readonly bubbleText = text(COLORS.bubbleText, BUBBLE_MAX_WIDTH);
  private look: Look;
  private lookKey = "";
  private poseKey = "";
  private labelText = "";
  private bubbleKey = "";

  constructor(view: CharacterView) {
    this.look = lookFor(view.key, view.color);
    this.label.anchor.set(0.5, 0);
    this.bubble.addChild(this.bubbleBack, this.bubbleText);
  }

  update(view: CharacterView, scale: number, size: Size): void {
    const lookKey = `${view.key}|${view.color ?? ""}`;
    if (lookKey !== this.lookKey) {
      this.look = lookFor(view.key, view.color);
      this.lookKey = lookKey;
      this.poseKey = "";
    }
    const u = Math.max(1, Math.round(2 * scale));
    const seated = view.seated;
    // Pose frames come from the model, which also says when they change next.
    const pose: Pose = {
      u,
      seated,
      stepFrame: view.stepFrame,
      typeFrame: view.typeFrame,
      dots: view.dots,
      cup: view.place === "coffee" && !view.walking,
      main: view.main,
      facing: view.facing,
    };
    const poseKey = Object.values(pose).join("|");
    if (poseKey !== this.poseKey) {
      this.poseKey = poseKey;
      drawFigure(this.figure, this.look, pose);
    }

    // Cues: a shake for errors, a little hop for messages and notes.
    let dx = 0;
    let dy = 0;
    if (view.cue === "shake") dx = Math.round(Math.sin(view.cueProgress * Math.PI * 8) * u * (1 - view.cueProgress));
    if (view.cue === "note" || view.cue === "talk") dy = -Math.round(Math.sin(view.cueProgress * Math.PI) * u * 2);
    const x = Math.round(view.x + dx);
    const y = Math.round(view.y + dy);
    this.figure.position.set(x, y);
    this.figure.zIndex = view.y;

    if (view.label !== this.labelText) {
      this.labelText = view.label;
      this.label.text = view.label;
    }
    // Below the desk's front edge when seated, below the feet otherwise.
    const below = seated ? (DESK_FRONT - SEAT_OFFSET) * scale + 2 : u;
    this.label.position.set(Math.round(view.x), Math.round(view.y + below));

    this.updateBubble(view, x, y - FIGURE_TOP * u - (pose.main ? 4 * u : 0) - 3, size);
  }

  private updateBubble(view: CharacterView, x: number, top: number, size: Size): void {
    const bubble = view.bubbleVisible ? view.bubble : null;
    this.bubble.visible = bubble !== null;
    if (!bubble) return;
    const maxWidth = Math.max(40, Math.min(BUBBLE_MAX_WIDTH, size.width - 16));
    const key = `${bubble.mark}|${bubble.text}|${maxWidth}`;
    if (key !== this.bubbleKey) {
      this.bubbleKey = key;
      this.bubbleText.style.wordWrapWidth = maxWidth - 2 * BUBBLE_PAD;
      this.bubbleText.text = `${bubble.glyph} ${bubble.text}`;
      const w = Math.ceil(this.bubbleText.width) + 2 * BUBBLE_PAD;
      const h = Math.ceil(this.bubbleText.height) + 2 * BUBBLE_PAD;
      this.bubbleText.position.set(Math.round(-w / 2 + BUBBLE_PAD), -h - 4 + BUBBLE_PAD);
      const fill = BUBBLE_FILL[bubble.mark];
      // Waiting and errors get a thicker edge too: not color alone.
      const edge = bubble.mark === "wait" || bubble.mark === "error" ? 2 : 1;
      const g = this.bubbleBack.clear();
      g.roundRect(Math.round(-w / 2), -h - 4, w, h, 3).fill(fill).stroke({ width: edge, color: COLORS.bubbleEdge });
      g.poly([-3, -4 - edge / 2, 3, -4 - edge / 2, 0, 0]).fill(fill);
    }
    // Above the head, kept inside the panel.
    const half = this.bubble.width / 2;
    const bx = Math.min(Math.max(x, half + 2), Math.max(half + 2, size.width - half - 2));
    this.bubble.scale.set(Math.min(1, 0.85 + (0.15 * view.bubbleAge) / POP_MS));
    this.bubble.position.set(Math.round(bx), Math.round(Math.max(top, this.bubble.height + 2)));
  }

  destroy(): void {
    this.figure.destroy();
    this.label.destroy();
    this.bubble.destroy({ children: true });
  }
}

interface Pose {
  readonly u: number;
  readonly seated: boolean;
  readonly stepFrame: number;
  readonly typeFrame: number;
  /** Thinking dots shown (0 to 3). */
  readonly dots: number;
  readonly cup: boolean;
  readonly main: boolean;
  readonly facing: -1 | 1;
}

/**
 * A placeholder figure in blocks of `u` pixels, seen from the front, feet at
 * 0,0. Seated, the legs are left out (the desk covers them) and the hands
 * rest on the desk, typing when at work.
 */
function drawFigure(g: Graphics, look: Look, pose: Pose): void {
  const { u } = pose;
  const r = (x: number, y: number, w: number, h: number, color: number) => g.rect(x * u, y * u, w * u, h * u).fill(color);
  g.clear();
  if (!pose.seated) {
    const step = pose.stepFrame;
    r(-3, -4 + step, 2, 4 - step, COLORS.legs);
    r(1, -3 - step, 2, 3 + step, COLORS.legs);
  }
  r(-4, -10, 8, 6, look.shirt);
  if (pose.seated) {
    // Forearms toward the desk, hands alternating while typing.
    r(-6, -10, 2, 4, look.shirt);
    r(4, -10, 2, 4, look.shirt);
    r(-5, -6 + pose.typeFrame * 0.5, 2, 1, look.skin);
    r(3, -5.5 - pose.typeFrame * 0.5, 2, 1, look.skin);
  } else {
    r(-6, -10, 2, 5, look.shirt);
    r(4, -10, 2, 5, look.shirt);
    r(-6, -5, 2, 1, look.skin);
    r(4, -5, 2, 1, look.skin);
    if (pose.cup) r(5, -7, 2, 2, COLORS.cup);
  }
  r(-3, -16, 6, 6, look.skin);
  // Eyes look down at the desk when seated, and where it walks otherwise.
  const eyeX = pose.seated ? 0 : pose.facing < 0 ? -1 : 0;
  const eyeY = pose.seated ? -12 : -13;
  r(-2 + eyeX, eyeY, 1, 1, COLORS.eyes);
  r(1 + eyeX, eyeY, 1, 1, COLORS.eyes);
  if (look.hairStyle !== "bald") r(-3, -16, 6, 2, look.hair);
  if (look.hairStyle === "long") {
    r(-4, -15, 1, 6, look.hair);
    r(3, -15, 1, 6, look.hair);
  }
  if (look.hairStyle === "bun") r(-1, -18, 2, 2, look.hair);
  if (look.hairStyle === "curly") r(-4, -17, 8, 3, look.hair);
  const top = -FIGURE_TOP - (look.hairStyle === "bun" || look.hairStyle === "curly" ? 2 : 0);
  // The main agent wears a small crown, so it is told apart without color.
  if (pose.main) {
    g.poly([-3, top, -3, top - 2.5, -1.5, top - 1.2, 0, top - 3, 1.5, top - 1.2, 3, top - 2.5, 3, top].map((v) => v * u)).fill(COLORS.crown);
  }
  // Thinking: dots that fill one by one, above the head.
  const dotsY = top - (pose.main ? 6 : 3);
  for (let i = 0; i < pose.dots; i++) r(-3.5 + i * 2.5, dotsY, 1.5, 1.5, COLORS.label);
}

/** A desk seen from the front, with the back of a laptop on it. */
function drawDesk(g: Graphics, d: Point, s: number): void {
  const w = Math.round(44 * s);
  const top = Math.round(d.y - 4 * s);
  const front = Math.round(d.y + DESK_FRONT * s);
  const x = Math.round(d.x - w / 2);
  g.clear();
  g.rect(Math.round(d.x + 7 * s), Math.round(d.y - 10 * s), Math.round(13 * s), Math.round(7 * s)).fill(COLORS.laptopEdge);
  g.rect(Math.round(d.x + 8 * s), Math.round(d.y - 9 * s), Math.round(11 * s), Math.round(6 * s)).fill(COLORS.laptop);
  g.rect(x, top, w, Math.round(8 * s)).fill(COLORS.desk);
  g.rect(x, top + Math.round(8 * s), w, front - top - Math.round(8 * s)).fill(COLORS.deskFront);
  g.zIndex = d.y + DESK_FRONT * s;
}

/** The room: floor, corridor with the door, and the coffee corner. */
function drawRoom(g: Graphics, layout: OfficeLayout): void {
  const { width, height } = layout.size;
  g.clear();
  g.rect(0, 0, width, height).fill(COLORS.floor);
  const tile = 16;
  for (let x = tile; x < width; x += tile) g.rect(x, 0, 1, layout.corridor.y).fill(COLORS.tile);
  for (let y = tile; y < layout.corridor.y; y += tile) g.rect(0, y, width, 1).fill(COLORS.tile);
  layout.zones.forEach((zone, i) => {
    if (i === 0) return;
    if (zone.height >= zone.width) g.rect(zone.x, zone.y, zone.width, 1).fill(COLORS.zoneLine);
    else g.rect(zone.x, zone.y, 1, zone.height).fill(COLORS.zoneLine);
  });
  const c = layout.corridor;
  g.rect(c.x, c.y, c.width, c.height).fill(COLORS.corridor);
  g.rect(c.x, c.y, c.width, 1).fill(COLORS.corridorLine);
  // Door on the left wall.
  const doorH = Math.round(c.height * 0.7);
  g.rect(0, Math.round(c.y + (c.height - doorH) / 2), 5, doorH).fill(COLORS.door);
  // Coffee corner: counter along the wall, machine with its light, a plant.
  const k = layout.coffee;
  const counterH = Math.round(Math.min(10, k.height * 0.25));
  g.rect(Math.round(k.x), k.y + 1, Math.round(k.width), counterH).fill(COLORS.counter);
  const mx = Math.round(k.x + k.width * 0.2);
  g.rect(mx, k.y + 1, 10, counterH + 4).fill(COLORS.machine);
  g.rect(mx + 7, k.y + 3, 2, 2).fill(COLORS.light);
  g.rect(Math.round(k.x + k.width - 12), k.y + k.height - 10, 8, 8).fill(COLORS.pot);
  g.circle(Math.round(k.x + k.width - 8), k.y + k.height - 14, 6).fill(COLORS.plant);
}

/** A cheap fingerprint of what the room and the desks depend on. */
function roomKey(layout: OfficeLayout): string {
  return JSON.stringify([layout.size, layout.scale, layout.desks]);
}

// Ticker.system is PixiJS's global ticker. Its autoStart is true, so any
// listener added to it (the renderer's scheduler adds one in init, pointer
// events another) starts a requestAnimationFrame loop that never ends, even
// with the office hidden; stopping it is not enough, since the next listener
// starts it again. So autoStart is turned off while an office renderer
// exists, and Ticker.system only advances inside our frames (draw). The
// office is the only PixiJS user in the app; the previous value comes back
// when the last renderer is destroyed.
let tickerHolds = 0;
let previousAutoStart = true;

function holdSystemTicker(): void {
  if (tickerHolds++ === 0) previousAutoStart = Ticker.system.autoStart;
  Ticker.system.autoStart = false;
  Ticker.system.stop();
}

function releaseSystemTicker(): void {
  if (tickerHolds === 0) return;
  if (--tickerHolds === 0) Ticker.system.autoStart = previousAutoStart;
}

/** A stopped PixiJS application with `preference`; cleaned up if it fails. */
async function startApplication(size: Size, preference: "canvas" | ("webgl" | "canvas")[]): Promise<Application> {
  const app = new Application();
  try {
    await app.init({
      width: Math.max(1, size.width),
      height: Math.max(1, size.height),
      background: COLORS.background,
      antialias: false,
      roundPixels: true,
      resolution: globalThis.devicePixelRatio || 1,
      autoDensity: true,
      autoStart: false,
      sharedTicker: false,
      preference,
      eventFeatures: { move: false, globalMove: false, click: true, wheel: false },
    });
    return app;
  } catch (error) {
    // Whatever init created (a context, listeners) goes too; a failure
    // while cleaning up must not hide the first error.
    try {
      // Only when init got as far as a renderer; before that there is
      // nothing to free and destroy() itself throws.
      if (app.renderer) app.destroy({ removeView: true });
    } catch (cleanup) {
      console.error("[termbay] could not clean up the office canvas", cleanup);
    }
    throw error;
  }
}

export class PixiRenderer implements SceneRenderer {
  private readonly room = new Graphics();
  /** Characters and desks, sorted by depth (zIndex = y). */
  private readonly people = new Container({ sortableChildren: true });
  private readonly labels = new Container();
  private readonly bubbles = new Container({ sortableChildren: true });
  private readonly sprites = new Map<string, CharacterSprite>();
  private desks: Graphics[] = [];
  private roomDrawn = "";
  private destroyed = false;
  private size: Size = { width: 0, height: 0 };

  private constructor(private readonly app: Application) {
    app.stage.addChild(this.room, this.people, this.labels, this.bubbles);
  }

  /** Creates the canvas inside `container`. Rejects if WebGL is not available. */
  static async create(container: HTMLElement, size: Size): Promise<PixiRenderer> {
    holdSystemTicker();
    let app: Application;
    try {
      // WebGL first, Canvas 2D where WebGL is missing (plan B of decision
      // E8). PixiJS only falls back when WebGL is reported missing; a WebGL
      // that is reported but fails to start (software GL, no GPU) throws,
      // so Canvas 2D gets a second, explicit try.
      try {
        app = await startApplication(size, ["webgl", "canvas"]);
      } catch (error) {
        console.warn("[termbay] office: WebGL did not start, drawing with Canvas 2D", error);
        app = await startApplication(size, "canvas");
      }
    } catch (error) {
      releaseSystemTicker();
      throw error;
    }
    app.stop();
    Ticker.system.stop();
    app.canvas.classList.add("office-canvas");
    app.canvas.setAttribute("aria-hidden", "true");
    container.append(app.canvas);
    return new PixiRenderer(app);
  }

  resize(size: Size): void {
    this.size = size;
    this.app.renderer.resize(Math.max(1, size.width), Math.max(1, size.height));
  }

  draw(frame: SceneFrame): void {
    const key = roomKey(frame.layout);
    if (key !== this.roomDrawn) {
      this.roomDrawn = key;
      drawRoom(this.room, frame.layout);
      this.redrawDesks(frame.layout);
    }
    const seen = new Set<string>();
    for (const view of frame.characters) {
      seen.add(view.key);
      let sprite = this.sprites.get(view.key);
      if (!sprite) {
        sprite = new CharacterSprite(view);
        this.sprites.set(view.key, sprite);
        this.people.addChild(sprite.figure);
        this.labels.addChild(sprite.label);
        this.bubbles.addChild(sprite.bubble);
      }
      sprite.update(view, frame.layout.scale, this.size);
      // The newest bubble on top.
      sprite.bubble.zIndex = -view.bubbleAge;
    }
    for (const [key, sprite] of this.sprites) {
      if (!seen.has(key)) {
        sprite.destroy();
        this.sprites.delete(key);
      }
    }
    this.app.render();
    // PixiJS's own upkeep (texture cleanup, pointer checks), only on our frames.
    Ticker.system.update();
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    for (const sprite of this.sprites.values()) sprite.destroy();
    this.sprites.clear();
    for (const desk of this.desks) desk.destroy();
    this.desks = [];
    this.app.destroy({ removeView: true }, { children: true });
    releaseSystemTicker();
  }

  private redrawDesks(layout: OfficeLayout): void {
    const points = layout.desks.flat();
    while (this.desks.length > points.length) this.desks.pop()?.destroy();
    while (this.desks.length < points.length) {
      const desk = new Graphics();
      this.desks.push(desk);
      this.people.addChild(desk);
    }
    points.forEach((d, i) => {
      const desk = this.desks[i];
      if (desk) drawDesk(desk, d, layout.scale);
    });
  }
}
