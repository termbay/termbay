import { describe, expect, it } from "vitest";
import type { CompletionItem, CompletionResult } from "../transport/types";
import { AutocompleteController, type AutocompleteKeyEvent } from "./controller";

/**
 * Hostile answers (AC8, docs/SEGURANCA.md): even if the core relayed a forged
 * or buggy answer, accepting a suggestion only types text. No CR/LF (nothing
 * runs), no ESC outside the bracketed paste markers (nothing ends the paste
 * early), and no more DELs than the word being completed has characters.
 */

const dec = (b: Uint8Array) => new TextDecoder().decode(b);
const tick = () => new Promise((r) => setTimeout(r, 0));

function key(k: string): AutocompleteKeyEvent {
  return {
    type: "keydown",
    key: k,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    preventDefault() {},
  };
}

const HOSTILE = "evil\r\nrm -rf ~\r\x1b[201~\x1b]0;x\x07\x9b2J\x00";

function hostile(items: CompletionItem[]): CompletionResult {
  return { status: "ok", line: "git ch", cursor: 6, replaceFrom: 4, items };
}

async function run(result: CompletionResult, bracketed: boolean, keys: string[]) {
  const writes: string[] = [];
  const controller = new AutocompleteController(
    { isAlternateBuffer: () => false, bracketedPasteMode: () => bracketed },
    {
      complete: async () => result,
      write: async (data) => {
        writes.push(dec(data));
      },
    },
    { show: () => {}, hide: () => {} },
  );
  controller.handleKey(key("Tab"));
  await tick();
  for (const k of keys) controller.handleKey(key(k));
  return writes.join("");
}

function expectOnlyText(sent: string, bracketed: boolean, maxDeletes: number) {
  const body = bracketed ? sent.replace("\x1b[200~", "").replace("\x1b[201~", "") : sent;
  expect(body).not.toMatch(/[\r\n\x1b\x00\x9b]/);
  const deletes = /^\x7f*/.exec(body)?.[0].length ?? 0;
  expect(deletes).toBeLessThanOrEqual(maxDeletes);
  expect(body.slice(deletes)).not.toMatch(/\x7f/);
}

describe("hostile completion answers", () => {
  for (const bracketed of [false, true]) {
    it(`a single candidate is typed as plain text (bracketed paste ${bracketed ? "on" : "off"})`, async () => {
      const sent = await run(hostile([{ text: HOSTILE, insert: `\x7f`.repeat(500) + HOSTILE, kind: "shell" }]), bracketed, []);
      expectOnlyText(sent, bracketed, 2);
      expect(sent).toContain("rm -rf ~");
    });

    it(`Enter on a hostile menu entry never sends CR (bracketed paste ${bracketed ? "on" : "off"})`, async () => {
      const sent = await run(
        hostile([
          { text: "checkout", insert: "eckout", kind: "shell" },
          { text: HOSTILE, insert: `eckout${HOSTILE}`, kind: "history" },
        ]),
        bracketed,
        ["ArrowDown", "Enter"],
      );
      expectOnlyText(sent, bracketed, 0);
    });
  }
});
