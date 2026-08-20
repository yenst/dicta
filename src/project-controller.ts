import type { DictaClient } from "./dicta-client";
import { RequestGate, type RequestToken } from "./request-gate";
import type { Project, Recording } from "./types";

export interface ProjectRefreshResult {
  applied: boolean;
  error: string | null;
  previousBranch: string | null;
  project: Project | null;
}

export interface RecordingRefreshResult {
  applied: boolean;
  error: string | null;
}

export class ProjectController {
  private readonly projectRequests = new RequestGate();
  private readonly recordingRequests = new RequestGate();
  private projectList: Project[] = [];
  private recordingList: Recording[] = [];
  private selectedId: string | null = null;

  constructor(
    private readonly client: DictaClient,
    private readonly onPosterChange: () => void,
  ) {}

  get projects(): Project[] {
    return this.projectList;
  }

  get recordings(): Recording[] {
    return this.recordingList;
  }

  get selectedProjectId(): string | null {
    return this.selectedId;
  }

  hydrate(projects: Project[], selectedProjectId: string | null): void {
    this.projectList = projects;
    this.selectedId = selectedProjectId;
  }

  replaceProjects(projects: Project[]): void {
    this.projectList = projects;
  }

  select(projectId: string | null): void {
    this.selectedId = projectId;
  }

  activeProject(): Project | undefined {
    return this.projectList.find((project) => project.id === this.selectedId);
  }

  project(projectId: string | null): Project | undefined {
    return this.projectList.find((project) => project.id === projectId);
  }

  upsertProject(project: Project): void {
    this.projectList = [project, ...this.projectList.filter((item) => item.id !== project.id)];
  }

  updateProject(project: Project): void {
    this.projectList = this.projectList.map((item) => item.id === project.id ? project : item);
  }

  removeProject(projectId: string): string | null {
    const removedIndex = this.projectList.findIndex((project) => project.id === projectId);
    this.projectList = this.projectList.filter((project) => project.id !== projectId);
    if (this.selectedId !== projectId) return this.selectedId;
    this.selectedId = this.projectList[Math.min(removedIndex, this.projectList.length - 1)]?.id ?? null;
    return this.selectedId;
  }

  updateRecording(updated: Recording): void {
    this.recordingList = this.recordingList.map((recording) => recording.id === updated.id ? updated : recording);
  }

  removeRecording(recordingId: string): void {
    this.recordingList = this.recordingList.filter((recording) => recording.id !== recordingId);
    const selectedProjectId = this.selectedId;
    this.projectList = this.projectList.map((project) => project.id === selectedProjectId
      ? { ...project, recording_count: this.recordingList.length }
      : project);
  }

  prependRecording(recording: Recording): void {
    this.recordingList = [recording, ...this.recordingList];
  }

  clearRecordings(): void {
    this.recordingRequests.invalidate();
    this.recordingList = [];
  }

  async refreshProject(projectId: string): Promise<ProjectRefreshResult> {
    const request = this.projectRequests.begin(projectId);
    const previousBranch = this.project(projectId)?.git_branch ?? null;
    try {
      const refreshed = await this.client.refreshProject(projectId);
      if (!this.projectRequests.isCurrent(request) || this.selectedId !== projectId) return staleProjectResult(previousBranch);
      this.updateProject(refreshed);
      if (refreshed.git_error) this.clearRecordings();
      else await this.refreshRecordings(projectId);
      if (!this.projectRequests.isCurrent(request) || this.selectedId !== projectId) return staleProjectResult(previousBranch);
      return { applied: true, error: null, previousBranch, project: refreshed };
    } catch (error) {
      if (!this.projectRequests.isCurrent(request) || this.selectedId !== projectId) return staleProjectResult(previousBranch);
      this.clearRecordings();
      return { applied: true, error: String(error), previousBranch, project: null };
    }
  }

  async refreshRecordings(projectId = this.selectedId): Promise<RecordingRefreshResult> {
    if (!projectId) {
      this.clearRecordings();
      return { applied: true, error: null };
    }

    const request = this.recordingRequests.begin(projectId);
    if (this.project(projectId)?.git_error) {
      if (this.selectedId === projectId && this.recordingRequests.isCurrent(request)) this.recordingList = [];
      return { applied: true, error: null };
    }

    try {
      const recordings = await this.client.listRecordings(projectId);
      if (!this.recordingRequests.isCurrent(request) || this.selectedId !== projectId) return { applied: false, error: null };
      this.recordingList = recordings;
      void this.backfillPosters(projectId, recordings, request);
      return { applied: true, error: null };
    } catch (error) {
      if (!this.recordingRequests.isCurrent(request) || this.selectedId !== projectId) return { applied: false, error: null };
      const message = String(error);
      this.recordingList = [];
      return { applied: true, error: message };
    }
  }

  private async backfillPosters(projectId: string, recordings: Recording[], request: RequestToken): Promise<void> {
    let changed = false;
    for (const recording of recordings) {
      if (!this.recordingRequests.isCurrent(request) || this.selectedId !== projectId) return;
      if (recording.poster_path || !recording.success || !recording.video_path) continue;
      try {
        const poster = await this.client.ensureRecordingPoster(projectId, recording.id);
        if (!this.recordingRequests.isCurrent(request) || this.selectedId !== projectId) return;
        if (!poster) continue;
        this.recordingList = this.recordingList.map((item) => item.project_id === projectId && item.id === recording.id
          ? { ...item, poster_path: poster }
          : item);
        changed = true;
      } catch {
        // Keep the fallback thumbnail if a frame cannot be extracted.
      }
    }
    if (changed && this.recordingRequests.isCurrent(request) && this.selectedId === projectId) this.onPosterChange();
  }
}

function staleProjectResult(previousBranch: string | null): ProjectRefreshResult {
  return { applied: false, error: null, previousBranch, project: null };
}
