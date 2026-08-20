import { describe, expect, it, vi } from "vitest";
import { handleViewerTabKeydown } from "./viewer-tab-events";

describe("handleViewerTabKeydown", () => {
  it.each([
    ["ArrowRight", "chapters"],
    ["ArrowLeft", "notes"],
    ["Home", "transcript"],
    ["End", "notes"],
  ])("moves focus and selects with %s", (key, expectedPanel) => {
    document.body.innerHTML = `<div role="tablist"><button role="tab" data-viewer-panel="transcript">Transcript</button><button role="tab" data-viewer-panel="chapters">Chapters</button><button role="tab" data-viewer-panel="notes">Notes</button></div>`;
    const first = document.querySelector<HTMLElement>("[data-viewer-panel='transcript']")!;
    first.focus();
    const select = vi.fn();
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });

    expect(handleViewerTabKeydown(event, first, select)).toBe(true);
    expect((document.activeElement as HTMLElement).dataset.viewerPanel).toBe(expectedPanel);
    expect(select).toHaveBeenCalledWith(expectedPanel);
    expect(event.defaultPrevented).toBe(true);
  });
});
