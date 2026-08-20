import { describe, expect, it } from "vitest";
import { createPlatformCapabilities } from "../platform";
import { renderModals, type ModalsViewModel } from "./modals-view";

const baseModel: ModalsViewModel = {
  createProjectOpen: false,
  startSheetOpen: false,
  projects: [],
  branchLocking: true,
  sessionNote: "",
  transcribeRecordingId: null,
  selectedTranscriptionLanguage: "auto",
  transcriptionLanguages: [],
  platform: createPlatformCapabilities("linux"),
};

describe("renderModals", () => {
  it("labels modal dialogs and exposes only open dialogs", () => {
    document.body.innerHTML = renderModals({ ...baseModel, createProjectOpen: true });
    const dialog = document.querySelector<HTMLElement>("[role='dialog']")!;
    expect(dialog).not.toBeNull();
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(dialog.getAttribute("aria-labelledby")!)).not.toBeNull();
    expect(document.querySelectorAll("[role='dialog']")).toHaveLength(1);
  });
});
