import type { PlatformCapabilities } from "../platform";
import type { Project, Recording } from "../types";
import { escapeHtml, formatDate, formatDuration, recordingTitle } from "./view-helpers";

export interface TranscriptionLanguageOption {
  code: string;
  label: string;
  native: string;
}

export interface ModalsViewModel {
  createProjectOpen: boolean;
  startSheetOpen: boolean;
  targetProject?: Project;
  projects: Project[];
  branchLocking: boolean;
  sessionNote: string;
  transcribeRecordingId: string | null;
  selectedTranscriptionLanguage: string;
  transcriptionLanguages: TranscriptionLanguageOption[];
  recordingToDelete?: Recording;
  projectToRemove?: Project;
  platform: PlatformCapabilities;
}

export function renderModals(vm: ModalsViewModel): string {
  return [renderCreateProject(vm), renderStartRecording(vm), renderTranscribe(vm), renderDeleteRecording(vm), renderRemoveProject(vm)].join("");
}

function renderCreateProject(vm: ModalsViewModel): string {
  if (!vm.createProjectOpen) return "";
  return `
    <div class="modal-backdrop" data-close-modal>
      <form class="modal" id="create-project-form" role="dialog" aria-modal="true" aria-labelledby="create-project-title" tabindex="-1">
        <button class="modal-close" type="button" data-close-modal aria-label="Close"><i class="ph ph-x"></i></button>
        <i class="ph ph-folder-plus modal-icon"></i>
        <h2 id="create-project-title">Link Git project</h2>
        <p>In the desktop app this opens a native folder picker and detects the current branch.</p>
        <label>Demo folder name<input id="project-name" maxlength="64" placeholder="peepel" autofocus required /></label>
        <div class="modal-actions"><button type="button" class="secondary" data-close-modal>Cancel</button><button type="submit" class="primary">Link folder</button></div>
      </form>
    </div>`;
}

function renderStartRecording(vm: ModalsViewModel): string {
  if (!vm.startSheetOpen) return "";
  const target = vm.targetProject;
  return `
    <div class="modal-backdrop" data-close-start>
      <form class="modal record-sheet" id="start-recording-form" role="dialog" aria-modal="true" aria-labelledby="start-recording-title" tabindex="-1">
        <button class="modal-close" type="button" data-close-start aria-label="Close"><i class="ph ph-x"></i></button>
        <span class="record-symbol sheet-symbol"><span></span></span>
        <h2 id="start-recording-title">Start a prompt packet</h2>
        <p>Choose where this recording belongs. You can keep browsing other projects while capture runs.</p>
        <label>Save to
          <select id="recording-project">${vm.projects.map((project) => `<option value="${escapeHtml(project.id)}" ${project.id === target?.id ? "selected" : ""}>${escapeHtml(project.name)}</option>`).join("")}</select>
        </label>
        ${target?.is_git ? `<div class="recording-scope-row"><div><strong>Lock to Git branch</strong><span>${vm.branchLocking ? `Only <b>${escapeHtml(target.git_branch ?? "the current branch")}</b>` : "Available across every branch in this repository"}</span></div><button class="switch ${vm.branchLocking ? "on" : ""}" id="branch-lock-toggle" type="button" role="switch" aria-checked="${vm.branchLocking}"><span></span></button></div>` : `<div class="recording-scope-note"><i class="ph ph-tray"></i><span>General</span></div>`}
        <label>What should Codex understand? <span>Optional</span><textarea id="session-note" rows="3" placeholder="Authentication edge cases, webhook behavior…">${escapeHtml(vm.sessionNote)}</textarea></label>
        <div class="source-summary"><span><i class="ph ph-monitor"></i>Main display</span><span><i class="ph ph-microphone"></i>Microphone</span><span><i class="ph ph-speaker-high"></i>System audio</span><span><i class="ph ph-timer"></i>20 min max</span></div>
        <div class="modal-actions"><button type="button" class="secondary" data-close-start>Cancel</button><button type="submit" class="primary record-primary">Record</button></div>
      </form>
    </div>`;
}

