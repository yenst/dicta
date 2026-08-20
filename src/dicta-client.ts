import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings, Bootstrap, CleanupSummary, Project, Recording, Status, TimelineNote } from "./types";

export interface StopRecordingResult {
  recording: Recording | null;
  status: Status | null;
}

export type ProjectPickerResult =
  | { kind: "linked"; project: Project }
  | { kind: "cancelled" }
  | { kind: "manual" };

export interface DictaClient {
  readonly isNative: boolean;
  bootstrap(): Promise<Bootstrap>;
  getAppSettings(): Promise<AppSettings>;
  selectProject(projectId: string | null): Promise<void>;
  refreshProject(projectId: string): Promise<Project>;
  listRecordings(projectId: string): Promise<Recording[]>;
  linkProjectFromPicker(): Promise<ProjectPickerResult>;
  createDemoProject(name: string): Promise<Project | null>;
  removeProject(projectId: string): Promise<void>;
  chooseGeneralPath(project: Project): Promise<Project | null>;
  setBranchLocking(enabled: boolean): Promise<AppSettings>;
  setShortcut(shortcutId: string): Promise<AppSettings>;
  setTranscriptionLanguage(language: string): Promise<AppSettings>;
  setCleanupMergedVideos(enabled: boolean): Promise<AppSettings>;
  cleanupMergedVideos(projectId: string): Promise<CleanupSummary>;
  saveTimelineNotes(recording: Recording, notes: TimelineNote[]): Promise<Recording>;
  buildRecordingContext(projectId: string, recordingId: string): Promise<string>;
  deleteRecording(projectId: string, recordingId: string): Promise<void>;
  retranscribeRecording(projectId: string, recordingId: string, language: string): Promise<void>;
  startRecording(projectId: string, note: string): Promise<Status>;
  stopRecording(): Promise<StopRecordingResult>;
  ensureRecordingPoster(projectId: string, recordingId: string): Promise<string | null>;
  transcribeVoiceNote(audioBytes: number[], mimeType: string, language: string): Promise<string>;
}

export function createNativeDictaClient(): DictaClient {
  return {
    isNative: true,
    bootstrap: () => invoke<Bootstrap>("bootstrap"),
    getAppSettings: () => invoke<AppSettings>("get_app_settings"),
    selectProject: (projectId) => invoke("select_project", { projectId }),
    refreshProject: (projectId) => invoke<Project>("refresh_project", { projectId }),
    listRecordings: (projectId) => invoke<Recording[]>("list_recordings", { projectId }),
    linkProjectFromPicker: async () => {
      const selected = await open({ directory: true, multiple: false, title: "Link a Git project" });
      if (typeof selected !== "string") return { kind: "cancelled" };
      const project = await invoke<Project>("link_project", { sourcePath: selected });
      return { kind: "linked", project };
    },
    createDemoProject: async () => null,
    removeProject: (projectId) => invoke("remove_project", { projectId }),
    chooseGeneralPath: async (project) => {
      const selected = await open({ directory: true, multiple: false, title: "Choose the General recordings folder", defaultPath: project.path });
      if (typeof selected !== "string") return null;
      return invoke<Project>("set_general_path", { path: selected });
    },
    setBranchLocking: (enabled) => invoke<AppSettings>("set_branch_locking", { enabled }),
    setShortcut: (shortcutId) => invoke<AppSettings>("set_shortcut", { shortcutId }),
    setTranscriptionLanguage: (language) => invoke<AppSettings>("set_transcription_language", { language }),
    setCleanupMergedVideos: (enabled) => invoke<AppSettings>("set_cleanup_merged_videos", { enabled }),
    cleanupMergedVideos: (projectId) => invoke<CleanupSummary>("cleanup_merged_videos", { projectId }),
    saveTimelineNotes: (recording, notes) => invoke<Recording>("save_timeline_notes", {
      projectId: recording.project_id,
      recordingId: recording.id,
      timelineNotes: notes,
    }),
    buildRecordingContext: (projectId, recordingId) => invoke<string>("build_recording_context", { projectId, recordingId }),
    deleteRecording: (projectId, recordingId) => invoke("delete_recording", { projectId, recordingId }),
    retranscribeRecording: (projectId, recordingId, language) => invoke("retranscribe_recording", { projectId, recordingId, language }),
    startRecording: async (projectId, note) => {
      await invoke("select_project", { projectId });
      return invoke<Status>("start_recording", { note });
    },
    stopRecording: async () => {
      await invoke("stop_recording");
      return { recording: null, status: null };
    },
    ensureRecordingPoster: (projectId, recordingId) => invoke<string | null>("ensure_recording_poster", { projectId, recordingId }),
    transcribeVoiceNote: (audioBytes, mimeType, language) => invoke<string>("transcribe_voice_note", { audioBytes, mimeType, language }),
  };
}

