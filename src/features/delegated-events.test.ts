import { describe, expect, it, vi } from "vitest";
import { mountDelegatedEvents } from "./delegated-events";
import { shouldDismissModal } from "./modal-lifecycle";

describe("mountDelegatedEvents", () => {
  it("routes events from replacement descendants and disposes cleanly", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const handle = vi.fn();
    const dispose = mountDelegatedEvents(root, [{ type: "click", selector: "[data-action]", handle }]);
    root.innerHTML = `<button data-action><span>Run</span></button>`;
    root.querySelector("span")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(handle).toHaveBeenCalledOnce();

    dispose();
    root.querySelector("button")!.click();
    expect(handle).toHaveBeenCalledOnce();
  });

  it("routes viewer tabs and timestamp controls through the app root", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const panels: string[] = [];
    const times: number[] = [];
    const dispose = mountDelegatedEvents(root, [
      { type: "click", selector: "[data-viewer-panel]", handle: (_event, matched) => { panels.push(matched.dataset.viewerPanel!); } },
      { type: "click", selector: "[data-transcript-time]", handle: (_event, matched) => { times.push(Number(matched.dataset.transcriptTime)); } },
    ]);
    root.innerHTML = `<article id="packet-viewer"><button data-viewer-panel="notes">Notes</button><button data-transcript-time="12.5">Jump</button></article>`;

    root.querySelector<HTMLButtonElement>("[data-viewer-panel]")!.click();
    root.querySelector<HTMLButtonElement>("[data-transcript-time]")!.click();
    expect(panels).toEqual(["notes"]);
    expect(times).toEqual([12.5]);
    dispose();
  });

  it("routes modal language and close controls without treating form clicks as backdrop clicks", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const languages: string[] = [];
    let closes = 0;
    const dispose = mountDelegatedEvents(root, [
      { type: "click", selector: "[data-language]", handle: (_event, matched) => { languages.push(matched.dataset.language!); } },
      { type: "click", selector: "[data-close-modal]", handle: (event, matched) => { if (shouldDismissModal(event, matched)) closes += 1; } },
    ]);
    root.innerHTML = `<div class="modal-backdrop" data-close-modal><form class="modal"><button type="button" data-language="nl">Dutch</button><button type="button" data-close-modal>Cancel</button><span>Body</span></form></div>`;

    root.querySelector<HTMLButtonElement>("[data-language]")!.click();
    root.querySelector("span")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(languages).toEqual(["nl"]);
    expect(closes).toBe(0);
    root.querySelector<HTMLButtonElement>("form [data-close-modal]")!.click();
    expect(closes).toBe(1);
    root.querySelector<HTMLElement>(".modal-backdrop")!.click();
    expect(closes).toBe(2);
    dispose();
  });
});