function renderTranscribe(vm: ModalsViewModel): string {
  if (!vm.transcribeRecordingId) return "";
  return `
    <div class="modal-backdrop" data-close-transcribe>
      <form class="modal transcribe-sheet" id="retranscribe-form" role="dialog" aria-modal="true" aria-labelledby="transcribe-title" tabindex="-1">
        <button class="modal-close" type="button" data-close-transcribe aria-label="Close"><i class="ph ph-x"></i></button>
        <div class="transcribe-heading-icon"><i class="ph ph-waveform"></i></div>
        <h2 id="transcribe-title">Transcribe recording</h2>
        <p>Choose the language spoken in this packet. Dicta will replace its transcript while keeping the original video.</p>
        <fieldset class="language-picker"><legend>Spoken language</legend>${vm.transcriptionLanguages.map((language) => `
          <button type="button" class="language-option ${vm.selectedTranscriptionLanguage === language.code ? "selected" : ""}" data-language="${language.code}" role="radio" aria-checked="${vm.selectedTranscriptionLanguage === language.code}">
            <span><strong>${language.label}</strong><small>${language.native}</small></span><i class="ph ${vm.selectedTranscriptionLanguage === language.code ? "ph-check-circle" : "ph-circle"}"></i>
          </button>`).join("")}</fieldset>
        <div class="modal-actions"><button type="button" class="secondary" data-close-transcribe>Cancel</button><button type="submit" class="primary"><i class="ph ph-waveform"></i>Transcribe</button></div>
      </form>
    </div>`;
}

function renderDeleteRecording(vm: ModalsViewModel): string {
  const recording = vm.recordingToDelete;
  if (!recording) return "";
  return `
    <div class="modal-backdrop" data-close-delete>
      <form class="modal delete-sheet" id="delete-recording-form" role="dialog" aria-modal="true" aria-labelledby="delete-recording-title" tabindex="-1">
        <button class="modal-close" type="button" data-close-delete aria-label="Close"><i class="ph ph-x"></i></button>
        <div class="delete-icon"><i class="ph ph-trash"></i></div><h2 id="delete-recording-title">Delete recording?</h2>
        <p>This removes its video, transcript, notes, and metadata from this branch. This cannot be undone.</p>
        <div class="delete-summary"><strong>${escapeHtml(recordingTitle(recording))}</strong><span>${formatDuration(recording.duration_seconds)} · ${formatDate(recording.started_at)}</span></div>
        <div class="modal-actions"><button type="button" class="secondary" data-close-delete>Cancel</button><button type="submit" class="danger">Delete</button></div>
      </form>
    </div>`;
}

function renderRemoveProject(vm: ModalsViewModel): string {
  const project = vm.projectToRemove;
  if (!project) return "";
  return `
    <div class="modal-backdrop" data-close-remove-project>
      <form class="modal delete-sheet" id="remove-project-form" role="dialog" aria-modal="true" aria-labelledby="remove-project-title" tabindex="-1">
        <button class="modal-close" type="button" data-close-remove-project aria-label="Close"><i class="ph ph-x"></i></button>
        <div class="delete-icon"><i class="ph ph-folder-minus"></i></div><h2 id="remove-project-title">Remove ${escapeHtml(project.name)}?</h2>
        <p>This removes the project from Dicta only. ${project.is_git ? "Your repository and its .dicta recordings stay exactly where they are." : "Its recordings and other files remain on disk."}</p>
        <div class="delete-summary"><strong>${escapeHtml(project.name)}</strong><span>${escapeHtml(vm.platform.compactPath(project.source_path ?? project.storage_path))} · ${project.recording_count} recording${project.recording_count === 1 ? "" : "s"}</span></div>
        <div class="modal-actions"><button type="button" class="secondary" data-close-remove-project>Cancel</button><button type="submit" class="danger">Remove project</button></div>
      </form>
    </div>`;
}
