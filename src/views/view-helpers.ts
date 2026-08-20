import type { Project, Recording } from "../types";

export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "—";
  return `${Math.floor(seconds / 60).toString().padStart(2, "0")}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
}

export function formatViewerTime(seconds: number): string {
  const safeSeconds = Math.max(0, Number.isFinite(seconds) ? seconds : 0);
  return `${Math.floor(safeSeconds / 60)}:${Math.floor(safeSeconds % 60).toString().padStart(2, "0")}`;
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 MB";
  const megabytes = bytes / 1024 / 1024;
  return megabytes >= 1024 ? `${(megabytes / 1024).toFixed(1)} GB` : `${Math.round(megabytes)} MB`;
}

export function formatDate(value: string): string {
  const date = new Date(value);
  const today = new Date();
  const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(date);
  if (date.toDateString() === today.toDateString()) return `Today, ${time}`;
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(date);
}

export function recordingDayHeading(value: string): string {
  return new Intl.DateTimeFormat(undefined, { month: "long", day: "numeric", year: "numeric" }).format(new Date(value));
}

export function transcriptExcerpt(recording: Recording, words = 18): string {
  const transcript = recording.transcript?.trim();
  if (!transcript) return "";
  const parts = transcript.split(/\s+/);
  return parts.length > words ? `${parts.slice(0, words).join(" ")}…` : transcript;
}

export function recordingTitle(recording: Recording): string {
  return recording.id;
}

export function recordingSubtitle(recording: Recording): string {
  return recording.note.trim() || transcriptExcerpt(recording);
}

export function scopeLabel(project: Project | undefined, branchLocking: boolean): string {
  if (!project || project.id === "__unprojected__") return "General";
  if (!branchLocking) return "Repository-wide";
  return project.git_branch ?? "Current branch";
}
