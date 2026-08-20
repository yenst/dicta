import { afterEach, describe, expect, it, vi } from "vitest";
import { mountModalLifecycle } from "./modal-lifecycle";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("mountModalLifecycle", () => {
  it("focuses the dialog and closes it with Escape", () => {
    document.body.innerHTML = `<button id="before">Before</button><div role="dialog" aria-modal="true" tabindex="-1"><button autofocus>First</button><button>Last</button></div>`;
    const before = document.querySelector<HTMLButtonElement>("#before")!;
    before.focus();
    const onEscape = vi.fn();
    const dispose = mountModalLifecycle(document, { onEscape });

    expect(document.activeElement?.textContent).toBe("First");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onEscape).toHaveBeenCalledOnce();

    dispose();
    expect(document.activeElement).toBe(before);
  });

  it("wraps keyboard focus within the active dialog", () => {
    document.body.innerHTML = `<div role="dialog" aria-modal="true" tabindex="-1"><button>First</button><button>Last</button></div>`;
    const [first, last] = [...document.querySelectorAll<HTMLButtonElement>("button")];
    const dispose = mountModalLifecycle(document, { onEscape: vi.fn() });

    last.focus();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    expect(document.activeElement).toBe(first);

    first.focus();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true }));
    expect(document.activeElement).toBe(last);
    dispose();
  });
});
