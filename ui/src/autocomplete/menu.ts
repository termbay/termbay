import "./menu.css";

import type { MenuEntry, MenuSink, MenuState } from "./controller";

/**
 * The autocomplete listbox, drawn in DOM over one terminal pane (outside the
 * WebGL canvas) and anchored to the cursor cell. Focus stays in the xterm.js
 * textarea; the listbox is announced through `aria-activedescendant`.
 */

export interface PositionInput {
  /** Column where the menu is anchored (start of the word, in cells). */
  readonly col: number;
  /** Row of the cursor inside the visible viewport (0 = top row). */
  readonly row: number;
  readonly rows: number;
  readonly cellWidth: number;
  readonly cellHeight: number;
  /** Offset of the xterm screen inside the overlay container, in px. */
  readonly screenLeft: number;
  readonly screenTop: number;
  /** Size of the overlay container (the pane), in px. */
  readonly areaWidth: number;
  readonly areaHeight: number;
  /** Natural size of the menu, in px. */
  readonly menuWidth: number;
  readonly menuHeight: number;
}

export interface MenuPosition {
  readonly left: number;
  readonly top: number;
  /** "below" the cursor row, or "above" it near the bottom edge. */
  readonly placement: "below" | "above";
  readonly maxHeight: number;
  readonly maxWidth: number;
}

/**
 * Where to draw the menu: under the cursor row, or above it when it does not
 * fit below and there is more room above; shifted left to stay inside the pane.
 * Null when the cursor row is not visible (scrolled out).
 */
export function computeMenuPosition(p: PositionInput): MenuPosition | null {
  if (p.row < 0 || p.row >= p.rows || p.cellWidth <= 0 || p.cellHeight <= 0) return null;
  const rowTop = p.screenTop + p.row * p.cellHeight;
  const rowBottom = rowTop + p.cellHeight;
  const spaceBelow = Math.max(0, p.areaHeight - rowBottom);
  const spaceAbove = Math.max(0, rowTop);

  const fitsBelow = p.menuHeight <= spaceBelow;
  const placement = fitsBelow || spaceBelow >= spaceAbove ? "below" : "above";
  const maxHeight = placement === "below" ? spaceBelow : spaceAbove;
  const height = Math.min(p.menuHeight, maxHeight);
  const top = placement === "below" ? rowBottom : rowTop - height;

  const maxWidth = Math.max(0, p.areaWidth);
  const width = Math.min(p.menuWidth, maxWidth);
  const anchor = p.screenLeft + Math.max(0, p.col) * p.cellWidth;
  const left = Math.max(0, Math.min(anchor, p.areaWidth - width));
  return { left, top, placement, maxHeight, maxWidth };
}

/** The cursor as xterm.js reports it, and the viewport. */
export interface CursorInfo {
  readonly cursorX: number;
  /** Relative to `baseY` (xterm.js `buffer.active.cursorY`). */
  readonly cursorY: number;
  readonly baseY: number;
  readonly viewportY: number;
  readonly cols: number;
  readonly rows: number;
}

/**
 * Cell where the menu is anchored: the start of the word being completed when
 * it is on the cursor row, otherwise the cursor itself; the row is relative to
 * what is on screen (the user may have scrolled up).
 */
export function anchorCell(cursor: CursorInfo, wordLength: number): { col: number; row: number } {
  const col = cursor.cursorX >= wordLength ? cursor.cursorX - wordLength : cursor.cursorX;
  const row = cursor.baseY + cursor.cursorY - cursor.viewportY;
  return { col: Math.min(col, Math.max(0, cursor.cols - 1)), row };
}

export interface MenuGroup {
  readonly kind: "shell" | "history";
  readonly label: string;
  /** Index of each entry in the flat list (what the controller's `selected` points to). */
  readonly entries: ReadonlyArray<{ readonly index: number; readonly entry: MenuEntry }>;
}

/** Splits the flat list into the shell and history sections, keeping indices. */
export function groupEntries(entries: readonly MenuEntry[]): MenuGroup[] {
  const shell: Array<{ index: number; entry: MenuEntry }> = [];
  const history: Array<{ index: number; entry: MenuEntry }> = [];
  entries.forEach((entry, index) => (entry.kind === "history" ? history : shell).push({ index, entry }));
  const groups: MenuGroup[] = [];
  if (shell.length > 0) groups.push({ kind: "shell", label: "Completions", entries: shell });
  if (history.length > 0) groups.push({ kind: "history", label: "History", entries: history });
  return groups;
}

/** The parts of the xterm.js `Terminal` the menu reads. */
export interface MenuTerminal {
  readonly cols: number;
  readonly rows: number;
  readonly element: HTMLElement | undefined;
  readonly textarea: HTMLTextAreaElement | undefined;
  readonly buffer: { readonly active: { cursorX: number; cursorY: number; baseY: number; viewportY: number } };
  readonly options: { fontFamily?: string | undefined; fontSize?: number | undefined };
}

let nextMenuId = 1;

/** One listbox per pane. Created hidden; costs nothing until shown. */
export class CompletionMenu implements MenuSink {
  private readonly root: HTMLDivElement;
  private readonly idPrefix = `termbay-ac-${nextMenuId++}`;
  private state: MenuState | null = null;
  private options: HTMLElement[] = [];

