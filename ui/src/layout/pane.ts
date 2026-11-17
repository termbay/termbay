import type { Trigger } from "../autocomplete/controller";
import type { Disposable, PtySession } from "../transport/types";

/**
 * What the layout needs from a pane's terminal. `TerminalView` implements it;
 * tests use fakes. The element is created once and only ever moved.
 */
export interface PaneView extends Disposable {
  readonly element: HTMLElement;
  readonly cols: number;
  readonly rows: number;
  attach(session: PtySession, options: { trigger?: Trigger }): Disposable;
  focus(): void;
  /** Hidden tab: drop the WebGL context; visible again: reload it. */
  setVisible(visible: boolean): void;
  onTitle(listener: (title: string | null) => void): Disposable;
  onCwdLabel(listener: (label: string | null) => void): Disposable;
  write(text: string): void;
  /** The shell ended and the pane stays (D20): from now on only Enter acts, calling `onEnter`. */
  setExited(onEnter: () => void): void;
}
