import { describe, expect, it } from "vitest";
import { renderViewer } from "./viewer-view";

describe("renderViewer", () => {
  it("connects each tab with its selected tab panel", () => {
    document.body.innerHTML = renderViewer({
      recording: { id: "packet", project_id: "project", video_path: "", metadata_path: "", note: "", recording_scope: "repository", git_branch: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, duration_seconds: 1, size_bytes: null, success: true, transcript: "hello", transcript_path: null, transcript_segments: [], transcription_status: "complete", transcription_error: null, transcription_language: null, poster_path: null, timeline_notes: [] },
      videoAsset: "", videoSource: "", poster: "", panel: "transcript", actionsMenu: "", markedTime: 0, noteDraft: "", listening: false, voiceProcessing: false,
    });
    const tab = document.querySelector<HTMLElement>("[role='tab'][aria-selected='true']")!;
    const panel = document.getElementById(tab.getAttribute("aria-controls")!);
    expect(tab.tabIndex).toBe(0);
    expect(panel?.getAttribute("role")).toBe("tabpanel");
    expect(panel?.getAttribute("aria-labelledby")).toBe(tab.id);
    expect(document.querySelectorAll("[role='tab'][tabindex='-1']")).toHaveLength(2);
  });
});
