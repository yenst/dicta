export type RecordingPhase = "idle" | "preparing" | "recording" | "stopping" | "error";

export interface Project {
  id: string;
  name: string;
  path: string;
  storage_path: string;
  source_path: string | null;
  git_branch: string | null;
  branch_path: string | null;
  is_git: boolean;
  git_error: string | null;
  created_at: string;
  recording_count: number;
}

export interface Recording {
  id: string;
  project_id: string;
  video_path: string;
  metadata_path: string;
  note: string;
  recording_scope: "branch" | "repository" | "unprojected";
  git_branch: string | null;
  started_at: string;
  ended_at: string | null;
  duration_seconds: number | null;
  size_bytes: number | null;
  success: boolean;
  transcript: string | null;
  transcript_path: string | null;
  transcript_segments: TranscriptSegment[];
  transcription_status: "pending" | "processing" | "complete" | "failed" | "";
  transcription_error: string | null;
  transcription_language: string | null;
  poster_path: string | null;
  timeline_notes: TimelineNote[];
}

export interface TranscriptSegment {
  start_seconds: number;
  end_seconds: number;
  text: string;
}

export interface TimelineNote {
  id: string;
  timestamp_seconds: number;
  text: string;
  created_at: string;
  source: "typed" | "voice";
}

export interface Status {
  phase: RecordingPhase;
  active_project_id: string | null;
  active_video_path: string | null;
  started_at: string | null;
  last_error: string | null;
}

export interface Bootstrap {
  root_path: string;
  projects: Project[];
  status: Status;
}

export interface McpStatus {
  installed: boolean;
  codex_configured: boolean;
  executable_path: string;
  message: string;
}

export interface ModelStatus {
  bundled_ready: boolean;
  quality_installed: boolean;
  quality_path: string;
  quality_size_bytes: number;
  download_size_bytes: number;
  active_model: string;
  active_model_path: string;
  message: string;
}

export interface ModelDownloadEvent {
  downloaded_bytes: number;
  total_bytes: number;
  progress: number;
  status: "downloading" | "verifying" | "complete" | "error";
  message: string;
}

export interface AppSettings {
  shortcut_id: string;
  cleanup_merged_videos: boolean;
  branch_locking: boolean;
  transcription_language: string;
  general_path: string | null;
}

export interface CleanupSummary {
  removed_files: number;
  freed_bytes: number;
  cleaned_branches: string[];
  default_branch: string | null;
  message: string;
}

export interface RecorderEvent {
  event: "preparing" | "started" | "stopping" | "finished" | "transcribing" | "transcribed" | "transcription_error" | "error";
  message: string;
  status: Status;
}
