import { describe, expect, it } from "vitest";
import { captureFocusKey, restoreFocusKey } from "./focus-restoration";

describe("focus restoration", () => {
  it("restores an equivalent recording control after the shell is replaced", () => {
    const root = document.createElement("div");
    document.body.append(root);
    root.innerHTML = `<button data-delete="recording-1">Delete</button>`;
    const trigger = root.querySelector<HTMLButtonElement>("button")!;
    trigger.focus();
    const key = captureFocusKey(root, document.activeElement);

    root.innerHTML = `<form role="dialog" aria-modal="true"><button>Confirm</button></form>`;
    root.innerHTML = `<button data-open-packet="recording-1">Recording 1</button>`;

    expect(restoreFocusKey(root, key)).toBe(true);
    expect(document.activeElement).toBe(root.querySelector("button"));
  });
});