export function createDemoDictaClient(defaultShortcutId: string): DictaClient {
  let settings: AppSettings = {
    shortcut_id: defaultShortcutId,
    cleanup_merged_videos: true,
    branch_locking: true,
    transcription_language: "auto",
    general_path: null,
  };
  let projects = initialDemoProjects();
  let selectedProjectId: string | null = "api-integration";
  let status: Status = idleStatus(selectedProjectId);
  const recordingsByProject = new Map<string, Recording[]>([["api-integration", demoRecordings("api-integration")]]);

  const updateRecording = (updated: Recording) => {
    const current = recordingsByProject.get(updated.project_id) ?? [];
    recordingsByProject.set(updated.project_id, current.map((recording) => recording.id === updated.id ? updated : recording));
  };

  return {
    isNative: false,
    bootstrap: async () => ({ root_path: "~/Documents/Dicta", projects: [...projects], status: { ...status } }),
    getAppSettings: async () => ({ ...settings }),
    selectProject: async (projectId) => {
      selectedProjectId = projectId;
      if (!["preparing", "recording", "stopping"].includes(status.phase)) status = idleStatus(projectId);
    },
    refreshProject: async (projectId) => {
      const project = projects.find((item) => item.id === projectId);
      if (!project) throw new Error("Project not found");
      return project;
    },
    listRecordings: async (projectId) => [...(recordingsByProject.get(projectId) ?? [])],
    linkProjectFromPicker: async () => ({ kind: "manual" }),
    createDemoProject: async (name) => {
      const project = demoProjectFromName(name);
      projects = [project, ...projects];
      selectedProjectId = project.id;
      status = idleStatus(project.id);
      recordingsByProject.set(project.id, []);
      return project;
    },
    removeProject: async (projectId) => {
      projects = projects.filter((project) => project.id !== projectId);
      recordingsByProject.delete(projectId);
    },
    chooseGeneralPath: async (project) => ({ ...project }),
    setBranchLocking: async (enabled) => (settings = { ...settings, branch_locking: enabled }),
    setShortcut: async (shortcutId) => (settings = { ...settings, shortcut_id: shortcutId }),
    setTranscriptionLanguage: async (language) => (settings = { ...settings, transcription_language: language }),
    setCleanupMergedVideos: async (enabled) => (settings = { ...settings, cleanup_merged_videos: enabled }),
    cleanupMergedVideos: async () => ({
      removed_files: 2,
      freed_bytes: 148_000_000,
      cleaned_branches: ["feature/oauth"],
      default_branch: "main",
      message: "Removed 2 merged videos.",
    }),
    saveTimelineNotes: async (recording, notes) => {
      const updated = { ...recording, timeline_notes: [...notes].sort((left, right) => left.timestamp_seconds - right.timestamp_seconds) };
      updateRecording(updated);
      return updated;
    },
    buildRecordingContext: async (projectId, recordingId) => `Within Dicta project \`${projects.find((project) => project.id === projectId)?.name}\`, look at recording \`${recordingId}\`.`,
    deleteRecording: async (projectId, recordingId) => {
      recordingsByProject.set(projectId, (recordingsByProject.get(projectId) ?? []).filter((recording) => recording.id !== recordingId));
      projects = projects.map((project) => project.id === projectId
        ? { ...project, recording_count: recordingsByProject.get(projectId)?.length ?? 0 }
        : project);
    },
    retranscribeRecording: async (projectId, recordingId, language) => {
      const recording = (recordingsByProject.get(projectId) ?? []).find((item) => item.id === recordingId);
      if (recording) updateRecording({ ...recording, transcription_language: language, transcription_status: "processing" });
    },
    startRecording: async (projectId) => {
      selectedProjectId = projectId;
      status = { phase: "recording", active_project_id: projectId, active_video_path: "/mock/recording.mp4", started_at: new Date().toISOString(), last_error: null };
      return { ...status };
    },
    stopRecording: async () => {
      status = { ...status, phase: "stopping" };
      await new Promise((resolve) => window.setTimeout(resolve, 900));
      const now = new Date().toISOString();
      const projectId = status.active_project_id ?? selectedProjectId ?? "__unprojected__";
      const recording: Recording = {
        id: `demo-${Date.now()}`,
        project_id: projectId,
        video_path: "/mock/new-packet.mp4",
        metadata_path: "/mock/new-packet.json",
        note: "New prompt packet",
        recording_scope: settings.branch_locking ? "branch" : "repository",
        git_branch: projects.find((project) => project.id === projectId)?.git_branch ?? null,
        started_at: status.started_at ?? now,
        ended_at: now,
        duration_seconds: 138,
        size_bytes: 18_400_000,
        success: true,
        transcript: "New prompt packet transcript",
        transcript_path: "/mock/new-packet.transcript.md",
        transcript_segments: [{ start_seconds: 0, end_seconds: 3.2, text: "New prompt packet transcript" }],
        transcription_status: "complete",
        transcription_error: null,
        transcription_language: settings.transcription_language,
        poster_path: null,
        timeline_notes: [],
      };
      recordingsByProject.set(projectId, [recording, ...(recordingsByProject.get(projectId) ?? [])]);
      status = idleStatus(selectedProjectId);
      return { recording, status: { ...status } };
    },
    ensureRecordingPoster: async () => null,
    transcribeVoiceNote: async () => "Demo voice note",
  };
}

