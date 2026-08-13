import "@phosphor-icons/web/regular";
import "./style.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import authThumb from "./assets/packet-auth.png";
import webhookThumb from "./assets/packet-webhook.png";
import retryThumb from "./assets/packet-retry.png";
import type { Bootstrap, McpStatus, ModelDownloadEvent, ModelStatus, Project, RecorderEvent, Recording, Status } from "./types";

const app = document.querySelector<HTMLDivElement>("#app")!;
const isTauri = "__TAURI_INTERNALS__" in window;

let projects: Project[] = [];
let recordings: Recording[] = [];
let selectedProjectId: string | null = null;
let status: Status = emptyStatus();
let elapsedTimer: number | null = null;
let createProjectOpen = false;
let startSheetOpen = false;
let openPacketMenu: string | null = null;
let transcribeRecordingId: string | null = null;
let deleteRecordingId: string | null = null;
let selectedTranscriptionLanguage = "nl";
let toastMessage = "";
let mockTimer: number | null = null;
let mockModelTimer: number | null = null;
let mcpRestarting = false;
let mcpStatus: McpStatus = { installed: false, codex_configured: false, executable_path: "", message: "Connect Dicta to Codex" };
let activeView: "project" | "settings" = "project";
let settingsSection: "appearance" | "transcription" = "appearance";
type ThemePreference = "system" | "light" | "dark";
const savedTheme = window.localStorage.getItem("dicta-theme");
let themePreference: ThemePreference = savedTheme === "light" || savedTheme === "dark" ? savedTheme : "system";
let modelDownloading = false;
let modelDownload: ModelDownloadEvent | null = null;
let modelStatus: ModelStatus = {
  bundled_ready: true,
  quality_installed: false,
  quality_path: "/Users/jens/Library/Application Support/Dicta/models/ggml-large-v3-turbo-q5_0.bin",
  quality_size_bytes: 0,
  download_size_bytes: 547 * 1024 * 1024,
  active_model: "Compact · base",
  active_model_path: "Dicta.app/Contents/Resources/ggml-base-q5_1.bin",
  message: "The compact offline model is active. Download high quality for better Dutch and technical speech.",
};

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

