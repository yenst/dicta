import { describe, expect, it } from "vitest";
import type { DictaClient } from "./dicta-client";
import { ProjectController } from "./project-controller";
import type { Project, Recording } from "./types";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}

function project(id: string): Project {
  return { id, name: id, path: id, storage_path: id, source_path: id, git_branch: "main", branch_path: id, is_git: true, git_error: null, created_at: "2026-01-01", recording_count: 0 };
}

function recording(projectId: string): Recording {
  return { id: `${projectId}-recording`, project_id: projectId, video_path: "video.mp4", metadata_path: "metadata.json", note: "", recording_scope: "branch", git_branch: "main", started_at: "2026-01-01", ended_at: null, duration_seconds: null, size_bytes: null, success: true, transcript: null, transcript_path: null, transcript_segments: [], transcription_status: "", transcription_error: null, transcription_language: null, poster_path: "poster.png", timeline_notes: [] };
}

describe("ProjectController", () => {
  it("does not allow a late project recording response to overwrite the current project", async () => {
    const projectA = deferred<Recording[]>();
    const projectB = deferred<Recording[]>();
    const client = {
      listRecordings: (projectId: string) => projectId === "a" ? projectA.promise : projectB.promise,
      ensureRecordingPoster: async () => null,
    } as unknown as DictaClient;
    const controller = new ProjectController(client, () => undefined);
    controller.hydrate([project("a"), project("b")], "a");

    const loadA = controller.refreshRecordings("a");
    controller.select("b");
    const loadB = controller.refreshRecordings("b");
    projectB.resolve([recording("b")]);
    await loadB;
    projectA.resolve([recording("a")]);
    await loadA;

    expect(controller.recordings.map((item) => item.project_id)).toEqual(["b"]);
  });

  it("does not mislabel a recording storage error as a Git failure", async () => {
    const linkedProject = project("peepel");
    const client = {
      listRecordings: async () => { throw new Error("Recording artifact escaped the active recording folder"); },
      ensureRecordingPoster: async () => null,
    } as unknown as DictaClient;
    const controller = new ProjectController(client, () => undefined);
    controller.hydrate([linkedProject], linkedProject.id);

    const result = await controller.refreshRecordings(linkedProject.id);

    expect(result.error).toContain("Recording artifact escaped");
    expect(controller.projects[0]?.git_branch).toBe("main");
    expect(controller.projects[0]?.git_error).toBeNull();
  });
});
