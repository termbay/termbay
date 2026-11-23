// How a placeholder character looks (task C2, until the art of task C4):
// skin, hair and shirt picked from the agent's key, so the same agent always
// looks the same and a team looks varied. Pure, drawn by render.ts.

export interface Look {
  readonly skin: number;
  readonly hair: number;
  readonly hairStyle: HairStyle;
  readonly shirt: number;
}

export type HairStyle = "short" | "long" | "bun" | "curly" | "bald";

/** A wide range of skin tones, light to dark. */
export const SKIN_TONES = [0xf6d7c3, 0xe8b896, 0xd19a6e, 0xa86f48, 0x7d4b2a, 0x4f2f1c] as const;
export const HAIR_COLORS = [0x1d1a17, 0x4a2c1a, 0x8a5a2b, 0xd8b46a, 0xb24a2a, 0x9a9a9a] as const;
export const HAIR_STYLES: readonly HairStyle[] = ["short", "long", "bun", "curly", "bald"];
export const SHIRT_COLORS = [0x4f7cac, 0x5a9e6f, 0xc1666b, 0xd4a24c, 0x8a6bb8, 0x3f9a9a, 0xd47fa6, 0x7a8a99] as const;

/** Colors Claude Code gives teammates, by name. */
export const NAMED_COLORS: Readonly<Record<string, number>> = {
  red: 0xc94f4f,
  orange: 0xd9853b,
  yellow: 0xd9c03b,
  green: 0x4fa65a,
  blue: 0x4f7cc9,
  purple: 0x8a5cc9,
  pink: 0xd46fa6,
  cyan: 0x3fb0b8,
};

/** FNV-1a: small, stable, good enough to spread keys over palettes. */
export function hash(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

function pick<T>(list: readonly T[], seed: number): T {
  return list[seed % list.length] as T;
}

/** The look for an agent; `color` (from a team) wins for the shirt. */
export function lookFor(key: string, color?: string): Look {
  const h = hash(key);
  const named = color ? NAMED_COLORS[color.toLowerCase()] : undefined;
  return {
    skin: pick(SKIN_TONES, h),
    hair: pick(HAIR_COLORS, h >>> 3),
    hairStyle: pick(HAIR_STYLES, h >>> 6),
    shirt: named ?? pick(SHIRT_COLORS, h >>> 9),
  };
}
