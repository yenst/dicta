import type { Recording, TimelineNote } from "../types";
import { escapeHtml, formatViewerTime, recordingSubtitle, recordingTitle } from "./view-helpers";

export type ViewerPanel = "notes" | "transcript" | "chapters";

export interface ViewerViewModel {
  recording?: Recording;
  videoAsset: string;
  videoSource: string;
  poster: string;
  panel: ViewerPanel;
  actionsMenu: string;
  markedTime: number;
  noteDraft: string;
  listening: boolean;
  voiceProcessing: boolean;
  recordingDrawerOpen: boolean;
}

export function renderViewer(vm: ViewerViewModel): string {
  const recording = vm.recording;
  if (!recording) return `<div class="inline-review-empty"><i class="ph ph-video-camera"></i><strong>Select a recording</strong><span>Choose a recording to review its video and transcript.</span></div>`;
  const notes = recording.timeline_notes ?? [];
  const segments = recording.transcript_segments ?? [];
  return `<article class="inline-review" id="packet-viewer" tabindex="-1" aria-label="Recording review">
    <header class="inline-review-header">
      <div class="inline-review-title"><button class="compact-recordings-button" id="toggle-recording-drawer" type="button" aria-label="Browse recordings" aria-expanded="${vm.recordingDrawerOpen}"><i class="ph ph-list-bullets"></i><span>Recordings</span></button><div><h2>${escapeHtml(recordingTitle(recording))}</h2>${recordingSubtitle(recording) ? `<p>${escapeHtml(recordingSubtitle(recording))}</p>` : ""}</div><div class="inline-action-wrap"><button class="inline-more" data-menu="${escapeHtml(recording.id)}" data-menu-surface="detail" aria-label="Recording actions"><i class="ph ph-dots-three"></i></button>${vm.actionsMenu}</div></div>
      <div class="inline-review-meta"><span>${new Intl.DateTimeFormat(undefined, { month: "long", day: "numeric", year: "numeric" }).format(new Date(recording.started_at))} · ${new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hour12: false }).format(new Date(recording.started_at))}</span><button class="review-context" type="button" data-copy-recording-context="${escapeHtml(recording.id)}" aria-label="Copy recording context" title="Copy context"><i class="ph ph-copy"></i></button></div>
    </header>
    <div class="inline-review-scroll">
      <div class="inline-video-shell">${renderVideo(vm)}</div>
      ${renderTabs(vm.panel, notes)}
      ${renderPanel(vm, recording, notes, segments)}
    </div>
  </article>`;
}

function renderVideo(vm: ViewerViewModel): string {
  if (!vm.videoAsset && !vm.poster) return `<div class="viewer-missing"><i class="ph ph-video-camera-slash"></i><span>Video file is missing.</span></div>`;
  return `<video id="packet-video" preload="metadata" playsinline controls${vm.poster ? ` poster="${escapeHtml(vm.poster)}"` : ""}>${vm.videoSource ? `<source src="${escapeHtml(vm.videoSource)}" type="video/mp4">` : ""}</video><div class="viewer-playback-error" id="viewer-playback-error" role="alert" hidden><i class="ph ph-warning-circle"></i><span>Video playback is unavailable.</span></div>`;
}

function renderTabs(panel: ViewerPanel, notes: TimelineNote[]): string {
  return `<div class="inline-review-tabs" role="tablist" aria-label="Recording details">
    ${renderTab("transcript", "Transcript", panel)}
    ${renderTab("chapters", "Chapters", panel)}
    ${renderTab("notes", `Notes${notes.length ? ` <span>${notes.length}</span>` : ""}`, panel)}
  </div>`;
}

function renderTab(id: ViewerPanel, label: string, selected: ViewerPanel): string {
  const active = id === selected;
  return `<button id="viewer-tab-${id}" type="button" role="tab" aria-controls="viewer-panel-${id}" aria-selected="${active}" tabindex="${active ? "0" : "-1"}" class="${active ? "selected" : ""}" data-viewer-panel="${id}">${label}</button>`;
}