function idleStatus(activeProjectId: string | null): Status {
  return { phase: "idle", active_project_id: activeProjectId, active_video_path: null, started_at: null, last_error: null };
}

function initialDemoProjects(): Project[] {
  return [
    { id: "__unprojected__", name: "General", path: "~/Documents/Dicta/General", storage_path: "~/Documents/Dicta/General", source_path: null, git_branch: null, branch_path: "~/Documents/Dicta/General", is_git: false, git_error: null, created_at: new Date(0).toISOString(), recording_count: 0 },
    demoProject("api-integration", "API integration", "feature/oauth", 3),
    demoProject("billing-rewrite", "Billing rewrite", "main", 0),
    demoProject("search-prototype", "Search prototype", "prototype/ranking", 0),
  ];
}

function demoProjectFromName(name: string): Project {
  const slug = name.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-");
  return { id: `mock-${Date.now()}`, name, path: `~/Projects/${slug}`, storage_path: `~/Documents/Dicta/${slug}`, source_path: `~/Projects/${slug}`, git_branch: "main", branch_path: `~/Documents/Dicta/${slug}/branches/main`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: 0 };
}

function demoProject(id: string, name: string, branch: string, recordingCount: number): Project {
  const branchFolder = branch.replaceAll("/", "__");
  return { id, name, path: `~/Projects/${id}`, storage_path: `~/Documents/Dicta/${id}`, source_path: `~/Projects/${id}`, git_branch: branch, branch_path: `~/Documents/Dicta/${id}/branches/${branchFolder}`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: recordingCount };
}

function demoRecordings(projectId: string): Recording[] {
  const base = new Date();
  const transcriptSegments = [
    { start_seconds: 0, end_seconds: 9, text: "In this recording, I’ll walk through the authentication edge cases we need to handle for the OAuth flow." },
    { start_seconds: 10, end_seconds: 37, text: "First, let’s talk about expired access tokens. When a request comes in with an expired token, we should return a 401 with the WWW-Authenticate header and an error code of token_expired." },
    { start_seconds: 38, end_seconds: 50, text: "If the refresh token is valid, the client can use it to get a new access token and retry the request." },
    { start_seconds: 51, end_seconds: 71, text: "Next, consider revoked refresh tokens. In that case, we must return 401 invalid_grant and force the user to re-authenticate." },
    { start_seconds: 72, end_seconds: 90, text: "Another case is missing scopes. If the token is valid but doesn’t include the required scope, return 403 insufficient_scope." },
    { start_seconds: 91, end_seconds: 107, text: "Finally, for rate limiting on token endpoints, respond with 429 and include a Retry-After header." },
    { start_seconds: 108, end_seconds: 128, text: "I’ll add examples for each case in the API docs and update the error handling middleware to standardize these responses." },
  ];
  const item = (id: string, note: string, seconds: number, hourOffset: number, success = true): Recording => ({ id, project_id: projectId, video_path: `~/Documents/Dicta/api-integration/branches/feature__oauth/${id}.mp4`, metadata_path: `~/Documents/Dicta/api-integration/branches/feature__oauth/${id}.json`, note, recording_scope: "branch", git_branch: "feature/oauth", started_at: new Date(base.getTime() - hourOffset * 3_600_000).toISOString(), ended_at: base.toISOString(), duration_seconds: seconds, size_bytes: 12_000_000, success, transcript: success ? transcriptSegments.map((segment) => segment.text).join(" ") : null, transcript_path: success ? `/mock/${id}.transcript.md` : null, transcript_segments: success ? transcriptSegments : [], transcription_status: success ? "complete" : "processing", transcription_error: null, transcription_language: "en", poster_path: null, timeline_notes: id === "20260818-15-53-49" ? [
    { id: "demo-note-1", timestamp_seconds: 22, text: "The expired-token response should preserve the original request ID.", created_at: base.toISOString(), source: "typed" },
    { id: "demo-note-2", timestamp_seconds: 74, text: "Compare this refresh path with the retry behavior shown later.", created_at: base.toISOString(), source: "voice" },
  ] : [] });
  return [
    item("20260818-15-53-49", "Authentication edge cases", 1721, 1),
    item("20260818-14-22-08", "Webhook payload design", 1156, 2, false),
    item("20260817-18-04-31", "Retry behavior and backoff", 963, 26),
  ];
}
