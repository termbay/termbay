/** Maximum title length, in code points. */
export const MAX_TITLE_LENGTH = 80;

// C0, DEL, C1, and bidirectional controls (ALM, LRM, RLM, embeddings,
// overrides and isolates), which could reorder what the user reads.
const CONTROLS = /[\u0000-\u001f\u007f-\u009f؜‎‏‪-‮⁦-⁩]/g;

/**
 * Cleans a title that came from the terminal output (OSC 0/2, OSC 7): no
 * control or bidi characters, at most 80 code points. Empty result: null.
 * The result must still only reach the DOM through `textContent`.
 */
export function sanitizeTitle(raw: string): string | null {
  const clean = Array.from(raw.replace(CONTROLS, "").trim()).slice(0, MAX_TITLE_LENGTH).join("").trim();
  return clean.length > 0 ? clean : null;
}

/**
 * Label for an OSC 7 payload (`file://host/path`): the last path component.
 * Display only: never used to open a pty (D23), since any output can forge it.
 */
export function cwdLabelFromOsc7(data: string): string | null {
  const match = /^file:\/\/[^/]*(\/.*)?$/i.exec(data.trim());
  if (!match) return null;
  let path = match[1] ?? "/";
  try {
    path = decodeURIComponent(path);
  } catch {
    // Keep the raw path when the escapes are malformed.
  }
  const parts = path.split(/[/\\]/).filter(Boolean);
  return sanitizeTitle(parts.at(-1) ?? "/");
}