function renderPanel(vm: ViewerViewModel, recording: Recording, notes: TimelineNote[], segments: NonNullable<Recording["transcript_segments"]>): string {
  if (vm.panel === "transcript") {
    const content = segments.length
      ? segments.map((segment) => `<article class="inline-transcript-segment"><button type="button" data-transcript-time="${segment.start_seconds}">${formatViewerTime(segment.start_seconds)}</button><p>${escapeHtml(segment.text)}</p></article>`).join("")
      : recording.transcript?.trim()
        ? `<p class="inline-transcript-copy">${escapeHtml(recording.transcript)}</p>`
        : `<div class="notes-empty"><i class="ph ph-waveform"></i><strong>No transcript yet</strong><p>${escapeHtml(recording.transcription_error || (recording.transcription_status === "processing" ? "The transcript is still being written." : "Transcribe this recording to read it here."))}</p></div>`;
    return `<div class="inline-transcript" id="viewer-panel-transcript" role="tabpanel" aria-labelledby="viewer-tab-transcript">${content}</div>`;
  }
  if (vm.panel === "chapters") {
    const content = segments.length ? segments.map((segment, index) => `<button type="button" data-transcript-time="${segment.start_seconds}"><span>${formatViewerTime(segment.start_seconds)}</span><div><strong>${index === 0 ? "Overview" : `Chapter ${index + 1}`}</strong><small>${escapeHtml(segment.text)}</small></div><i class="ph ph-caret-right"></i></button>`).join("") : `<div class="notes-empty"><i class="ph ph-list-numbers"></i><strong>No chapters yet</strong><p>Timestamped transcript sections will appear here.</p></div>`;
    return `<div class="inline-chapters" id="viewer-panel-chapters" role="tabpanel" aria-labelledby="viewer-tab-chapters">${content}</div>`;
  }
  return `<div class="inline-notes" id="viewer-panel-notes" role="tabpanel" aria-labelledby="viewer-tab-notes">
    <form class="note-composer" id="timeline-note-form"><div class="composer-heading"><span class="composer-time"><i class="ph ph-map-pin"></i><strong id="marked-time">${formatViewerTime(vm.markedTime)}</strong></span><button type="button" id="use-current-time">Use current time</button></div><textarea id="timeline-note-input" rows="3" maxlength="2000" placeholder="What should an agent notice here?">${escapeHtml(vm.noteDraft)}</textarea><div class="composer-actions"><button class="dictate-button ${vm.listening ? "listening" : ""} ${vm.voiceProcessing ? "processing" : ""}" id="dictate-note" type="button" ${vm.voiceProcessing ? "disabled" : ""}><i class="ph ${vm.voiceProcessing ? "ph-spinner-gap" : vm.listening ? "ph-stop-circle" : "ph-microphone"}"></i><span>${vm.voiceProcessing ? "Transcribing…" : vm.listening ? "Listening…" : "Speak"}</span></button><button class="add-note-button" type="submit" ${vm.noteDraft.trim() ? "" : "disabled"}>Add note</button></div></form>
    <div class="timeline-notes">${notes.length ? notes.map(renderNote).join("") : `<div class="notes-empty"><i class="ph ph-map-pin-line"></i><strong>No notes yet</strong><p>Mark a moment in the video, then type or speak what matters.</p></div>`}</div>
  </div>`;
}

function renderNote(note: TimelineNote): string {
  return `<article class="timeline-note"><button class="note-jump" type="button" data-note-time="${note.timestamp_seconds}"><i class="ph ph-play"></i>${formatViewerTime(note.timestamp_seconds)}</button><div><p>${escapeHtml(note.text)}</p><small>${note.source === "voice" ? `<i class="ph ph-microphone"></i> Spoken note` : "Timestamp note"}</small></div><button class="note-delete" type="button" data-delete-note="${escapeHtml(note.id)}" aria-label="Delete note"><i class="ph ph-trash"></i></button></article>`;
}
