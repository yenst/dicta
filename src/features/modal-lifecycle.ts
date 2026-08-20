const focusableSelector = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export interface ModalLifecycleOptions {
  onEscape: () => void;
  document?: Document;
}

export function shouldDismissModal(event: Event, matched: HTMLElement): boolean {
  return !matched.classList.contains("modal-backdrop") || event.target === matched;
}

export function mountModalLifecycle(root: ParentNode, options: ModalLifecycleOptions): () => void {
  const documentRef = options.document ?? document;
  const modal = [...root.querySelectorAll<HTMLElement>("[role='dialog'][aria-modal='true']")].at(-1);
  if (!modal) return () => undefined;

  const previousFocus = documentRef.activeElement instanceof HTMLElement ? documentRef.activeElement : null;
  const focusable = () => [...modal.querySelectorAll<HTMLElement>(focusableSelector)].filter((element) => !element.hidden);
  const initialFocus = modal.querySelector<HTMLElement>("[autofocus]") ?? focusable()[0] ?? modal;
  initialFocus.focus({ preventScroll: true });

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      event.preventDefault();
      options.onEscape();
      return;
    }
    if (event.key !== "Tab") return;

    const candidates = focusable();
    if (candidates.length === 0) {
      event.preventDefault();
      modal.focus({ preventScroll: true });
      return;
    }
    const first = candidates[0];
    const last = candidates.at(-1)!;
    if (event.shiftKey && documentRef.activeElement === first) {
      event.preventDefault();
      last.focus();
      return;
    }
    if (!event.shiftKey && documentRef.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  documentRef.addEventListener("keydown", onKeyDown);
  return () => {
    documentRef.removeEventListener("keydown", onKeyDown);
    if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  };
}