  constructor(
    private readonly container: HTMLElement,
    private readonly term: MenuTerminal,
    private readonly onPick: (index: number) => void,
  ) {
    const doc = container.ownerDocument;
    if (doc.defaultView?.getComputedStyle(container).position === "static") container.style.position = "relative";
    this.root = doc.createElement("div");
    this.root.id = `${this.idPrefix}-list`;
    this.root.className = "termbay-autocomplete";
    this.root.setAttribute("role", "listbox");
    this.root.setAttribute("aria-label", "Suggestions");
    this.root.hidden = true;
    // Keep the focus in the terminal: picking with the mouse must not blur it.
    this.root.addEventListener("mousedown", (ev) => {
      ev.preventDefault();
      const option = (ev.target as HTMLElement | null)?.closest<HTMLElement>("[data-index]");
      if (option) this.onPick(Number(option.dataset.index));
    });
    container.appendChild(this.root);
  }

  show(state: MenuState): void {
    const sameList = this.state?.entries === state.entries;
    this.state = state;
    if (!sameList) this.renderList(state.entries);
    this.renderSelection(state.selected);
    this.root.hidden = false;
    this.reposition();
    const textarea = this.term.textarea;
    textarea?.setAttribute("aria-controls", this.root.id);
    textarea?.setAttribute("aria-expanded", "true");
  }

  hide(): void {
    if (!this.state) return;
    this.state = null;
    this.root.hidden = true;
    this.root.replaceChildren();
    this.options = [];
    const textarea = this.term.textarea;
    textarea?.removeAttribute("aria-activedescendant");
    textarea?.removeAttribute("aria-controls");
    textarea?.setAttribute("aria-expanded", "false");
  }

  /** Recomputes the position (cursor moved, scroll, resize). Hides it while the cursor is off screen. */
  reposition(): void {
    if (!this.state) return;
    const screen = this.term.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!screen) return;
    const screenRect = screen.getBoundingClientRect();
    const areaRect = this.container.getBoundingClientRect();
    const { buffer, cols, rows } = this.term;
    const active = buffer.active;
    const cell = anchorCell(
      { cursorX: active.cursorX, cursorY: active.cursorY, baseY: active.baseY, viewportY: active.viewportY, cols, rows },
      this.state.wordLength,
    );

    // Natural size first, then the limits of the chosen placement.
    const style = this.root.style;
    style.maxHeight = "";
    style.maxWidth = "";
    const position = computeMenuPosition({
      ...cell,
      rows,
      // The public API does not expose the cell size; the screen is cols x rows cells.
      cellWidth: screenRect.width / cols,
      cellHeight: screenRect.height / rows,
      screenLeft: screenRect.left - areaRect.left,
      screenTop: screenRect.top - areaRect.top,
      areaWidth: this.container.clientWidth,
      areaHeight: this.container.clientHeight,
      menuWidth: this.root.offsetWidth,
      menuHeight: this.root.offsetHeight,
    });
    if (!position) {
      style.visibility = "hidden";
      return;
    }
    style.visibility = "";
    style.left = `${position.left}px`;
    style.top = `${position.top}px`;
    style.maxHeight = `${position.maxHeight}px`;
    style.maxWidth = `${position.maxWidth}px`;
    this.root.dataset.placement = position.placement;
  }

  dispose(): void {
    this.hide();
    this.root.remove();
    this.term.textarea?.removeAttribute("aria-expanded");
  }

  private renderList(entries: readonly MenuEntry[]): void {
    const doc = this.container.ownerDocument;
    const { fontFamily, fontSize } = this.term.options;
    if (fontFamily) this.root.style.fontFamily = fontFamily;
    if (fontSize) this.root.style.fontSize = `${fontSize}px`;

    this.options = new Array<HTMLElement>(entries.length);
    const groups = groupEntries(entries);
    const children: HTMLElement[] = [];
    for (const group of groups) {
      const el = doc.createElement("div");
      el.setAttribute("role", "group");
      el.className = `termbay-autocomplete-group termbay-autocomplete-${group.kind}`;
      if (groups.length > 1) {
        const heading = doc.createElement("div");
        heading.id = `${this.idPrefix}-${group.kind}`;
        heading.className = "termbay-autocomplete-heading";
        heading.setAttribute("role", "presentation");
        heading.textContent = group.label;
        el.setAttribute("aria-labelledby", heading.id);
        el.appendChild(heading);
      } else {
        el.setAttribute("aria-label", group.label);
      }
      for (const { index, entry } of group.entries) {
        const option = doc.createElement("div");
        option.id = `${this.idPrefix}-${index}`;
        option.className = "termbay-autocomplete-option";
        option.setAttribute("role", "option");
        option.dataset.index = String(index);
        const text = doc.createElement("span");
        text.className = "termbay-autocomplete-text";
        text.textContent = entry.text;
        option.appendChild(text);
        if (entry.description) {
          const description = doc.createElement("span");
          description.className = "termbay-autocomplete-description";
          description.textContent = entry.description;
          option.appendChild(description);
        }
        el.appendChild(option);
        this.options[index] = option;
      }
      children.push(el);
    }
    this.root.replaceChildren(...children);
  }

  private renderSelection(selected: number): void {
    this.options.forEach((option, index) => option.setAttribute("aria-selected", String(index === selected)));
    const current = this.options[selected];
    if (!current) return;
    current.scrollIntoView({ block: "nearest" });
    this.term.textarea?.setAttribute("aria-activedescendant", current.id);
  }
}
