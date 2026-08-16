import "@phosphor-icons/web/regular";
import "./style.css";
import dictaAppIconUrl from "../src-tauri/icons/icon.png";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings, Bootstrap, CleanupSummary, McpStatus, ModelDownloadEvent, ModelStatus, Project, RecorderEvent, Recording, Status, TimelineNote } from "./types";

const app = document.querySelector<HTMLDivElement>("#app")!;
const isTauri = "__TAURI_INTERNALS__" in window && !(import.meta.env.DEV && new URLSearchParams(window.location.search).has("demo"));
const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);
const platformName = isMacPlatform ? "Mac" : "Linux computer";
const defaultShortcutId = isMacPlatform ? "command_shift_r" : "alt_shift_r";

let projects: Project[] = [];
let recordings: Recording[] = [];
let selectedProjectId: string | null = null;
let status: Status = emptyStatus();
let elapsedTimer: number | null = null;
let createProjectOpen = false;
let startSheetOpen = false;
let openPacketMenu: string | null = null;
let openProjectMenu: string | null = null;
let removeProjectId: string | null = null;
let transcribeRecordingId: string | null = null;
let deleteRecordingId: string | null = null;
let selectedTranscriptionLanguage = "auto";
let sessionNote = "";
let lastSessionNote = "";
let viewingRecordingId: string | null = null;
let viewerVideoBlobUrl: string | null = null;
let viewerVideoBlobRecordingId: string | null = null;
let viewerVideoBlobLoad: { recordingId: string; promise: Promise<string> } | null = null;
let viewerTime = 0;
let viewerPaused = true;
let viewerMarkedTime = 0;
let viewerNoteDraft = "";
let viewerNoteSource: TimelineNote["source"] = "typed";
let viewerPanel: "notes" | "transcript" = "notes";
let viewerListening = false;
let viewerVoiceProcessing = false;
let activeSpeechRecognition: SpeechRecognitionLike | null = null;
let activeVoiceRecorder: MediaRecorder | null = null;
let activeVoiceStream: MediaStream | null = null;
let voiceChunks: Blob[] = [];
let voiceStopTimer: number | null = null;
let toastMessage = "";
let mockTimer: number | null = null;
let mockModelTimer: number | null = null;
let mcpRestarting = false;
let mcpStatus: McpStatus = { installed: false, codex_configured: false, executable_path: "", message: "Connect Dicta to Codex" };
let activeView: "project" | "settings" = "project";
let settingsSection: "appearance" | "shortcuts" | "transcription" | "storage" = "appearance";
let appSettings: AppSettings = { shortcut_id: defaultShortcutId, cleanup_merged_videos: true, transcription_language: "auto" };
let cleanupRunning = false;
let cleanupSummary: CleanupSummary | null = null;
type ThemePreference = "system" | "light" | "dark";
const savedTheme = window.localStorage.getItem("dicta-theme");
let themePreference: ThemePreference = savedTheme === "light" || savedTheme === "dark" ? savedTheme : "system";
let modelDownloading = false;
let modelDownload: ModelDownloadEvent | null = null;
let modelStatus: ModelStatus = {
  bundled_ready: true,
  quality_installed: false,
  quality_path: isMacPlatform
    ? "~/Library/Application Support/Dicta/models/ggml-large-v3-turbo-q5_0.bin"
    : "~/.local/share/Dicta/models/ggml-large-v3-turbo-q5_0.bin",
  quality_size_bytes: 0,
  download_size_bytes: 547 * 1024 * 1024,
  active_model: "Compact · base",
  active_model_path: "Dicta.app/Contents/Resources/ggml-base-q5_1.bin",
  message: "The compact offline model is active. Download high quality for better Dutch and technical speech.",
};

interface SpeechRecognitionResultLike {
  isFinal: boolean;
  0: { transcript: string };
}

interface SpeechRecognitionEventLike extends Event {
  resultIndex: number;
  results: ArrayLike<SpeechRecognitionResultLike>;
}

interface SpeechRecognitionLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  start(): void;
  stop(): void;
  abort(): void;
  onresult: ((event: SpeechRecognitionEventLike) => void) | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
}

type SpeechRecognitionConstructor = new () => SpeechRecognitionLike;

function emptyStatus(): Status {
  return { phase: "idle", active_project_id: null, active_video_path: null, started_at: null, last_error: null };
}

function resolvedTheme(): "light" | "dark" {
  if (themePreference === "system") return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  return themePreference;
}

function applyTheme(): void {
  document.documentElement.dataset.theme = resolvedTheme();
  document.documentElement.style.colorScheme = resolvedTheme();
}

function setTheme(preference: ThemePreference): void {
  themePreference = preference;
  window.localStorage.setItem("dicta-theme", preference);
  applyTheme();
  render();
}

applyTheme();

function escapeHtml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#039;");
}

