/**
 * In-page confirmation (D21), without a Tauri plugin. The message only goes
 * through `textContent`. Cancel has the focus, so a stray Enter keeps the pane;
 * Escape cancels.
 */
export function confirmInPage(root: HTMLElement, message: string, confirmLabel = "Close"): Promise<boolean> {
  const doc = root.ownerDocument;
  return new Promise((resolve) => {
    const backdrop = doc.createElement("div");
    backdrop.className = "confirm-backdrop";
    const dialog = doc.createElement("div");
    dialog.className = "confirm";
    dialog.setAttribute("role", "alertdialog");
    dialog.setAttribute("aria-modal", "true");
    const text = doc.createElement("p");
    text.className = "confirm-message";
    text.textContent = message;
    dialog.setAttribute("aria-label", message);
    const cancel = doc.createElement("button");
    cancel.type = "button";
    cancel.className = "confirm-cancel";
    cancel.textContent = "Cancel";
    const ok = doc.createElement("button");
    ok.type = "button";
    ok.className = "confirm-ok";
    ok.textContent = confirmLabel;
    const buttons = doc.createElement("div");
    buttons.className = "confirm-buttons";
    buttons.append(cancel, ok);
    dialog.append(text, buttons);
    backdrop.append(dialog);

    const done = (answer: boolean) => {
      backdrop.remove();
      resolve(answer);
    };
    cancel.addEventListener("click", () => done(false));
    ok.addEventListener("click", () => done(true));
    dialog.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        ev.stopPropagation();
        done(false);
      } else if (ev.key === "Tab") {
        // Keep the focus inside the dialog.
        ev.preventDefault();
        (doc.activeElement === ok ? cancel : ok).focus();
      }
    });
    root.append(backdrop);
    cancel.focus();
  });
}