function thumbnailFor(recording: Recording, index: number): string {
  const note = recording.note.toLowerCase();
  if (note.includes("webhook")) return webhookThumb;
  if (note.includes("retry")) return retryThumb;
  if (note.includes("auth")) return authThumb;
  return [authThumb, webhookThumb, retryThumb][index % 3];
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
  const project = activeProject();
  const isBusy = ["preparing", "recording", "stopping"].includes(status.phase);
  const branchUnavailable = Boolean(project?.is_git && (!project.git_branch || project.git_error));
  const buttonDisabled = !project || branchUnavailable || status.phase === "preparing" || status.phase === "stopping";
  const recordingToDelete = recordings.find((recording) => recording.id === deleteRecordingId);

  app.innerHTML = `
    <main class="app-shell">
      <aside class="sidebar">
        <div class="sidebar-title">Projects</div>
        <nav class="project-list" aria-label="Projects">
          ${projects.length === 0 ? `<div class="sidebar-empty">No projects yet</div>` : projects.map((item) => `
            <button class="project-item ${activeView === "project" && item.id === selectedProjectId ? "selected" : ""}" data-project-id="${escapeHtml(item.id)}" ${isBusy ? "disabled" : ""}>
              <i class="ph ph-folder" aria-hidden="true"></i>
              <span class="project-label"><span>${escapeHtml(item.name)}</span><small>${escapeHtml(item.git_branch ?? (item.is_git ? "Git unavailable" : "Unlinked project"))}</small></span>
            </button>
          `).join("")}
        </nav>
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
        <header class="project-header">
          <div class="project-heading">
            <h1>${escapeHtml(project?.name ?? "Choose a project")}</h1>
            <div class="project-context">
              <button class="path-button" id="copy-path" ${project ? "" : "disabled"} title="Copy working-copy path">
                <i class="ph ph-folder" aria-hidden="true"></i>
                <span>${project ? escapeHtml(compactPath(project.path)) : "Link a Git project to begin"}</span>
                ${project ? '<i class="ph ph-copy" aria-hidden="true"></i>' : ""}
              </button>
              ${project ? `<button class="branch-pill ${branchUnavailable ? "unavailable" : ""}" id="refresh-branch" title="Refresh current Git branch"><i class="ph ph-git-branch"></i><span>${escapeHtml(project.git_branch ?? "Git unavailable")}</span><i class="ph ph-arrows-clockwise refresh-icon"></i></button>` : ""}
            </div>
          </div>
          <button class="record-cta ${isBusy ? "active" : ""}" id="record-toggle" ${buttonDisabled ? "disabled" : ""}>
            <span class="record-cta-main">
              <span class="record-symbol"><span></span></span>
              <span class="record-cta-copy"><strong id="record-label">${statusCopy()}</strong><small>${status.phase === "recording" ? "Capturing" : "Screen + audio"}</small></span>
            </span>
            <span class="record-cta-meta">${status.phase === "recording" ? `<span class="record-time" id="record-time">${elapsed()}</span>` : ""}<kbd>⌘ ⇧ R</kbd></span>
          </button>
          ${status.last_error || project?.git_error ? `<div class="error-banner"><i class="ph ph-warning-circle"></i><span>${escapeHtml(status.last_error ?? project?.git_error ?? "")}</span></div>` : ""}
        </header>

        <section class="packet-section">
          <div class="packet-title-row">
            <div><h2>Prompt packets</h2><p>${recordings.length} recording${recordings.length === 1 ? "" : "s"}</p></div>
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
              ` : recordings.slice(0, 12).map((recording, index) => {
                const packet = packetStatus(recording);
                return `
                <div class="packet-row" role="row" data-recording-id="${escapeHtml(recording.id)}">
                  <button class="thumbnail-button" data-reveal="${escapeHtml(recording.video_path)}" aria-label="Reveal ${escapeHtml(recording.note || "recording")}">
                    <img src="${thumbnailFor(recording, index)}" alt="Screen preview for ${escapeHtml(recording.note || "recording")}" />
                    <span class="play-overlay"><i class="ph ph-play" aria-hidden="true"></i></span>
                  </button>
                  <button class="packet-name" data-reveal="${escapeHtml(recording.video_path)}">${escapeHtml(recording.note || "Untitled recording")}</button>
                  <span class="packet-meta">${formatDuration(recording.duration_seconds)}</span>
                  <span class="packet-meta">${formatDate(recording.started_at)}</span>
                  <span><span class="status-chip ${packet.className}" title="${escapeHtml(packet.title)}"><i class="ph ${packet.icon}"></i>${packet.label}</span></span>
                  <div class="menu-cell">
                    <button class="more-button" data-menu="${escapeHtml(recording.id)}" aria-label="More actions"><i class="ph ph-dots-three"></i></button>
                    ${openPacketMenu === recording.id ? `
                      <div class="packet-menu">
                        <button data-transcribe="${escapeHtml(recording.id)}" ${recording.success ? "" : "disabled"}><i class="ph ph-waveform"></i>Transcribe</button>
                        <span class="packet-menu-divider"></span>
                        <button data-reveal="${escapeHtml(recording.video_path)}"><i class="ph ph-folder-open"></i>Reveal</button>
                        <button data-copy-video="${escapeHtml(recording.video_path)}"><i class="ph ph-copy"></i>Copy</button>
                        <span class="packet-menu-divider"></span>
                        <button class="danger" data-delete="${escapeHtml(recording.id)}"><i class="ph ph-trash"></i>Delete</button>
                      </div>
                    ` : ""}
                  </div>
                </div>`;
              }).join("")}
            </div>
          </div>
        </section>

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
                <span class="settings-eyebrow">Dicta on this Mac</span>
                <h1 id="settings-title">Settings</h1>
              </div>
              <button class="settings-close" id="close-settings" aria-label="Close settings"><i class="ph ph-x"></i></button>
            </header>

            <div class="settings-layout">
              <nav class="settings-nav" aria-label="Settings sections">
                <button class="${settingsSection === "appearance" ? "selected" : ""}" data-settings-section="appearance"><i class="ph ph-palette"></i><span>Appearance</span></button>
                <button class="${settingsSection === "transcription" ? "selected" : ""}" data-settings-section="transcription"><i class="ph ph-waveform"></i><span>Transcription</span></button>
                <button disabled><i class="ph ph-keyboard"></i><span>Shortcuts</span><small>Soon</small></button>
              </nav>

              <div class="settings-content">
                <section class="settings-section-block" id="appearance-settings">
                  <div class="settings-content-heading">
                    <h2>Appearance</h2>
                    <p>Choose how Dicta looks on this Mac.</p>
                  </div>
                  <div class="settings-group" aria-label="Theme">
                    <div class="settings-group-label">Theme</div>
                    <div class="theme-options" role="radiogroup" aria-label="Theme">
                      ${([
                        ["system", "ph-desktop", "System", "Follow macOS"],
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

                <section class="settings-section-block" id="transcription-settings">
                <div class="settings-content-heading">
                  <h2>Transcription</h2>
                  <p>Choose the local speech model Dicta uses to turn recordings into agent-readable context.</p>
                </div>

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
                        <span><i class="ph ph-lock-key"></i>Runs entirely on your Mac</span>
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
          <p>Dicta will capture your main display, microphone, and system audio into <strong>${escapeHtml(project?.git_branch ?? "the current branch")}</strong>.</p>
          <label>What should Codex understand? <span>Optional</span><textarea id="session-note" rows="3" placeholder="Authentication edge cases, webhook behavior…"></textarea></label>
          <div class="source-summary"><span><i class="ph ph-monitor"></i>Main display</span><span><i class="ph ph-microphone"></i>Microphone</span><span><i class="ph ph-speaker-high"></i>System audio</span></div>
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

    ${toastMessage ? `<div class="toast"><i class="ph ph-check-circle"></i>${escapeHtml(toastMessage)}</div>` : ""}
  `;

  bindEvents();
  if (isBusy) {
    elapsedTimer = window.setInterval(() => {
      const time = document.querySelector("#record-time");
      if (time) time.textContent = elapsed();
    }, 500);
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
    render();
  }));

  document.querySelector("#new-project")?.addEventListener("click", async () => {
    activeView = "project";
    if (isTauri) await linkProjectFolder();
    else { createProjectOpen = true; render(); }
  });
  document.querySelector("#open-settings")?.addEventListener("click", () => {
    activeView = "settings";
    settingsSection = "appearance";
    openPacketMenu = null;
    render();
  });
  document.querySelector("#close-settings")?.addEventListener("click", () => {
    activeView = "project";
    render();
  });
  document.querySelector("#download-model")?.addEventListener("click", () => { void downloadQualityModel(); });
  document.querySelectorAll<HTMLButtonElement>("[data-settings-section]").forEach((button) => button.addEventListener("click", () => {
    settingsSection = button.dataset.settingsSection === "transcription" ? "transcription" : "appearance";
    render();
    window.requestAnimationFrame(() => document.querySelector(`#${settingsSection}-settings`)?.scrollIntoView({ behavior: "smooth", block: "start" }));
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach((button) => button.addEventListener("click", () => {
    setTheme((button.dataset.themeChoice ?? "system") as ThemePreference);
  }));
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

  const openStart = () => { if (activeProject()) { startSheetOpen = true; render(); } };
  document.querySelector("#empty-record")?.addEventListener("click", openStart);
  document.querySelector("#record-toggle")?.addEventListener("click", async () => {
    if (status.phase === "recording") await stopRecording(); else openStart();
  });
  document.querySelector("#start-recording-form")?.addEventListener("click", stopPropagationOnModal);
  document.querySelector("#start-recording-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const note = document.querySelector<HTMLTextAreaElement>("#session-note")?.value.trim() ?? "";
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
        : { installed: true, codex_configured: true, executable_path: "/Library/Application Support/Dicta/bin/dicta-mcp", message: mcpRestarting ? "Dicta MCP restarted." : "Dicta is connected." };
      mcpRestarting = false;
      render();
      showToast(mcpStatus.message);
    } catch (error) {
      mcpRestarting = false;
      status.last_error = String(error);
      render();
    }
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
    selectedTranscriptionLanguage = recording.transcription_language ?? "nl";
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
    selectedTranscriptionLanguage = button.dataset.language ?? "nl";
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
  if (isTauri) await invoke("reveal_path", { path }); else showToast("Finder opens in the desktop app");
}

async function startRecording(note: string): Promise<void> {
  try {
    if (isTauri) status = await invoke<Status>("start_recording", { note });
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
        recordings = [{ id: `demo-${Date.now()}`, project_id: selectedProjectId!, video_path: "/mock/new-packet.mp4", metadata_path: "/mock/new-packet.json", note: "New prompt packet", git_branch: activeProject()?.git_branch ?? null, started_at: status.started_at ?? now, ended_at: now, duration_seconds: 138, size_bytes: 18_400_000, success: true, transcript: "New prompt packet transcript", transcript_path: "/mock/new-packet.transcript.md", transcription_status: "complete", transcription_error: null, transcription_language: "nl" }, ...recordings];
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
    } catch (error) {
      recordings = [];
      projects = projects.map((project) => project.id === selectedProjectId ? { ...project, git_error: String(error), git_branch: null, branch_path: null, recording_count: 0 } : project);
    }
  } else recordings = mockRecordings(selectedProjectId);
}

function mockCreateProject(name: string): Project {
  const slug = name.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-");
  return { id: `mock-${Date.now()}`, name, path: `/Users/jens/Projects/${slug}`, storage_path: `/Users/jens/Documents/Dicta/${slug}`, source_path: `/Users/jens/Projects/${slug}`, git_branch: "main", branch_path: `/Users/jens/Documents/Dicta/${slug}/branches/main`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: 0 };
}

function mockRecordings(projectId: string | null): Recording[] {
  if (projectId !== "api-integration") return [];
  const base = new Date();
  const item = (id: string, note: string, seconds: number, hourOffset: number, success = true): Recording => ({ id, project_id: projectId, video_path: `/Users/jens/Documents/Dicta/api-integration/branches/feature__oauth/${id}.mp4`, metadata_path: `/Users/jens/Documents/Dicta/api-integration/branches/feature__oauth/${id}.json`, note, git_branch: "feature/oauth", started_at: new Date(base.getTime() - hourOffset * 3_600_000).toISOString(), ended_at: base.toISOString(), duration_seconds: seconds, size_bytes: 12_000_000, success, transcript: success ? `${note} transcript` : null, transcript_path: success ? `/mock/${id}.transcript.md` : null, transcription_status: success ? "complete" : "processing", transcription_error: null, transcription_language: "nl" });
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
    const [initial, initialMcpStatus, initialModelStatus] = await Promise.all([
      invoke<Bootstrap>("bootstrap"),
      invoke<McpStatus>("mcp_status"),
      invoke<ModelStatus>("model_status"),
    ]);
    mcpStatus = initialMcpStatus;
    modelStatus = initialModelStatus;
    projects = initial.projects; status = initial.status;
    selectedProjectId = status.active_project_id ?? projects[0]?.id ?? null;
    if (selectedProjectId && selectedProjectId !== status.active_project_id) await invoke("select_project", { projectId: selectedProjectId });
    await refreshRecordings();
    await listen<RecorderEvent>("recorder-event", async ({ payload }) => {
      status = payload.status;
      if (["finished", "transcribed", "transcription_error", "error"].includes(payload.event)) {
        await refreshRecordings();
        const updated = await invoke<Bootstrap>("bootstrap"); projects = updated.projects;
        if (payload.event === "finished") toastMessage = "Recording saved — transcribing now";
        if (payload.event === "transcribed") toastMessage = "Transcript ready for agents";
        if (payload.event === "transcription_error") toastMessage = payload.message;
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

initialize().catch((error) => { app.innerHTML = `<div class="fatal"><strong>Dicta could not start.</strong><pre>${escapeHtml(String(error))}</pre></div>`; });

function mockProject(id: string, name: string, branch: string, recordingCount: number): Project {
  const branchFolder = branch.replaceAll("/", "__");
  return { id, name, path: `/Users/jens/Projects/${id}`, storage_path: `/Users/jens/Documents/Dicta/${id}`, source_path: `/Users/jens/Projects/${id}`, git_branch: branch, branch_path: `/Users/jens/Documents/Dicta/${id}/branches/${branchFolder}`, is_git: true, git_error: null, created_at: new Date().toISOString(), recording_count: recordingCount };
}

window.addEventListener("focus", () => { void refreshActiveProject(); });
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (themePreference === "system") {
    applyTheme();
    render();
  }
});