function formatDuration(seconds: number | null): string {
  if (seconds === null) return "—";
  return `${Math.floor(seconds / 60).toString().padStart(2, "0")}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
}

function formatViewerTime(seconds: number): string {
  const safeSeconds = Math.max(0, Number.isFinite(seconds) ? seconds : 0);
  const minutes = Math.floor(safeSeconds / 60);
  const remainder = Math.floor(safeSeconds % 60);
  return `${minutes}:${remainder.toString().padStart(2, "0")}`;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 MB";
  const megabytes = bytes / 1024 / 1024;
  return megabytes >= 1024 ? `${(megabytes / 1024).toFixed(1)} GB` : `${Math.round(megabytes)} MB`;
}

function formatDate(value: string): string {
  const date = new Date(value);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(date);
  return sameDay ? `Today, ${time}` : new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(date);
}

function recordingDayHeading(value: string): string {
  const date = new Date(value);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const prefix = date.toDateString() === today.toDateString()
    ? "Today"
    : date.toDateString() === yesterday.toDateString()
      ? "Yesterday"
      : new Intl.DateTimeFormat(undefined, { weekday: "long" }).format(date);
  const calendarDate = new Intl.DateTimeFormat(undefined, { month: "long", day: "numeric", year: "numeric" }).format(date);
  return `${prefix} — ${calendarDate}`;
}

function headerClock(): { date: string; time: string } {
  const now = new Date();
  return {
    date: new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric", year: "numeric" }).format(now),
    time: new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hour12: false }).format(now),
  };
}

function activeProject(): Project | undefined {
  return projects.find((project) => project.id === selectedProjectId);
}

function compactPath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

function elapsed(): string {
  if (!status.started_at) return "00:00";
  return formatDuration((Date.now() - new Date(status.started_at).getTime()) / 1000);
}

function mediaSrc(path: string | null | undefined): string {
  if (!path || !isTauri) return "";
  return convertFileSrc(path);
}

function transcriptExcerpt(recording: Recording, words = 18): string {
  const transcript = recording.transcript?.trim();
  if (!transcript) return "";
  const parts = transcript.split(/\s+/);
  return parts.length > words ? `${parts.slice(0, words).join(" ")}…` : transcript;
}

function packetStatus(recording: Recording): { className: string; icon: string; label: string; title: string } {
  if (recording.transcription_status === "processing") {
    return { className: "processing", icon: "ph-spinner-gap", label: "Working", title: "Dicta is turning the narration into agent-readable context" };
  }
  if (recording.transcription_status === "complete" || Boolean(recording.transcript?.trim())) {
    return { className: "ready", icon: "ph-check-circle", label: "Ready", title: "Transcript ready for agents" };
  }
  if (!recording.success || recording.transcription_status === "failed") {
    return { className: "failed", icon: "ph-warning-circle", label: "Issue", title: recording.transcription_error ?? "Transcription failed" };
  }
  return { className: "processing", icon: "ph-spinner-gap", label: "Working", title: "Dicta is turning the narration into agent-readable context" };
}

const transcriptionLanguages = [
  { code: "nl", label: "Dutch", native: "Nederlands" },
  { code: "en", label: "English", native: "English" },
  { code: "auto", label: "Auto-detect", native: "Let Whisper decide" },
  { code: "fr", label: "French", native: "Français" },
  { code: "de", label: "German", native: "Deutsch" },
  { code: "es", label: "Spanish", native: "Español" },
];

const shortcutOptions = [
  { id: defaultShortcutId, label: isMacPlatform ? "⌘ ⇧ R" : "Alt Shift R", detail: "Default" },
  { id: "command_shift_d", label: isMacPlatform ? "⌘ ⇧ D" : "Super Shift D", detail: "Dicta" },
  { id: "option_space", label: isMacPlatform ? "⌥ Space" : "Alt Space", detail: "Compact" },
  { id: "control_space", label: isMacPlatform ? "⌃ Space" : "Ctrl Space", detail: "Alternate" },
];

function shortcutLabel(): string {
  return shortcutOptions.find((shortcut) => shortcut.id === appSettings.shortcut_id)?.label ?? shortcutOptions[0].label;
}

function statusCopy(): string {
  if (status.phase === "recording") return "Stop";
  if (status.phase === "preparing") return "Prepare";
  if (status.phase === "stopping") return "Save";
  return "Record";
}

function showToast(message: string): void {
  toastMessage = message;
  render();
  window.setTimeout(() => {
    if (toastMessage === message) {
      toastMessage = "";
      render();
    }
  }, 1800);
}

function render(): void {
  if (elapsedTimer !== null) window.clearInterval(elapsedTimer);
  const previousPacketSection = document.querySelector<HTMLElement>(".packet-section");
  const packetScrollTop = previousPacketSection?.dataset.projectId === (selectedProjectId ?? "")
    ? previousPacketSection.scrollTop
    : 0;
  const project = activeProject();
  const isBusy = ["preparing", "recording", "stopping"].includes(status.phase);
  const branchUnavailable = Boolean(project?.is_git && (!project.git_branch || project.git_error));
  const buttonDisabled = !project || branchUnavailable || status.phase === "preparing" || status.phase === "stopping";
  const recordingToDelete = recordings.find((recording) => recording.id === deleteRecordingId);
  const projectToRemove = projects.find((item) => item.id === removeProjectId);
  const latestRecording = recordings[0];
  const clock = headerClock();
  const recordingGroups = recordings.reduce<Array<{ key: string; label: string; items: Recording[] }>>((groups, recording) => {
    const key = new Date(recording.started_at).toDateString();
    const existing = groups.find((group) => group.key === key);
    if (existing) existing.items.push(recording);
    else groups.push({ key, label: recordingDayHeading(recording.started_at), items: [recording] });
    return groups;
  }, []);

  app.innerHTML = `
    <main class="app-shell">
      <aside class="sidebar">
        ${isMacPlatform ? `<div class="sidebar-chrome-space" data-tauri-drag-region></div>` : ""}
        <div class="sidebar-brand" data-tauri-drag-region>
          <img src="${dictaAppIconUrl}" alt="" aria-hidden="true" data-tauri-drag-region />
          <strong data-tauri-drag-region>Dicta</strong>
        </div>
        <div class="sidebar-section-label">Projects</div>
        <nav class="project-list" aria-label="Projects">
          ${projects.length === 0 ? `<div class="sidebar-empty">No projects yet</div>` : projects.map((item) => `
            <div class="project-entry ${openProjectMenu === item.id ? "menu-open" : ""}">
              <button class="project-item ${activeView === "project" && item.id === selectedProjectId ? "selected" : ""}" data-project-id="${escapeHtml(item.id)}" ${isBusy ? "disabled" : ""}>
                <i class="ph ph-folder" aria-hidden="true"></i>
                <span class="project-label"><span>${escapeHtml(item.name)}</span><small>${escapeHtml(item.git_branch ?? (item.is_git ? "Git unavailable" : "Unlinked project"))}</small></span>
              </button>
              <button class="project-more" type="button" data-project-menu="${escapeHtml(item.id)}" aria-label="Project actions for ${escapeHtml(item.name)}" ${isBusy ? "disabled" : ""}><i class="ph ph-dots-three"></i></button>
              ${openProjectMenu === item.id ? `
                <div class="packet-menu project-menu">
                  <button data-project-reveal="${escapeHtml(item.source_path ?? item.storage_path)}"><i class="ph ph-folder-open"></i>${isMacPlatform ? "Reveal in Finder" : "Show in Files"}</button>
                  <button data-project-copy-path="${escapeHtml(item.path)}"><i class="ph ph-copy"></i>Copy path</button>
                  <span class="packet-menu-divider"></span>
                  <button class="danger" data-remove-project="${escapeHtml(item.id)}"><i class="ph ph-minus-circle"></i>Remove from Dicta</button>
                </div>` : ""}
            </div>
          `).join("")}
        </nav>
        <section class="sidebar-recents" aria-labelledby="recents-title">
          <div class="sidebar-section-label" id="recents-title">Recents</div>
          ${latestRecording ? `
            <button class="recent-item" data-open-packet="${escapeHtml(latestRecording.id)}" title="Open ${escapeHtml(latestRecording.note || "recent recording")}">
              <i class="ph ph-clock" aria-hidden="true"></i>
              <span>${escapeHtml(latestRecording.note || "Untitled recording")}</span>
            </button>
          ` : `
            <div class="recent-item recent-item-empty">
              <i class="ph ph-clock" aria-hidden="true"></i>
              <span>No recent recordings</span>
            </div>
          `}
        </section>
        <div class="sidebar-actions">
          <button class="add-project" id="new-project" ${isBusy ? "disabled" : ""}>
            <i class="ph ph-folder-plus" aria-hidden="true"></i><span>Link project folder</span>
          </button>
          <button class="sidebar-settings ${activeView === "settings" ? "selected" : ""}" id="open-settings" ${isBusy ? "disabled" : ""}>
            <i class="ph ph-gear-six" aria-hidden="true"></i><span>Settings</span>
          </button>
        </div>
      </aside>

      <section class="workspace">
        <header class="project-header" data-tauri-drag-region>
          <div class="project-heading" data-tauri-drag-region>
            <h1 data-tauri-drag-region>${escapeHtml(project?.name ?? "Choose a project")}</h1>
            <div class="project-context">
              <button class="path-button" id="copy-path" ${project ? "" : "disabled"} title="Copy working-copy path">
                <i class="ph ph-folder" aria-hidden="true"></i>
                <span>${project ? escapeHtml(compactPath(project.path)) : "Link a Git project to begin"}</span>
                ${project ? '<i class="ph ph-copy" aria-hidden="true"></i>' : ""}
              </button>
              ${project ? `<button class="branch-pill ${branchUnavailable ? "unavailable" : ""}" id="refresh-branch" title="Refresh current Git branch"><i class="ph ph-git-branch"></i><span>${escapeHtml(project.git_branch ?? "Git unavailable")}</span><i class="ph ph-arrows-clockwise refresh-icon"></i></button>` : ""}
            </div>
          </div>
          <div class="header-clock" aria-label="${escapeHtml(`${clock.date}, ${clock.time}`)}" data-tauri-drag-region>
            <span data-tauri-drag-region>${escapeHtml(clock.date)}</span><strong data-tauri-drag-region>${escapeHtml(clock.time)}</strong>
          </div>
          ${status.last_error || project?.git_error ? `<div class="error-banner"><i class="ph ph-warning-circle"></i><span>${escapeHtml(status.last_error ?? project?.git_error ?? "")}</span></div>` : ""}
        </header>

        <section class="packet-section" data-project-id="${escapeHtml(selectedProjectId ?? "")}">
          <div class="packet-title-row">
            <div class="packet-title-copy"><i class="ph ph-stack" aria-hidden="true"></i><h2>Prompt packets</h2><p>${recordings.length} recording${recordings.length === 1 ? "" : "s"}</p></div>
          </div>
          <div class="packet-table" role="table" aria-label="Prompt packets">
            <div class="packet-head" role="row">
              <span>Preview</span><span>Title</span><span>Duration</span><span>Recorded</span><span>Status</span><span></span>
            </div>
            <div class="packet-body">
              ${recordings.length === 0 ? `
                <div class="empty-state">
                  <i class="ph ph-monitor-play" aria-hidden="true"></i>
                  <h3>Your first explanation starts here</h3>
                  <p>Record your screen and voice. This packet will be saved only for ${escapeHtml(project?.git_branch ?? "the current Git branch")}.</p>
                  <button id="empty-record" ${buttonDisabled ? "disabled" : ""}>Record</button>
                </div>
              ` : recordingGroups.map((group) => `
                <div class="packet-group-heading"><span>${escapeHtml(group.label)}</span><i aria-hidden="true"></i></div>
                ${group.items.map((recording) => {
                  const packet = packetStatus(recording);
                  const poster = mediaSrc(recording.poster_path);
                  const excerpt = transcriptExcerpt(recording);
                  return `
                  <div class="packet-row" role="row" data-recording-id="${escapeHtml(recording.id)}">
                    <button class="thumbnail-button" data-open-packet="${escapeHtml(recording.id)}" aria-label="Play ${escapeHtml(recording.note || "recording")}">
                      ${poster ? `<img src="${escapeHtml(poster)}" alt="" />` : `<span class="thumbnail-fallback"><i class="ph ph-monitor-play" aria-hidden="true"></i></span>`}
                      <span class="play-overlay"><i class="ph ph-play" aria-hidden="true"></i></span>
                    </button>
                    <button class="packet-name" data-open-packet="${escapeHtml(recording.id)}">
                      <strong>${escapeHtml(recording.note || "Untitled recording")}</strong>
                      ${excerpt ? `<small>${escapeHtml(excerpt)}</small>` : recording.transcription_status === "complete" ? "" : `<small>${escapeHtml(packet.title)}</small>`}
                    </button>
                    <span class="packet-meta">${formatDuration(recording.duration_seconds)}</span>
                    <span class="packet-meta">${formatDate(recording.started_at)}</span>
                    <span><span class="status-chip ${packet.className}" title="${escapeHtml(packet.title)}"><i class="ph ${packet.icon}"></i>${packet.label}</span></span>
                    <div class="menu-cell">
                      <button class="more-button" data-menu="${escapeHtml(recording.id)}" aria-label="More actions"><i class="ph ph-dots-three"></i></button>
                      ${openPacketMenu === recording.id ? `
                        <div class="packet-menu">
                          <button data-open-packet="${escapeHtml(recording.id)}"><i class="ph ph-play"></i>Open</button>
                          <button data-transcribe="${escapeHtml(recording.id)}" ${recording.success ? "" : "disabled"}><i class="ph ph-waveform"></i>Transcribe</button>
                          <span class="packet-menu-divider"></span>
                          <button data-reveal="${escapeHtml(recording.video_path)}"><i class="ph ph-folder-open"></i>Reveal</button>
                          <button data-copy-video="${escapeHtml(recording.video_path)}"><i class="ph ph-copy"></i>Copy path</button>
                          <span class="packet-menu-divider"></span>
                          <button class="danger" data-delete="${escapeHtml(recording.id)}"><i class="ph ph-trash"></i>Delete</button>
                        </div>
                      ` : ""}
                    </div>
                  </div>`;
                }).join("")}
              `).join("")}
            </div>
          </div>
        </section>

        <div class="floating-record-zone">
          ${status.phase === "recording" ? `<span class="recording-live-time" id="record-time">${elapsed()}</span>` : ""}
          <div class="floating-record ${isBusy ? "is-busy" : ""} ${status.phase === "recording" ? "is-recording" : ""} ${buttonDisabled ? "is-disabled" : ""}">
            <span class="access-light access-light-mic is-ready" role="status" aria-label="Microphone access ready" title="Microphone access ready"></span>
            <button class="floating-record-button" id="record-toggle" type="button" aria-label="${status.phase === "recording" ? "Stop recording" : "Start screen and audio recording"}" title="${statusCopy()} · ${escapeHtml(shortcutLabel())}" ${buttonDisabled ? "disabled" : ""}>
              <span class="record-button-mark" aria-hidden="true"></span>
            </button>
            <span class="access-light access-light-folder ${project ? "is-ready" : "is-waiting"}" role="status" aria-label="${project ? "Project folder access ready" : "Link a project folder to record"}" title="${project ? "Project folder access ready" : "Link a project folder to record"}"></span>
            <button class="capture-options" id="record-options" type="button" aria-label="Open recording options" title="Recording options" ${buttonDisabled || status.phase === "recording" ? "disabled" : ""}><i class="ph ph-caret-down" aria-hidden="true"></i></button>
          </div>
        </div>

        <footer class="workspace-footer">
          <button class="footer-button" id="copy-context" ${recordings.length === 0 ? "disabled" : ""}><i class="ph ph-copy"></i>Context</button>
          <div class="footer-actions">
            <button class="footer-button mcp-button ${mcpStatus.codex_configured ? "connected" : ""}" id="connect-mcp" ${mcpRestarting ? "disabled" : ""}><i class="ph ${mcpRestarting ? "ph-spinner-gap mcp-spin" : mcpStatus.codex_configured ? "ph-arrow-clockwise" : "ph-plug"}"></i>${mcpRestarting ? "Restarting…" : mcpStatus.codex_configured ? "Restart" : "Connect"}</button>
            <button class="footer-button" id="reveal-project" ${project?.branch_path || project?.storage_path ? "" : "disabled"}><i class="ph ph-folder-open"></i>Reveal</button>
          </div>
        </footer>

        ${activeView === "settings" ? `
          <section class="settings-page" aria-labelledby="settings-title">
            <header class="settings-header">
              <div>
                <span class="settings-eyebrow">Dicta on this ${platformName}</span>
                <h1 id="settings-title">Settings</h1>
              </div>
              <button class="settings-close" id="close-settings" aria-label="Close settings"><i class="ph ph-x"></i></button>
            </header>

            <div class="settings-layout">
              <nav class="settings-nav" aria-label="Settings sections">
                <button class="${settingsSection === "appearance" ? "selected" : ""}" data-settings-section="appearance"><i class="ph ph-palette"></i><span>Appearance</span></button>
                <button class="${settingsSection === "shortcuts" ? "selected" : ""}" data-settings-section="shortcuts"><i class="ph ph-keyboard"></i><span>Shortcuts</span></button>
                <button class="${settingsSection === "transcription" ? "selected" : ""}" data-settings-section="transcription"><i class="ph ph-waveform"></i><span>Transcription</span></button>
                <button class="${settingsSection === "storage" ? "selected" : ""}" data-settings-section="storage"><i class="ph ph-hard-drives"></i><span>Storage</span></button>
              </nav>

              <div class="settings-content">
                <section class="settings-section-block" id="appearance-settings">
                  <div class="settings-content-heading">
                    <h2>Appearance</h2>
                    <p>Choose how Dicta looks on this ${platformName}.</p>
                  </div>
                  <div class="settings-group" aria-label="Theme">
                    <div class="settings-group-label">Theme</div>
                    <div class="theme-options" role="radiogroup" aria-label="Theme">
                      ${([
                        ["system", "ph-desktop", "System", isMacPlatform ? "Follow macOS" : "Follow Linux"],
                        ["light", "ph-sun", "Light", "Always light"],
                        ["dark", "ph-moon-stars", "Dark", "Always dark"],
                      ] as const).map(([value, icon, label, detail]) => `
                        <button class="theme-option ${themePreference === value ? "selected" : ""}" type="button" data-theme-choice="${value}" role="radio" aria-checked="${themePreference === value}">
                          <i class="ph ${icon}"></i><span><strong>${label}</strong><small>${detail}</small></span><i class="ph ${themePreference === value ? "ph-check-circle" : "ph-circle"}"></i>
                        </button>
                      `).join("")}
                    </div>
                  </div>
                </section>

                <section class="settings-section-block" id="shortcuts-settings">
                  <div class="settings-content-heading">
                    <h2>Shortcuts</h2>
                    <p>Choose the global shortcut that starts or stops a recording—even when Dicta is hidden.</p>
                  </div>
                  <div class="settings-group" aria-label="Recording shortcut">
                    <div class="settings-group-label">Record</div>
                    <div class="shortcut-options" role="radiogroup" aria-label="Record shortcut">
                      ${shortcutOptions.map((shortcut) => `
                        <button class="shortcut-option ${appSettings.shortcut_id === shortcut.id ? "selected" : ""}" type="button" data-shortcut-choice="${shortcut.id}" role="radio" aria-checked="${appSettings.shortcut_id === shortcut.id}">
                          <span><strong>${escapeHtml(shortcut.label)}</strong><small>${escapeHtml(shortcut.detail)}</small></span>
                          <i class="ph ${appSettings.shortcut_id === shortcut.id ? "ph-check-circle" : "ph-circle"}"></i>
                        </button>
                      `).join("")}
                    </div>
                    <p class="settings-help"><i class="ph ph-info"></i>${isMacPlatform ? "Double-Fn is reserved by macOS and cannot be registered reliably; these combinations work globally." : "Global shortcuts use Super, Alt, or Control and work while Dicta is in the background."}</p>
                  </div>
                </section>

                <section class="settings-section-block" id="transcription-settings">
                <div class="settings-content-heading">
                  <h2>Transcription</h2>
                  <p>Choose the spoken language and the local speech model Dicta uses to turn recordings into agent-readable context.</p>
                </div>

                <section class="settings-group" aria-label="Default spoken language">
                  <div class="settings-group-label">Default language</div>
                  <div class="language-picker settings-language-picker">
                    ${transcriptionLanguages.map((language) => `
                      <button type="button" class="language-option ${appSettings.transcription_language === language.code ? "selected" : ""}" data-default-language="${language.code}" role="radio" aria-checked="${appSettings.transcription_language === language.code}">
                        <span><strong>${language.label}</strong><small>${language.native}</small></span>
                        <i class="ph ${appSettings.transcription_language === language.code ? "ph-check-circle" : "ph-circle"}"></i>
                      </button>
                    `).join("")}
                  </div>
                  <p class="settings-help"><i class="ph ph-info"></i>Used for new recordings and for the local Whisper fallback. You can still pick another language when re-transcribing a packet.</p>
                </section>

                <section class="settings-group" aria-label="Transcription models">
                  <div class="settings-group-label">Models</div>
                  <article class="model-row model-row-featured">
                    <div class="model-icon"><i class="ph ph-sparkle"></i></div>
                    <div class="model-copy">
                      <div class="model-title-line">
                        <h3>High quality</h3>
                        <span class="recommend-badge">Recommended</span>
                        ${modelStatus.quality_installed ? '<span class="installed-badge"><i class="ph ph-check"></i>Installed</span>' : ""}
                      </div>
                      <p>Whisper large-v3-turbo Q5 delivers much better Dutch, names, and technical vocabulary.</p>
                      <div class="model-facts">
                        <span><i class="ph ph-hard-drives"></i>${formatBytes(modelStatus.download_size_bytes)}</span>
                        <span><i class="ph ph-lock-key"></i>Runs entirely on your ${platformName}</span>
                        <span><i class="ph ph-wifi-high"></i>Internet needed once</span>
                      </div>
                      ${modelDownloading || modelDownload ? `
                        <div class="download-state ${modelDownload?.status ?? "downloading"}" aria-live="polite">
                          <div class="download-state-label">
                            <span>${escapeHtml(modelDownload?.message ?? "Preparing download…")}</span>
                            <strong>${modelDownload?.status === "verifying" ? "Verifying" : modelDownload?.status === "complete" ? "Ready" : `${Math.round((modelDownload?.progress ?? 0) * 100)}%`}</strong>
                          </div>
                          <div class="download-track"><span style="width: ${Math.round((modelDownload?.progress ?? 0) * 100)}%"></span></div>
                          ${modelDownload?.status === "downloading" ? `<small>${formatBytes(modelDownload.downloaded_bytes)} of about ${formatBytes(modelDownload.total_bytes)}</small>` : ""}
                        </div>
                      ` : ""}
                    </div>
                    <button class="model-action ${modelStatus.quality_installed ? "installed" : ""}" id="download-model" ${modelDownloading || modelStatus.quality_installed ? "disabled" : ""}>
                      <i class="ph ${modelStatus.quality_installed ? "ph-check" : modelDownloading ? "ph-spinner-gap model-spin" : "ph-download-simple"}"></i>
                      ${modelStatus.quality_installed ? "Installed" : modelDownloading ? "Downloading…" : "Download model"}
                    </button>
                  </article>

                  <article class="model-row model-row-compact">
                    <div class="model-icon compact"><i class="ph ph-feather"></i></div>
                    <div class="model-copy">
                      <div class="model-title-line"><h3>Compact</h3><span class="included-badge">Included</span></div>
                      <p>Fast offline fallback for rough transcripts when the high-quality model is unavailable.</p>
                    </div>
                    <span class="model-size">57 MB</span>
                  </article>
                </section>

                <section class="settings-group current-engine" aria-label="Current transcription engine">
                  <div class="settings-group-label">Current engine</div>
                  <div class="engine-row">
                    <span class="engine-dot"></span>
                    <div><strong>${escapeHtml(modelStatus.active_model)}</strong><small>${escapeHtml(compactPath(modelStatus.active_model_path))}</small></div>
                    <span class="active-badge">Active</span>
                  </div>
                  <p class="engine-message">${escapeHtml(modelStatus.message)}</p>
                </section>

                <div class="privacy-note"><i class="ph ph-shield-check"></i><p><strong>Your recordings stay private.</strong> Dicta downloads the model directly, verifies it before installation, and transcribes locally.</p></div>
                </section>

                <section class="settings-section-block" id="storage-settings">
                  <div class="settings-content-heading">
                    <h2>Storage</h2>
                    <p>Keep prompt context useful while removing large files after a branch has landed.</p>
                  </div>
                  <div class="settings-group" aria-label="Merged branch cleanup">
                    <div class="settings-group-label">Cleanup</div>
                    <article class="preference-row">
                      <div class="preference-icon"><i class="ph ph-git-merge"></i></div>
                      <div class="preference-copy"><strong>Merged videos</strong><p>Delete only video files after Git confirms their branch tip is merged into the default branch. Transcripts, notes, and metadata stay available to agents.</p></div>
                      <button class="switch ${appSettings.cleanup_merged_videos ? "on" : ""}" type="button" id="cleanup-toggle" role="switch" aria-checked="${appSettings.cleanup_merged_videos}" aria-label="Clean merged branch videos"><span></span></button>
                    </article>
                    <div class="cleanup-action-row">
                      <div>${cleanupSummary ? `<strong>${escapeHtml(cleanupSummary.message)}</strong><small>${cleanupSummary.freed_bytes > 0 ? `${formatBytes(cleanupSummary.freed_bytes)} freed · ` : ""}${cleanupSummary.cleaned_branches.length > 0 ? cleanupSummary.cleaned_branches.map(escapeHtml).join(", ") : "Checks the selected project"}</small>` : `<strong>Manual</strong><small>Videos are removed only when you press Clean. Transcripts stay for agents.</small>`}</div>
                      <button class="secondary-action" id="cleanup-now" ${!selectedProjectId || !appSettings.cleanup_merged_videos || cleanupRunning ? "disabled" : ""}><i class="ph ${cleanupRunning ? "ph-spinner-gap mcp-spin" : "ph-broom"}"></i>${cleanupRunning ? "Checking…" : "Clean"}</button>
                    </div>
                  </div>
                </section>
              </div>
            </div>
          </section>
        ` : ""}
      </section>
    </main>

    ${createProjectOpen ? `
      <div class="modal-backdrop" data-close-modal>
        <form class="modal" id="create-project-form">
          <button class="modal-close" type="button" data-close-modal aria-label="Close"><i class="ph ph-x"></i></button>
          <i class="ph ph-folder-plus modal-icon"></i>
          <h2>Link Git project</h2>
          <p>In the desktop app this opens a native folder picker and detects the current branch.</p>
          <label>Demo folder name<input id="project-name" maxlength="64" placeholder="peepel" autofocus required /></label>
          <div class="modal-actions"><button type="button" class="secondary" data-close-modal>Cancel</button><button type="submit" class="primary">Link folder</button></div>
        </form>
      </div>
    ` : ""}

    ${startSheetOpen ? `
      <div class="modal-backdrop" data-close-start>
        <form class="modal record-sheet" id="start-recording-form">
          <button class="modal-close" type="button" data-close-start aria-label="Close"><i class="ph ph-x"></i></button>
          <span class="record-symbol sheet-symbol"><span></span></span>
          <h2>Start a prompt packet</h2>
          <p>Dicta will capture your main display, microphone, and system audio into <strong>${escapeHtml(project?.git_branch ?? "the current branch")}</strong>. Recordings stop automatically at 20 minutes.</p>
          <label>What should Codex understand? <span>Optional</span><textarea id="session-note" rows="3" placeholder="Authentication edge cases, webhook behavior…">${escapeHtml(sessionNote || lastSessionNote)}</textarea></label>
          <div class="source-summary"><span><i class="ph ph-monitor"></i>Main display</span><span><i class="ph ph-microphone"></i>Microphone</span><span><i class="ph ph-speaker-high"></i>System audio</span><span><i class="ph ph-timer"></i>20 min max</span></div>
          <div class="modal-actions"><button type="button" class="secondary" data-close-start>Cancel</button><button type="submit" class="primary record-primary">Record</button></div>
        </form>
      </div>
    ` : ""}

    ${transcribeRecordingId ? `
      <div class="modal-backdrop" data-close-transcribe>
        <form class="modal transcribe-sheet" id="retranscribe-form">
          <button class="modal-close" type="button" data-close-transcribe aria-label="Close"><i class="ph ph-x"></i></button>
          <div class="transcribe-heading-icon"><i class="ph ph-waveform"></i></div>
          <h2>Transcribe recording</h2>
          <p>Choose the language spoken in this packet. Dicta will replace its transcript while keeping the original video.</p>
          <fieldset class="language-picker">
            <legend>Spoken language</legend>
            ${transcriptionLanguages.map((language) => `
              <button type="button" class="language-option ${selectedTranscriptionLanguage === language.code ? "selected" : ""}" data-language="${language.code}" role="radio" aria-checked="${selectedTranscriptionLanguage === language.code}">
                <span><strong>${language.label}</strong><small>${language.native}</small></span>
                <i class="ph ${selectedTranscriptionLanguage === language.code ? "ph-check-circle" : "ph-circle"}"></i>
              </button>
            `).join("")}
          </fieldset>
          <div class="modal-actions"><button type="button" class="secondary" data-close-transcribe>Cancel</button><button type="submit" class="primary"><i class="ph ph-waveform"></i>Transcribe</button></div>
        </form>
      </div>
    ` : ""}

    ${recordingToDelete ? `
      <div class="modal-backdrop" data-close-delete>
        <form class="modal delete-sheet" id="delete-recording-form">
          <button class="modal-close" type="button" data-close-delete aria-label="Close"><i class="ph ph-x"></i></button>
          <div class="delete-icon"><i class="ph ph-trash"></i></div>
          <h2>Delete recording?</h2>
          <p>This removes its video, transcript, notes, and metadata from this branch. This cannot be undone.</p>
          <div class="delete-summary"><strong>${escapeHtml(recordingToDelete.note || "Untitled recording")}</strong><span>${formatDuration(recordingToDelete.duration_seconds)} · ${formatDate(recordingToDelete.started_at)}</span></div>
          <div class="modal-actions"><button type="button" class="secondary" data-close-delete>Cancel</button><button type="submit" class="danger">Delete</button></div>
        </form>
      </div>
    ` : ""}

    ${projectToRemove ? `
      <div class="modal-backdrop" data-close-remove-project>
        <form class="modal delete-sheet" id="remove-project-form">
          <button class="modal-close" type="button" data-close-remove-project aria-label="Close"><i class="ph ph-x"></i></button>
          <div class="delete-icon"><i class="ph ph-folder-minus"></i></div>
          <h2>Remove ${escapeHtml(projectToRemove.name)}?</h2>
          <p>This removes the project from Dicta only. ${projectToRemove.is_git ? "Your repository and its .dicta recordings stay exactly where they are." : "Its recordings and other files remain on disk."}</p>
          <div class="delete-summary"><strong>${escapeHtml(projectToRemove.name)}</strong><span>${escapeHtml(compactPath(projectToRemove.source_path ?? projectToRemove.storage_path))} · ${projectToRemove.recording_count} recording${projectToRemove.recording_count === 1 ? "" : "s"}</span></div>
          <div class="modal-actions"><button type="button" class="secondary" data-close-remove-project>Cancel</button><button type="submit" class="danger">Remove project</button></div>
        </form>
      </div>
    ` : ""}

    ${(() => {
      const viewing = recordings.find((recording) => recording.id === viewingRecordingId);
      if (!viewing) return "";
      const videoAsset = mediaSrc(viewing.video_path);
      const video = !isMacPlatform && viewerVideoBlobRecordingId === viewing.id
        ? viewerVideoBlobUrl
        : isMacPlatform ? videoAsset : null;
      const poster = mediaSrc(viewing.poster_path);
      const duration = Math.max(0, viewing.duration_seconds ?? 0);
      const timelineNotes = viewing.timeline_notes ?? [];
      return `
      <section class="packet-review" id="packet-viewer" tabindex="-1" aria-label="Video review">
        <header class="review-header">
          <div class="review-title">
            <button class="review-icon-button" type="button" data-close-viewer aria-label="Back to recordings"><i class="ph ph-arrow-left"></i></button>
            <div><h2>${escapeHtml(viewing.note || "Untitled recording")}</h2><p>${formatDate(viewing.started_at)} · ${formatDuration(viewing.duration_seconds)}</p></div>
          </div>
          <div class="review-header-actions">
            <span><i class="ph ph-note-pencil"></i>${timelineNotes.length} note${timelineNotes.length === 1 ? "" : "s"}</span>
            <button class="review-icon-button" id="review-fullscreen" type="button" aria-label="Enter full screen" title="Enter full screen"><i class="ph ph-corners-out"></i></button>
            <button class="review-done" type="button" data-close-viewer>Done</button>
          </div>
        </header>

        <div class="review-body">
          <div class="review-stage">
            <div class="viewer-media">
              ${videoAsset ? `<video id="packet-video" preload="metadata" playsinline${poster ? ` poster="${escapeHtml(poster)}"` : ""}>${video ? `<source src="${escapeHtml(video)}" type="video/mp4">` : ""}</video><div class="viewer-playback-error" id="viewer-playback-error" role="alert" hidden><i class="ph ph-warning-circle"></i><span>Video playback is unavailable.</span></div>` : `<div class="viewer-missing"><i class="ph ph-video-camera-slash"></i><span>Video file is missing.</span></div>`}
            </div>
            <div class="review-controls">
              <div class="review-control-row">
                <div class="playback-actions">
                  <button class="control-button" type="button" data-skip="-5" aria-label="Back 5 seconds"><i class="ph ph-arrow-counter-clockwise"></i><small>5</small></button>
                  <button class="play-button" id="viewer-play" type="button" aria-label="Play"><i class="ph ${viewerPaused ? "ph-play" : "ph-pause"}"></i></button>
                  <button class="control-button" type="button" data-skip="5" aria-label="Forward 5 seconds"><i class="ph ph-arrow-clockwise"></i><small>5</small></button>
                </div>
                <span class="viewer-clock"><strong id="viewer-current-time">${formatViewerTime(viewerTime)}</strong><span>/</span>${formatViewerTime(duration)}</span>
                <button class="mark-button" id="mark-timestamp" type="button"><i class="ph ph-map-pin-plus"></i>Mark ${formatViewerTime(viewerTime)}<kbd>M</kbd></button>
              </div>
              <div class="timeline-wrap">
                <input id="viewer-timeline" type="range" min="0" max="${duration || 1}" step="0.05" value="${Math.min(viewerTime, duration || 1)}" aria-label="Video timeline" />
                <div class="timeline-note-markers" aria-hidden="true">
                  ${duration > 0 ? timelineNotes.map((note) => `<button type="button" data-note-time="${note.timestamp_seconds}" style="left:${Math.min(100, Math.max(0, note.timestamp_seconds / duration * 100))}%" title="${escapeHtml(note.text)}"></button>`).join("") : ""}
                </div>
              </div>
            </div>
          </div>

          <aside class="review-sidebar">
            <div class="review-tabs" role="tablist">
              <button type="button" role="tab" aria-selected="${viewerPanel === "notes"}" class="${viewerPanel === "notes" ? "selected" : ""}" data-viewer-panel="notes">Notes <span>${timelineNotes.length}</span></button>
              <button type="button" role="tab" aria-selected="${viewerPanel === "transcript"}" class="${viewerPanel === "transcript" ? "selected" : ""}" data-viewer-panel="transcript">Transcript</button>
            </div>
            ${viewerPanel === "notes" ? `
              <div class="notes-panel">
                <form class="note-composer" id="timeline-note-form">
                  <div class="composer-heading">
                    <span class="composer-time"><i class="ph ph-map-pin"></i><strong id="marked-time">${formatViewerTime(viewerMarkedTime)}</strong></span>
                    <button type="button" id="use-current-time">Use current time</button>
                  </div>
                  <textarea id="timeline-note-input" rows="4" maxlength="2000" placeholder="What should an agent notice here?">${escapeHtml(viewerNoteDraft)}</textarea>
                  <div class="composer-actions">
                    <button class="dictate-button ${viewerListening ? "listening" : ""} ${viewerVoiceProcessing ? "processing" : ""}" id="dictate-note" type="button" title="Speak this note" ${viewerVoiceProcessing ? "disabled" : ""}><i class="ph ${viewerVoiceProcessing ? "ph-spinner-gap" : viewerListening ? "ph-stop-circle" : "ph-microphone"}"></i><span>${viewerVoiceProcessing ? "Transcribing…" : viewerListening ? "Listening…" : "Speak"}</span></button>
                    <button class="add-note-button" type="submit" ${viewerNoteDraft.trim() ? "" : "disabled"}>Add note</button>
                  </div>
                </form>
                <div class="timeline-notes" aria-live="polite">
                  ${timelineNotes.length ? timelineNotes.map((note) => `
                    <article class="timeline-note" data-note-time="${note.timestamp_seconds}">
                      <button class="note-jump" type="button" data-note-time="${note.timestamp_seconds}"><i class="ph ph-play"></i>${formatViewerTime(note.timestamp_seconds)}</button>
                      <div><p>${escapeHtml(note.text)}</p><small>${note.source === "voice" ? `<i class="ph ph-microphone"></i> Spoken note` : "Timestamp note"}</small></div>
                      <button class="note-delete" type="button" data-delete-note="${escapeHtml(note.id)}" aria-label="Delete note"><i class="ph ph-trash"></i></button>
                    </article>`).join("") : `<div class="notes-empty"><i class="ph ph-map-pin-line"></i><strong>No notes yet</strong><p>Mark a moment in the video, then type or speak what matters.</p></div>`}
                </div>
              </div>` : `
              <div class="viewer-transcript">
                ${viewing.transcript_segments?.length
                  ? `<div class="transcript-segments">${viewing.transcript_segments.map((segment) => `
                      <article class="transcript-segment">
                        <button type="button" data-transcript-time="${segment.start_seconds}" title="Jump to ${formatViewerTime(segment.start_seconds)}"><i class="ph ph-play"></i>${formatViewerTime(segment.start_seconds)}</button>
                        <div><p>${escapeHtml(segment.text)}</p><small>${formatViewerTime(segment.start_seconds)}–${formatViewerTime(segment.end_seconds)}</small></div>
                      </article>`).join("")}</div>`
                  : viewing.transcript?.trim()
                    ? `<pre>${escapeHtml(viewing.transcript)}</pre><p class="legacy-transcript-note">Retranscribe this recording to add exact timestamps.</p>`
                  : `<div class="notes-empty"><i class="ph ph-waveform"></i><strong>No transcript yet</strong><p>${escapeHtml(viewing.transcription_error || (viewing.transcription_status === "processing" ? "The transcript is still being written." : "Transcribe this recording to read it here."))}</p></div>`}
              </div>`}
          </aside>
        </div>
      </section>`;
    })()}

    ${toastMessage ? `<div class="toast"><i class="ph ph-check-circle"></i>${escapeHtml(toastMessage)}</div>` : ""}
  `;

  bindEvents();
  const packetSection = document.querySelector<HTMLElement>(".packet-section");
  if (packetSection) packetSection.scrollTop = packetScrollTop;
  restoreViewer();
  if (isBusy) {
    elapsedTimer = window.setInterval(() => {
      const time = document.querySelector("#record-time");
      if (time) time.textContent = elapsed();
    }, 500);
  }
}

function restoreViewer(): void {
  document.querySelector<HTMLElement>("#packet-viewer")?.focus({ preventScroll: true });
  const video = document.querySelector<HTMLVideoElement>("#packet-video");
  if (!video) return;
  const updateControls = () => {
    viewerTime = video.currentTime;
    const timeline = document.querySelector<HTMLInputElement>("#viewer-timeline");
    const currentTime = document.querySelector<HTMLElement>("#viewer-current-time");
    const markButton = document.querySelector<HTMLButtonElement>("#mark-timestamp");
    if (timeline) {
      if (video.duration && Number.isFinite(video.duration)) timeline.max = String(video.duration);
      timeline.value = String(video.currentTime);
    }
    if (currentTime) currentTime.textContent = formatViewerTime(video.currentTime);
    if (markButton) markButton.childNodes.forEach((node) => {
      if (node.nodeType === Node.TEXT_NODE && node.textContent?.includes("Mark")) node.textContent = `Mark ${formatViewerTime(video.currentTime)}`;
    });
  };
  const updatePlayButton = () => {
    const playButton = document.querySelector<HTMLButtonElement>("#viewer-play");
    const icon = playButton?.querySelector("i");
    if (!playButton || !icon) return;
    playButton.ariaLabel = video.paused ? "Play" : "Pause";
    icon.className = `ph ${video.paused ? "ph-play" : "ph-pause"}`;
  };
  const showPlaybackError = (message: string) => {
    const error = document.querySelector<HTMLElement>("#viewer-playback-error");
    const label = error?.querySelector<HTMLElement>("span");
    if (label) label.textContent = message;
    if (error) error.hidden = false;
  };
  const clearPlaybackError = () => {
    const error = document.querySelector<HTMLElement>("#viewer-playback-error");
    if (error) error.hidden = true;
  };
  const apply = () => {
    if (Math.abs(video.currentTime - viewerTime) > 0.35) video.currentTime = viewerTime;
    if (!viewerPaused) void playViewerVideo(video);
    updateControls();
    updatePlayButton();
  };
  if (video.readyState >= 1) apply();
  else video.addEventListener("loadedmetadata", apply, { once: true });
  video.addEventListener("timeupdate", updateControls);
  video.addEventListener("loadeddata", clearPlaybackError);
  video.addEventListener("play", () => { viewerPaused = false; clearPlaybackError(); updatePlayButton(); });
  video.addEventListener("pause", () => { viewerPaused = true; updatePlayButton(); });
  video.addEventListener("ended", updatePlayButton);
  video.addEventListener("error", () => {
    viewerPaused = true;
    updatePlayButton();
    const mediaError = video.error;
    showPlaybackError(mediaError?.message
      ? `This recording could not be loaded: ${mediaError.message}`
      : "This recording could not be loaded.");
  });
  const viewing = recordings.find((recording) => recording.id === viewingRecordingId);
  if (!isMacPlatform && viewing) void loadLinuxViewerVideo(viewing, showPlaybackError);
}

function releaseViewerVideoBlob(): void {
  if (viewerVideoBlobUrl) URL.revokeObjectURL(viewerVideoBlobUrl);
  viewerVideoBlobUrl = null;
  viewerVideoBlobRecordingId = null;
}

async function loadLinuxViewerVideo(recording: Recording, showPlaybackError: (message: string) => void): Promise<void> {
  if (viewerVideoBlobRecordingId === recording.id && viewerVideoBlobUrl) return;
  if (!viewerVideoBlobLoad || viewerVideoBlobLoad.recordingId !== recording.id) {
    const promise = fetch(mediaSrc(recording.video_path)).then(async (response) => {
      if (!response.ok) throw new Error(`media request returned ${response.status}`);
      const blob = await response.blob();
      return URL.createObjectURL(blob.type === "video/mp4" ? blob : new Blob([blob], { type: "video/mp4" }));
    });
    viewerVideoBlobLoad = { recordingId: recording.id, promise };
  }

  const activeLoad = viewerVideoBlobLoad;
  try {
    const url = await activeLoad.promise;
    if (viewingRecordingId !== recording.id) {
      URL.revokeObjectURL(url);
      return;
    }
    if (viewerVideoBlobRecordingId === recording.id && viewerVideoBlobUrl) {
      const video = document.querySelector<HTMLVideoElement>("#packet-video");
      if (video && video.currentSrc !== viewerVideoBlobUrl) {
        video.replaceChildren();
        video.src = viewerVideoBlobUrl;
        video.load();
      }
      return;
    }
    releaseViewerVideoBlob();
    viewerVideoBlobUrl = url;
    viewerVideoBlobRecordingId = recording.id;
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (video) {
      video.replaceChildren();
      video.src = url;
      video.load();
    }
  } catch (error) {
    showPlaybackError(`This recording could not be loaded: ${String(error)}`);
  } finally {
    if (viewerVideoBlobLoad === activeLoad) viewerVideoBlobLoad = null;
  }
}

async function playViewerVideo(video: HTMLVideoElement): Promise<void> {
  try {
    await video.play();
  } catch (error) {
    viewerPaused = true;
    const playButton = document.querySelector<HTMLButtonElement>("#viewer-play");
    const icon = playButton?.querySelector("i");
    if (playButton) playButton.ariaLabel = "Play";
    if (icon) icon.className = "ph ph-play";
    const playbackError = document.querySelector<HTMLElement>("#viewer-playback-error");
    const label = playbackError?.querySelector<HTMLElement>("span");
    if (label) label.textContent = "Playback was blocked. Press play to try again.";
    if (playbackError) playbackError.hidden = false;
    console.error("Could not play recording", error);
  }
}

function openPacket(recordingId: string): void {
  if (viewerVideoBlobRecordingId !== recordingId) releaseViewerVideoBlob();
  viewingRecordingId = recordingId;
  viewerTime = 0;
  viewerPaused = true;
  viewerMarkedTime = 0;
  viewerNoteDraft = "";
  viewerNoteSource = "typed";
  viewerPanel = "notes";
  openPacketMenu = null;
  render();
}

function closePacketViewer(): void {
  activeSpeechRecognition?.abort();
  if (activeVoiceRecorder?.state === "recording") activeVoiceRecorder.stop();
  if (voiceStopTimer !== null) window.clearTimeout(voiceStopTimer);
  voiceStopTimer = null;
  activeVoiceStream?.getTracks().forEach((track) => track.stop());
  activeSpeechRecognition = null;
  activeVoiceRecorder = null;
  activeVoiceStream = null;
  viewerListening = false;
  viewerVoiceProcessing = false;
  releaseViewerVideoBlob();
  viewingRecordingId = null;
  viewerTime = 0;
  viewerPaused = true;
  viewerMarkedTime = 0;
  viewerNoteDraft = "";
  render();
}

function setMarkedTime(time: number, focusComposer = true): void {
  const video = document.querySelector<HTMLVideoElement>("#packet-video");
  const duration = video?.duration && Number.isFinite(video.duration) ? video.duration : Number.POSITIVE_INFINITY;
  viewerMarkedTime = Math.max(0, Math.min(time, duration));
  const markedTime = document.querySelector<HTMLElement>("#marked-time");
  if (markedTime) markedTime.textContent = formatViewerTime(viewerMarkedTime);
  if (focusComposer) {
    video?.pause();
    document.querySelector<HTMLTextAreaElement>("#timeline-note-input")?.focus();
  }
}

async function persistTimelineNotes(recording: Recording, notes: TimelineNote[]): Promise<void> {
  const updated = isTauri
    ? await invoke<Recording>("save_timeline_notes", { projectId: recording.project_id, recordingId: recording.id, timelineNotes: notes })
    : { ...recording, timeline_notes: [...notes].sort((left, right) => left.timestamp_seconds - right.timestamp_seconds) };
  recordings = recordings.map((item) => item.id === updated.id ? updated : item);
}

function updateVoiceButton(): void {
  const button = document.querySelector<HTMLButtonElement>("#dictate-note");
  if (!button) return;
  button.classList.toggle("listening", viewerListening);
  button.classList.toggle("processing", viewerVoiceProcessing);
  button.disabled = viewerVoiceProcessing;
  const icon = button.querySelector("i");
  const label = button.querySelector("span");
  if (icon) icon.className = `ph ${viewerVoiceProcessing ? "ph-spinner-gap" : viewerListening ? "ph-stop-circle" : "ph-microphone"}`;
  if (label) label.textContent = viewerVoiceProcessing ? "Transcribing…" : viewerListening ? "Listening…" : "Speak";
}

async function startOfflineVoiceNote(): Promise<void> {
  try {
    activeVoiceStream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true }, video: false });
    const preferredTypes = ["audio/mp4", "audio/webm;codecs=opus", "audio/webm"];
    const mimeType = preferredTypes.find((type) => MediaRecorder.isTypeSupported(type));
    activeVoiceRecorder = mimeType ? new MediaRecorder(activeVoiceStream, { mimeType }) : new MediaRecorder(activeVoiceStream);
    const recordedMimeType = activeVoiceRecorder.mimeType || mimeType || "audio/mp4";
    voiceChunks = [];
    activeVoiceRecorder.addEventListener("dataavailable", (event) => {
      if (event.data.size) voiceChunks.push(event.data);
    });
    activeVoiceRecorder.addEventListener("stop", async () => {
      if (voiceStopTimer !== null) window.clearTimeout(voiceStopTimer);
      voiceStopTimer = null;
      viewerListening = false;
      viewerVoiceProcessing = true;
      updateVoiceButton();
      activeVoiceStream?.getTracks().forEach((track) => track.stop());
      activeVoiceStream = null;
      const blob = new Blob(voiceChunks, { type: recordedMimeType });
      activeVoiceRecorder = null;
      voiceChunks = [];
      if (!viewingRecordingId) {
        viewerVoiceProcessing = false;
        return;
      }
      try {
        const audioBytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
        const transcript = await invoke<string>("transcribe_voice_note", {
          audioBytes,
          mimeType: blob.type,
          language: appSettings.transcription_language,
        });
        viewerNoteDraft = [viewerNoteDraft.trim(), transcript.trim()].filter(Boolean).join(" ");
        viewerNoteSource = "voice";
        const input = document.querySelector<HTMLTextAreaElement>("#timeline-note-input");
        const submit = document.querySelector<HTMLButtonElement>(".add-note-button");
        if (input) input.value = viewerNoteDraft;
        if (submit) submit.disabled = !viewerNoteDraft;
      } catch (error) {
        showToast(`Could not transcribe voice note: ${String(error)}`);
      } finally {
        viewerVoiceProcessing = false;
        updateVoiceButton();
      }
    }, { once: true });
    viewerListening = true;
    updateVoiceButton();
    activeVoiceRecorder.start(250);
    voiceStopTimer = window.setTimeout(() => {
      if (activeVoiceRecorder?.state === "recording") activeVoiceRecorder.stop();
    }, 60_000);
  } catch (error) {
    activeVoiceStream?.getTracks().forEach((track) => track.stop());
    activeVoiceStream = null;
    activeVoiceRecorder = null;
    viewerListening = false;
    showToast(`Microphone unavailable: ${String(error)}`);
  }
}

function startBrowserDictation(): void {
  if (activeSpeechRecognition) {
    activeSpeechRecognition.stop();
    return;
  }
  const speechWindow = window as unknown as { SpeechRecognition?: SpeechRecognitionConstructor; webkitSpeechRecognition?: SpeechRecognitionConstructor };
  const Recognition = speechWindow.SpeechRecognition ?? speechWindow.webkitSpeechRecognition;
  if (!Recognition) {
    showToast(`Voice dictation is unavailable on this ${platformName}`);
    return;
  }

  const recognition = new Recognition();
  const baseDraft = viewerNoteDraft.trim();
  recognition.continuous = true;
  recognition.interimResults = true;
  recognition.lang = appSettings.transcription_language === "auto" ? navigator.language : appSettings.transcription_language;
  recognition.onresult = (event) => {
    let finalText = "";
    let interimText = "";
    for (let index = 0; index < event.results.length; index += 1) {
      const result = event.results[index];
      if (result.isFinal) finalText += result[0].transcript;
      else interimText += result[0].transcript;
    }
    viewerNoteDraft = [baseDraft, finalText, interimText].filter(Boolean).join(" ").trim();
    viewerNoteSource = "voice";
    const input = document.querySelector<HTMLTextAreaElement>("#timeline-note-input");
    const submit = document.querySelector<HTMLButtonElement>(".add-note-button");
    if (input) input.value = viewerNoteDraft;
    if (submit) submit.disabled = !viewerNoteDraft;
  };
  recognition.onerror = () => {
    viewerListening = false;
    updateVoiceButton();
  };
  recognition.onend = () => {
    viewerListening = false;
    activeSpeechRecognition = null;
    updateVoiceButton();
  };
  activeSpeechRecognition = recognition;
  viewerListening = true;
  updateVoiceButton();
  recognition.start();
}

function toggleVoiceNote(): void {
  if (activeVoiceRecorder?.state === "recording") {
    activeVoiceRecorder.stop();
    return;
  }
  if (activeSpeechRecognition) {
    activeSpeechRecognition.stop();
    return;
  }
  if (isTauri && typeof MediaRecorder !== "undefined") {
    void startOfflineVoiceNote();
  } else {
    startBrowserDictation();
  }
}

function stopPropagationOnModal(event: Event): void {
  event.stopPropagation();
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>(".project-item").forEach((button) => button.addEventListener("click", async () => {
    activeView = "project";
    selectedProjectId = button.dataset.projectId ?? null;
    status.active_project_id = selectedProjectId;
    if (isTauri) {
      await invoke("select_project", { projectId: selectedProjectId });
      await refreshActiveProject();
    }
    await refreshRecordings();
    openPacketMenu = null;
    openProjectMenu = null;
    render();
  }));

  document.querySelectorAll<HTMLButtonElement>("[data-project-menu]").forEach((button) => button.addEventListener("click", (event) => {
    event.stopPropagation();
    openProjectMenu = openProjectMenu === button.dataset.projectMenu ? null : button.dataset.projectMenu ?? null;
    openPacketMenu = null;
    render();
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-project-reveal]").forEach((button) => button.addEventListener("click", () => {
    openProjectMenu = null;
    reveal(button.dataset.projectReveal);
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-project-copy-path]").forEach((button) => button.addEventListener("click", async () => {
    await copyText(button.dataset.projectCopyPath ?? "");
    openProjectMenu = null;
    showToast("Project path copied");
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-remove-project]").forEach((button) => button.addEventListener("click", () => {
    removeProjectId = button.dataset.removeProject ?? null;
    openProjectMenu = null;
    render();
  }));
  document.querySelector("#remove-project-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelectorAll("[data-close-remove-project]").forEach((node) => node.addEventListener("click", () => { removeProjectId = null; render(); }));
  document.querySelector("#remove-project-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!removeProjectId) return;
    const projectId = removeProjectId;
    const removedIndex = projects.findIndex((item) => item.id === projectId);
    removeProjectId = null;
    try {
      if (isTauri) await invoke("remove_project", { projectId });
      projects = projects.filter((item) => item.id !== projectId);
      if (selectedProjectId === projectId) {
        const fallback = projects[Math.min(removedIndex, projects.length - 1)] ?? null;
        selectedProjectId = fallback?.id ?? null;
        status.active_project_id = selectedProjectId;
        if (isTauri) await invoke("select_project", { projectId: selectedProjectId });
        await refreshRecordings();
      }
      render();
      showToast("Project removed from Dicta");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });

  document.querySelector("#new-project")?.addEventListener("click", async () => {
    activeView = "project";
    openProjectMenu = null;
    if (isTauri) await linkProjectFolder();
    else { createProjectOpen = true; render(); }
  });
  document.querySelector("#open-settings")?.addEventListener("click", () => {
    activeView = "settings";
    settingsSection = "appearance";
    openPacketMenu = null;
    openProjectMenu = null;
    render();
  });
  document.querySelector("#close-settings")?.addEventListener("click", () => {
    activeView = "project";
    render();
  });
  document.querySelector("#download-model")?.addEventListener("click", () => { void downloadQualityModel(); });
  document.querySelectorAll<HTMLButtonElement>("[data-settings-section]").forEach((button) => button.addEventListener("click", () => {
    const section = button.dataset.settingsSection;
    settingsSection = section === "shortcuts" || section === "transcription" || section === "storage" ? section : "appearance";
    render();
    window.requestAnimationFrame(() => document.querySelector(`#${settingsSection}-settings`)?.scrollIntoView({ behavior: "smooth", block: "start" }));
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach((button) => button.addEventListener("click", () => {
    setTheme((button.dataset.themeChoice ?? "system") as ThemePreference);
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-shortcut-choice]").forEach((button) => button.addEventListener("click", async () => {
    const shortcutId = button.dataset.shortcutChoice;
    if (!shortcutId || shortcutId === appSettings.shortcut_id) return;
    try {
      appSettings = isTauri
        ? await invoke<AppSettings>("set_shortcut", { shortcutId })
        : { ...appSettings, shortcut_id: shortcutId };
      render();
      showToast(`Shortcut set to ${shortcutLabel()}`);
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-default-language]").forEach((button) => button.addEventListener("click", async () => {
    const language = button.dataset.defaultLanguage;
    if (!language || language === appSettings.transcription_language) return;
    try {
      appSettings = isTauri
        ? await invoke<AppSettings>("set_transcription_language", { language })
        : { ...appSettings, transcription_language: language };
      selectedTranscriptionLanguage = language;
      render();
      showToast(`Default language set to ${transcriptionLanguages.find((item) => item.code === language)?.label ?? language}`);
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  }));
  document.querySelector("#cleanup-toggle")?.addEventListener("click", async () => {
    const enabled = !appSettings.cleanup_merged_videos;
    try {
      appSettings = isTauri
        ? await invoke<AppSettings>("set_cleanup_merged_videos", { enabled })
        : { ...appSettings, cleanup_merged_videos: enabled };
      cleanupSummary = null;
      render();
      showToast(enabled ? "Cleanup enabled" : "Cleanup disabled");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#cleanup-now")?.addEventListener("click", async () => {
    if (!selectedProjectId || cleanupRunning) return;
    cleanupRunning = true;
    render();
    try {
      cleanupSummary = isTauri
        ? await invoke<CleanupSummary>("cleanup_merged_videos", { projectId: selectedProjectId })
        : { removed_files: 2, freed_bytes: 148_000_000, cleaned_branches: ["feature/oauth"], default_branch: "main", message: "Removed 2 merged videos." };
      cleanupRunning = false;
      render();
      showToast(cleanupSummary.message);
    } catch (error) {
      cleanupRunning = false;
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#create-project-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelector("#create-project-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = document.querySelector<HTMLInputElement>("#project-name")!.value.trim();
    if (!name) return;
    const project = mockCreateProject(name);
    projects = [project, ...projects];
    selectedProjectId = project.id;
    status.active_project_id = project.id;
    recordings = [];
    createProjectOpen = false;
    showToast("Git project linked");
  });
  document.querySelectorAll("[data-close-modal]").forEach((node) => node.addEventListener("click", () => { createProjectOpen = false; render(); }));

  const openStart = () => {
    if (!activeProject()) return;
    if (!sessionNote) sessionNote = lastSessionNote;
    startSheetOpen = true;
    render();
  };
  document.querySelector("#empty-record")?.addEventListener("click", openStart);
  document.querySelector("#record-toggle")?.addEventListener("click", async () => {
    if (status.phase === "recording") await stopRecording(); else openStart();
  });
  document.querySelector("#record-options")?.addEventListener("click", openStart);
  document.querySelector("#start-recording-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelector<HTMLTextAreaElement>("#session-note")?.addEventListener("input", (event) => {
    sessionNote = (event.target as HTMLTextAreaElement).value;
  });
  document.querySelector("#start-recording-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const note = document.querySelector<HTMLTextAreaElement>("#session-note")?.value.trim() ?? "";
    sessionNote = note;
    lastSessionNote = note;
    startSheetOpen = false;
    await startRecording(note);
  });
  document.querySelectorAll("[data-close-start]").forEach((node) => node.addEventListener("click", () => { startSheetOpen = false; render(); }));

  document.querySelector("#copy-path")?.addEventListener("click", async () => {
    const project = activeProject(); if (!project) return;
    await copyText(project.path); showToast("Project path copied");
  });
  document.querySelector("#refresh-branch")?.addEventListener("click", async () => {
    await refreshActiveProject(true);
  });
  document.querySelector("#copy-context")?.addEventListener("click", async () => {
    if (!selectedProjectId) return;
    const context = isTauri ? await invoke<string>("build_context", { projectId: selectedProjectId }) : mockContext();
    await copyText(context); showToast("Project context copied");
  });
  document.querySelector("#reveal-project")?.addEventListener("click", () => {
    const project = activeProject();
    reveal(project?.branch_path ?? project?.storage_path);
  });
  document.querySelector("#connect-mcp")?.addEventListener("click", async () => {
    try {
      mcpRestarting = mcpStatus.codex_configured;
      if (mcpRestarting) render();
      mcpStatus = isTauri
        ? await invoke<McpStatus>(mcpStatus.codex_configured ? "restart_codex_mcp" : "configure_codex_mcp")
        : { installed: true, codex_configured: true, executable_path: isMacPlatform ? "/Library/Application Support/Dicta/bin/dicta-mcp" : "~/.local/share/Dicta/bin/dicta-mcp", message: mcpRestarting ? "Dicta MCP restarted." : "Dicta is connected." };
      mcpRestarting = false;
      render();
      showToast(mcpStatus.message);
    } catch (error) {
      mcpRestarting = false;
      status.last_error = String(error);
      render();
    }
  });
  document.querySelectorAll<HTMLElement>("[data-open-packet]").forEach((node) => node.addEventListener("click", (event) => {
    event.stopPropagation();
    const recordingId = node.dataset.openPacket;
    if (recordingId) openPacket(recordingId);
  }));
  document.querySelector("#packet-viewer")?.addEventListener("click", stopPropagationOnModal);
  document.querySelectorAll("[data-close-viewer]").forEach((node) => node.addEventListener("click", closePacketViewer));
  document.querySelector("#review-fullscreen")?.addEventListener("click", async () => {
    const viewer = document.querySelector<HTMLElement>("#packet-viewer");
    if (!viewer) return;
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await viewer.requestFullscreen();
    } catch {
      showToast("Full screen is unavailable");
    }
  });
  document.querySelector("#viewer-play")?.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    if (video.paused) void playViewerVideo(video); else video.pause();
  });
  document.querySelectorAll<HTMLButtonElement>("[data-skip]").forEach((button) => button.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    video.currentTime = Math.max(0, Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + Number(button.dataset.skip ?? 0)));
  }));
  document.querySelector<HTMLInputElement>("#viewer-timeline")?.addEventListener("input", (event) => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (video) video.currentTime = Number((event.target as HTMLInputElement).value);
  });
  document.querySelectorAll<HTMLButtonElement>("button[data-note-time]").forEach((button) => button.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    video.currentTime = Number(button.dataset.noteTime ?? 0);
    void playViewerVideo(video);
  }));
  document.querySelectorAll<HTMLButtonElement>("button[data-transcript-time]").forEach((button) => button.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    video.currentTime = Number(button.dataset.transcriptTime ?? 0);
    void playViewerVideo(video);
  }));
  document.querySelector("#mark-timestamp")?.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    setMarkedTime(video?.currentTime ?? viewerTime);
  });
  document.querySelector("#use-current-time")?.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    setMarkedTime(video?.currentTime ?? viewerTime, false);
  });
  document.querySelectorAll<HTMLButtonElement>("[data-viewer-panel]").forEach((button) => button.addEventListener("click", () => {
    viewerPanel = button.dataset.viewerPanel === "transcript" ? "transcript" : "notes";
    render();
  }));
  document.querySelector<HTMLTextAreaElement>("#timeline-note-input")?.addEventListener("input", (event) => {
    viewerNoteDraft = (event.target as HTMLTextAreaElement).value;
    if (!viewerListening) viewerNoteSource = "typed";
    const submit = document.querySelector<HTMLButtonElement>(".add-note-button");
    if (submit) submit.disabled = !viewerNoteDraft.trim();
  });
  document.querySelector("#dictate-note")?.addEventListener("click", toggleVoiceNote);
  document.querySelector("#timeline-note-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const recording = recordings.find((item) => item.id === viewingRecordingId);
    const text = viewerNoteDraft.trim();
    if (!recording || !text) return;
    activeSpeechRecognition?.stop();
    const note: TimelineNote = {
      id: globalThis.crypto?.randomUUID?.() ?? `note-${Date.now()}`,
      timestamp_seconds: viewerMarkedTime,
      text,
      created_at: new Date().toISOString(),
      source: viewerNoteSource,
    };
    try {
      await persistTimelineNotes(recording, [...(recording.timeline_notes ?? []), note]);
      viewerNoteDraft = "";
      viewerNoteSource = "typed";
      render();
      showToast(`Note added at ${formatViewerTime(note.timestamp_seconds)}`);
    } catch (error) {
      showToast(`Could not save note: ${String(error)}`);
    }
  });
  document.querySelectorAll<HTMLButtonElement>("[data-delete-note]").forEach((button) => button.addEventListener("click", async () => {
    const recording = recordings.find((item) => item.id === viewingRecordingId);
    if (!recording) return;
    try {
      await persistTimelineNotes(recording, (recording.timeline_notes ?? []).filter((note) => note.id !== button.dataset.deleteNote));
      render();
      showToast("Timeline note deleted");
    } catch (error) {
      showToast(`Could not delete note: ${String(error)}`);
    }
  }));
  document.querySelector<HTMLElement>("#packet-viewer")?.addEventListener("keydown", (event) => {
    const keyboardEvent = event as KeyboardEvent;
    const target = keyboardEvent.target as HTMLElement;
    const editing = target.matches("input, textarea, button");
    if (keyboardEvent.key === "Escape") closePacketViewer();
    if (editing) return;
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    if (keyboardEvent.key === " ") { keyboardEvent.preventDefault(); if (video.paused) void playViewerVideo(video); else video.pause(); }
    if (keyboardEvent.key.toLowerCase() === "m") { keyboardEvent.preventDefault(); setMarkedTime(video.currentTime); }
    if (keyboardEvent.key === "ArrowLeft") video.currentTime = Math.max(0, video.currentTime - 5);
    if (keyboardEvent.key === "ArrowRight") video.currentTime = Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + 5);
  });
  document.querySelectorAll<HTMLElement>("[data-reveal]").forEach((node) => node.addEventListener("click", () => reveal(node.dataset.reveal)));
  document.querySelectorAll<HTMLButtonElement>("[data-menu]").forEach((button) => button.addEventListener("click", (event) => {
    event.stopPropagation(); openPacketMenu = openPacketMenu === button.dataset.menu ? null : button.dataset.menu ?? null; render();
  }));
  document.querySelectorAll<HTMLElement>("[data-copy-video]").forEach((node) => node.addEventListener("click", async () => {
    await copyText(node.dataset.copyVideo ?? ""); openPacketMenu = null; showToast("Video path copied");
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-transcribe]").forEach((button) => button.addEventListener("click", () => {
    const recording = recordings.find((item) => item.id === button.dataset.transcribe);
    if (!recording) return;
    transcribeRecordingId = recording.id;
    selectedTranscriptionLanguage = recording.transcription_language ?? appSettings.transcription_language;
    openPacketMenu = null;
    render();
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-delete]").forEach((button) => button.addEventListener("click", () => {
    deleteRecordingId = button.dataset.delete ?? null;
    openPacketMenu = null;
    render();
  }));
  document.querySelector("#delete-recording-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelectorAll("[data-close-delete]").forEach((node) => node.addEventListener("click", () => { deleteRecordingId = null; render(); }));
  document.querySelector("#delete-recording-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!selectedProjectId || !deleteRecordingId) return;
    const recordingId = deleteRecordingId;
    deleteRecordingId = null;
    try {
      if (isTauri) {
        await invoke("delete_recording", { projectId: selectedProjectId, recordingId });
        await refreshRecordings();
        const updated = await invoke<Bootstrap>("bootstrap");
        projects = updated.projects;
      } else {
        recordings = recordings.filter((recording) => recording.id !== recordingId);
        projects = projects.map((project) => project.id === selectedProjectId ? { ...project, recording_count: recordings.length } : project);
      }
      render();
      showToast("Recording deleted");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#retranscribe-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelectorAll<HTMLButtonElement>("[data-language]").forEach((button) => button.addEventListener("click", () => {
    selectedTranscriptionLanguage = button.dataset.language ?? appSettings.transcription_language;
    render();
  }));
  document.querySelectorAll("[data-close-transcribe]").forEach((node) => node.addEventListener("click", () => { transcribeRecordingId = null; render(); }));
  document.querySelector("#retranscribe-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!selectedProjectId || !transcribeRecordingId) return;
    const recordingId = transcribeRecordingId;
    transcribeRecordingId = null;
    try {
      if (isTauri) await invoke("retranscribe_recording", { projectId: selectedProjectId, recordingId, language: selectedTranscriptionLanguage });
      await refreshRecordings();
      render();
      showToast(`Transcribing in ${transcriptionLanguages.find((item) => item.code === selectedTranscriptionLanguage)?.label ?? selectedTranscriptionLanguage}`);
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
}

async function copyText(text: string): Promise<void> {
  if (isTauri) await invoke("copy_to_clipboard", { text }); else await navigator.clipboard.writeText(text);
}

async function linkProjectFolder(): Promise<void> {
  try {
    const selected = await open({ directory: true, multiple: false, title: "Link a Git project" });
    if (typeof selected !== "string") return;
    const project = await invoke<Project>("link_project", { sourcePath: selected });
    projects = [project, ...projects.filter((item) => item.id !== project.id)];
    selectedProjectId = project.id;
    status.active_project_id = project.id;
    status.last_error = null;
    await refreshRecordings();
    showToast(`${project.name} linked on ${project.git_branch ?? "Git"}`);
  } catch (error) {
    status.last_error = String(error);
    render();
  }
}

async function refreshActiveProject(showFeedback = false): Promise<void> {
  if (!selectedProjectId || ["preparing", "recording", "stopping"].includes(status.phase)) return;
  if (!isTauri) {
    if (showFeedback) showToast(`On ${activeProject()?.git_branch ?? "main"}`);
    return;
  }
  const previous = activeProject();
  try {
    const refreshed = await invoke<Project>("refresh_project", { projectId: selectedProjectId });
    projects = projects.map((item) => item.id === refreshed.id ? refreshed : item);
    status.last_error = null;
    if (refreshed.git_error) recordings = [];
    else await refreshRecordings();
    const changed = previous?.git_branch && previous.git_branch !== refreshed.git_branch;
    if (showFeedback) showToast(changed ? `Switched to ${refreshed.git_branch}` : `On ${refreshed.git_branch ?? "legacy storage"}`);
    else render();
  } catch (error) {
    status.last_error = String(error);
    recordings = [];
    render();
  }
}

async function reveal(path?: string): Promise<void> {
  if (!path) return;
  if (isTauri) await invoke("reveal_path", { path }); else showToast(isMacPlatform ? "Finder opens in the desktop app" : "Files opens in the desktop app");
}

async function startRecording(note: string): Promise<void> {
  try {
    if (isTauri) {
      const startedStatus = await invoke<Status>("start_recording", { note });
      // Native capture may emit `started` before this invoke resolves. Never
      // replace that newer event state with an older `preparing` response.
      if (status.phase !== "recording" || startedStatus.phase === "recording") status = startedStatus;
    }
    else {
      status = { phase: "recording", active_project_id: selectedProjectId, active_video_path: "/mock/recording.mp4", started_at: new Date().toISOString(), last_error: null };
    }
    render();
  } catch (error) {
    status.phase = "error"; status.last_error = String(error); render();
  }
}

async function stopRecording(): Promise<void> {
  try {
    if (isTauri) await invoke("stop_recording");
    else {
      if (mockTimer !== null) window.clearTimeout(mockTimer);
      status.phase = "stopping"; render();
      mockTimer = window.setTimeout(() => {
        const now = new Date().toISOString();
        recordings = [{ id: `demo-${Date.now()}`, project_id: selectedProjectId!, video_path: "/mock/new-packet.mp4", metadata_path: "/mock/new-packet.json", note: "New prompt packet", git_branch: activeProject()?.git_branch ?? null, started_at: status.started_at ?? now, ended_at: now, duration_seconds: 138, size_bytes: 18_400_000, success: true, transcript: "New prompt packet transcript", transcript_path: "/mock/new-packet.transcript.md", transcript_segments: [{ start_seconds: 0, end_seconds: 3.2, text: "New prompt packet transcript" }], transcription_status: "complete", transcription_error: null, transcription_language: appSettings.transcription_language, poster_path: null, timeline_notes: [] }, ...recordings];
        status = { ...emptyStatus(), active_project_id: selectedProjectId };
        showToast("Prompt packet created");
      }, 900);
    }
  } catch (error) {
    status.phase = "error"; status.last_error = String(error); render();
  }
}

async function refreshRecordings(): Promise<void> {
  if (isTauri) {
    if (!selectedProjectId || activeProject()?.git_error) {
      recordings = [];
      return;
    }
    try {
      recordings = await invoke<Recording[]>("list_recordings", { projectId: selectedProjectId });
      void backfillPosters();
    } catch (error) {
      recordings = [];
      projects = projects.map((project) => project.id === selectedProjectId ? { ...project, git_error: String(error), git_branch: null, branch_path: null, recording_count: 0 } : project);
    }
  } else recordings = mockRecordings(selectedProjectId);
}

function mockCreateProject(name: string): Project {
  const slug = name.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-");
  return { id: `mock-${Date.now()}`, name, path: `~/Projects/${slug}`, storage_path: `~/Documents/Dicta/${slug}`, source_path: `~/Projects/${slug}`, git_branch: "main", branch_path: `~/Documents/Dicta/${slug}/branches/main`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: 0 };
}

function mockRecordings(projectId: string | null): Recording[] {
  if (projectId !== "api-integration") return [];
  const base = new Date();
  const item = (id: string, note: string, seconds: number, hourOffset: number, success = true): Recording => ({ id, project_id: projectId, video_path: `~/Documents/Dicta/api-integration/branches/feature__oauth/${id}.mp4`, metadata_path: `~/Documents/Dicta/api-integration/branches/feature__oauth/${id}.json`, note, git_branch: "feature/oauth", started_at: new Date(base.getTime() - hourOffset * 3_600_000).toISOString(), ended_at: base.toISOString(), duration_seconds: seconds, size_bytes: 12_000_000, success, transcript: success ? `${note}. Walk through the relevant files, then show the failure and the expected behavior.` : null, transcript_path: success ? `/mock/${id}.transcript.md` : null, transcript_segments: success ? [{ start_seconds: 4, end_seconds: 11, text: `${note}. Walk through the relevant files.` }, { start_seconds: 12, end_seconds: 19, text: "Then show the failure and the expected behavior." }] : [], transcription_status: success ? "complete" : "processing", transcription_error: null, transcription_language: "en", poster_path: null, timeline_notes: id === "authentication-edge-cases" ? [
    { id: "demo-note-1", timestamp_seconds: 22, text: "The expired-token response should preserve the original request ID.", created_at: base.toISOString(), source: "typed" },
    { id: "demo-note-2", timestamp_seconds: 74, text: "Compare this refresh path with the retry behavior shown later.", created_at: base.toISOString(), source: "voice" },
  ] : [] });
  return [item("authentication-edge-cases", "Authentication edge cases", 138, 1), item("webhook-payload", "Webhook payload", 282, 2, false), item("retry-behavior", "Retry behavior", 96, 3)];
}

function mockContext(): string {
  return `# Dicta context: ${activeProject()?.name}\n\nWorking copy: \`${activeProject()?.path}\`\nGit branch: \`${activeProject()?.git_branch}\`\n\n${recordings.map((item) => `- ${item.note}: ${item.video_path}`).join("\n")}`;
}

async function downloadQualityModel(): Promise<void> {
  if (modelDownloading || modelStatus.quality_installed) return;
  modelDownloading = true;
  modelDownload = {
    downloaded_bytes: 0,
    total_bytes: modelStatus.download_size_bytes,
    progress: 0,
    status: "downloading",
    message: "Preparing download…",
  };
  render();
  try {
    if (isTauri) {
      modelStatus = await invoke<ModelStatus>("download_quality_model");
      modelDownloading = false;
      modelDownload = { downloaded_bytes: modelStatus.quality_size_bytes, total_bytes: modelStatus.quality_size_bytes, progress: 1, status: "complete", message: "High-quality transcription is ready." };
      render();
      showToast("High-quality model installed");
      return;
    }

    if (mockModelTimer !== null) window.clearInterval(mockModelTimer);
    mockModelTimer = window.setInterval(() => {
      const next = Math.min(1, (modelDownload?.progress ?? 0) + 0.08);
      modelDownload = {
        downloaded_bytes: Math.round(modelStatus.download_size_bytes * next),
        total_bytes: modelStatus.download_size_bytes,
        progress: next,
        status: next >= 1 ? "verifying" : "downloading",
        message: next >= 1 ? "Verifying the model…" : "Downloading the high-quality model…",
      };
      render();
      if (next >= 1 && mockModelTimer !== null) {
        window.clearInterval(mockModelTimer);
        mockModelTimer = window.setTimeout(() => {
          modelStatus = { ...modelStatus, quality_installed: true, quality_size_bytes: modelStatus.download_size_bytes, active_model: "High quality · large-v3-turbo", active_model_path: modelStatus.quality_path, message: "The high-quality model is installed and ready." };
          modelDownloading = false;
          modelDownload = { downloaded_bytes: modelStatus.download_size_bytes, total_bytes: modelStatus.download_size_bytes, progress: 1, status: "complete", message: "High-quality transcription is ready." };
          render();
          showToast("High-quality model installed");
        }, 650);
      }
    }, 160);
  } catch (error) {
    modelDownloading = false;
    modelDownload = { downloaded_bytes: 0, total_bytes: modelStatus.download_size_bytes, progress: 0, status: "error", message: String(error) };
    status.last_error = String(error);
    render();
  }
}

async function initialize(): Promise<void> {
  if (isTauri) {
    const [initial, initialMcpStatus, initialModelStatus, initialAppSettings] = await Promise.all([
      invoke<Bootstrap>("bootstrap"),
      invoke<McpStatus>("mcp_status"),
      invoke<ModelStatus>("model_status"),
      invoke<AppSettings>("get_app_settings"),
    ]);
    mcpStatus = initialMcpStatus;
    modelStatus = initialModelStatus;
    appSettings = initialAppSettings;
    selectedTranscriptionLanguage = appSettings.transcription_language;
    projects = initial.projects; status = initial.status;
    selectedProjectId = status.active_project_id ?? projects[0]?.id ?? null;
    if (selectedProjectId && selectedProjectId !== status.active_project_id) await invoke("select_project", { projectId: selectedProjectId });
    await refreshRecordings();
    await listen<RecorderEvent>("recorder-event", async ({ payload }) => {
      status = payload.status;
      if (payload.event === "stopping" && payload.message.includes("20-minute")) {
        showToast(payload.message);
      }
      if (["finished", "transcribed", "transcription_error", "error"].includes(payload.event)) {
        await refreshRecordings();
        const updated = await invoke<Bootstrap>("bootstrap"); projects = updated.projects;
        if (payload.event === "finished") {
          showToast("Recording saved — transcribing now");
          return;
        }
        if (payload.event === "transcription_error") {
          showToast(payload.message);
          return;
        }
      }
      render();
    });
    await listen<ModelDownloadEvent>("model-download-progress", ({ payload }) => {
      modelDownload = payload;
      modelDownloading = payload.status === "downloading" || payload.status === "verifying";
      render();
    });
  } else {
    projects = [
      mockProject("api-integration", "API integration", "feature/oauth", 3),
      mockProject("billing-rewrite", "Billing rewrite", "main", 0),
      mockProject("search-prototype", "Search prototype", "prototype/ranking", 0),
    ];
    selectedProjectId = projects[0].id; status.active_project_id = selectedProjectId; await refreshRecordings();
  }
  render();
}

async function backfillPosters(): Promise<void> {
  if (!isTauri || !selectedProjectId) return;
  let changed = false;
  for (const recording of recordings) {
    if (recording.poster_path || !recording.success || !recording.video_path) continue;
    try {
      const poster = await invoke<string | null>("ensure_recording_poster", {
        projectId: selectedProjectId,
        recordingId: recording.id,
      });
      if (poster) {
        recording.poster_path = poster;
        changed = true;
      }
    } catch {
      // Keep the fallback thumbnail if a frame cannot be extracted.
    }
  }
  if (changed) render();
}

initialize().catch((error) => { app.innerHTML = `<div class="fatal"><strong>Dicta could not start.</strong><pre>${escapeHtml(String(error))}</pre></div>`; });

function mockProject(id: string, name: string, branch: string, recordingCount: number): Project {
  const branchFolder = branch.replaceAll("/", "__");
  return { id, name, path: `~/Projects/${id}`, storage_path: `~/Documents/Dicta/${id}`, source_path: `~/Projects/${id}`, git_branch: branch, branch_path: `~/Documents/Dicta/${id}/branches/${branchFolder}`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: recordingCount };
}

window.addEventListener("focus", () => { void refreshActiveProject(); });
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (themePreference === "system") {
    applyTheme();
    render();
  }
});
