import type { Project, Recording, Status } from "../types";
import { escapeHtml, formatDuration, recordingSubtitle, recordingTitle, scopeLabel } from "./view-helpers";

export interface RecordingGroup {
  key: string;
  label: string;
  items: Recording[];
}

export interface RecordingsViewModel {
  selectedProjectId: string | null;
  project?: Project;
  recordings: Recording[];
  visibleGroups: RecordingGroup[];
  viewingRecordingId: string | null;
  branchLocking: boolean;
  buttonDisabled: boolean;
  status: Status;
  shortcutLabel: string;
  viewerHtml: string;
}

export type RecordingIndexViewModel = Pick<RecordingsViewModel,
  "recordings" | "visibleGroups" | "project" | "branchLocking" | "buttonDisabled" | "viewingRecordingId"
>;

export function renderRecordings(vm: RecordingsViewModel): string {
  return `<section class="packet-section split-review-workspace" data-project-id="${escapeHtml(vm.selectedProjectId ?? "")}">
    <aside class="recording-index" aria-label="Recordings">
      <header class="recording-index-header"><div><h2>Recordings</h2><button id="focus-recording-search" type="button" aria-label="Search and filter recordings"><i class="ph ph-funnel-simple"></i></button></div></header>
      <div class="recording-index-body">${renderRecordingIndexBody(vm)}</div>
    </aside>
    ${vm.viewerHtml}
    <div class="capture-dock ${vm.status.phase === "recording" ? "active" : ""}" aria-live="polite">
      <button class="capture-dock-button" id="record-toggle" type="button" aria-label="${vm.status.phase === "recording" ? "Stop recording" : "Start recording"}" title="${vm.status.phase === "recording" ? "Stop recording" : `Record · ${escapeHtml(vm.shortcutLabel)}`}" ${vm.buttonDisabled ? "disabled" : ""}><span class="capture-dock-icon"><i class="ph ${vm.status.phase === "recording" ? "ph-stop" : "ph-record"}"></i></span></button>
    </div>
  </section>`;
}

export function renderRecordingIndexBody(vm: RecordingIndexViewModel): string {
  if (vm.recordings.length === 0) {
    return `<div class="empty-state split-empty-state"><i class="ph ph-monitor-play" aria-hidden="true"></i><h3>Your first explanation starts here</h3><p>Record your screen and voice. This packet will be saved to ${escapeHtml(scopeLabel(vm.project, vm.branchLocking))}.</p><button id="empty-record" ${vm.buttonDisabled ? "disabled" : ""}>Record</button></div>`;
  }
  if (vm.visibleGroups.length === 0) {
    return `<div class="recording-search-empty"><i class="ph ph-magnifying-glass"></i><strong>No matching recordings</strong><span>Try another ID, note, or transcript phrase.</span></div>`;
  }
  return vm.visibleGroups.map((group) => `<div class="recording-index-group"><div class="recording-index-date"><span>${escapeHtml(group.label)}</span><i></i></div>${group.items.map((recording) => renderRecording(recording, vm.viewingRecordingId)).join("")}</div>`).join("");
}

function renderRecording(recording: Recording, viewingRecordingId: string | null): string {
  const subtitle = recordingSubtitle(recording);
  return `<div class="recording-index-item ${recording.id === viewingRecordingId ? "selected" : ""}" data-recording-id="${escapeHtml(recording.id)}">
    <button class="recording-index-main" data-open-packet="${escapeHtml(recording.id)}" aria-label="Open ${escapeHtml(recording.id)}"><span class="recording-index-play"><i class="ph ph-play"></i></span><span class="recording-index-copy"><strong>${escapeHtml(recordingTitle(recording))}</strong>${subtitle ? `<small>${escapeHtml(subtitle)}</small>` : ""}</span><span class="recording-index-duration">${formatDuration(recording.duration_seconds)}</span></button>
    <button class="recording-index-copy-context" type="button" data-copy-recording-context="${escapeHtml(recording.id)}" aria-label="Copy context for ${escapeHtml(recording.id)}" title="Copy context"><i class="ph ph-copy"></i></button>
  </div>`;
}
