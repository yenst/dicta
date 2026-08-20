import "@phosphor-icons/web/regular";
import "./style.css";
import dictaMarkUrl from "./assets/dicta-mark.png";
import dictaMarkLightUrl from "./assets/dicta-mark-light.png";
import demoRecordingPosterUrl from "./assets/demo-recording-poster.png";
import codexLightUrl from "./assets/codex-light.png";
import codexDarkUrl from "./assets/codex-dark.png";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createDemoDictaClient, createNativeDictaClient } from "./dicta-client";
import { MediaBlobCache, mediaIdentityKey, type MediaIdentity } from "./media-blob-cache";
import { createPlatformCapabilities, detectPlatform } from "./platform";
import { ProjectController } from "./project-controller";
import type { AppSettings, CleanupSummary, McpStatus, ModelDownloadEvent, ModelStatus, Project, RecorderEvent, Recording, Status, TimelineNote } from "./types";
import { mountModalLifecycle, shouldDismissModal } from "./features/modal-lifecycle";
import { runAsyncAction } from "./features/async-action";
import { AppLifecycle } from "./features/app-lifecycle";
import { mountDelegatedEvents } from "./features/delegated-events";
import { captureFocusKey, restoreFocusKey, type FocusKey } from "./features/focus-restoration";
import { handleViewerTabKeydown } from "./features/viewer-tab-events";
import { renderModals } from "./views/modals-view";
import { renderPlatformChrome, renderProjectHeader, renderProjectsSidebar, type ProjectsViewModel } from "./views/projects-view";
import { renderRecordingIndexBody, renderRecordings, type RecordingGroup } from "./views/recordings-view";
import { renderSettings, type SettingsSection, type ThemePreference } from "./views/settings-view";
import { renderShell } from "./views/shell-view";
import { renderViewer } from "./views/viewer-view";
import {
  escapeHtml,
  formatDuration,
  formatViewerTime,
  recordingDayHeading,
} from "./views/view-helpers";

const app = document.querySelector<HTMLDivElement>("#app")!;
const nativeApp = "__TAURI_INTERNALS__" in window && !(import.meta.env.DEV && new URLSearchParams(window.location.search).has("demo"));
const platform = createPlatformCapabilities(detectPlatform(navigator.platform));
const dictaClient = nativeApp ? createNativeDictaClient() : createDemoDictaClient(platform.defaultShortcutId);
const projectController = new ProjectController(dictaClient, () => render());
const appLifecycle = new AppLifecycle();

let status: Status = emptyStatus();
let elapsedTimer: number | null = null;
let createProjectOpen = false;
let startSheetOpen = false;
let recordingTargetProjectId: string | null = null;
let openPacketMenu: string | null = null;
let openPacketMenuSurface: "index" | "detail" | null = null;
let openProjectMenu: string | null = null;
let removeProjectId: string | null = null;
let transcribeRecordingId: string | null = null;
let deleteRecordingId: string | null = null;
let selectedTranscriptionLanguage = "auto";
let sessionNote = "";
let lastSessionNote = "";
let viewingRecordingId: string | null = null;
const viewerVideoCache = new MediaBlobCache();
let viewerTime = 0;
let viewerPaused = true;
let viewerMarkedTime = 0;
let viewerNoteDraft = "";
let viewerNoteSource: TimelineNote["source"] = "typed";
let viewerPanel: "notes" | "transcript" | "chapters" = "transcript";
let viewerListening = false;
let viewerVoiceProcessing = false;
let activeSpeechRecognition: SpeechRecognitionLike | null = null;
let activeVoiceRecorder: MediaRecorder | null = null;
let activeVoiceStream: MediaStream | null = null;
let voiceChunks: Blob[] = [];
let voiceStopTimer: number | null = null;
let toastMessage = "";
let toastTimer: number | null = null;
let mockModelTimer: number | null = null;
let mcpRestarting = false;
let mcpStatus: McpStatus = { installed: false, codex_configured: false, executable_path: "", message: "Connect Dicta to Codex" };
interface UiState {
  activeView: "project" | "settings";
  settingsSection: SettingsSection;
  projectPickerOpen: boolean;
  recordingSearchOpen: boolean;
  recordingQuery: string;
  recordingDrawerOpen: boolean;
}

interface RecordingSelection {
  project_id: string;
  recording_id: string;
}

const ui: UiState = {
  activeView: "project",
  settingsSection: "appearance",
  projectPickerOpen: false,
  recordingSearchOpen: false,
  recordingQuery: "",
  recordingDrawerOpen: false,
};
let appSettings: AppSettings = { shortcut_id: platform.defaultShortcutId, cleanup_merged_videos: true, branch_locking: true, transcription_language: "auto", general_path: null };
let cleanupRunning = false;
let cleanupSummary: CleanupSummary | null = null;
const savedTheme = window.localStorage.getItem("dicta-theme");
let themePreference: ThemePreference = savedTheme === "light" || savedTheme === "dark" ? savedTheme : "system";
let modelDownloading = false;
let modelDownload: ModelDownloadEvent | null = null;
let modalReturnFocusKey: FocusKey | null = null;
let appDisposed = false;
let modelStatus: ModelStatus = {
  bundled_ready: true,
  quality_installed: false,
  quality_path: platform.qualityModelPath,
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
  app.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach((button) => {
    const selected = button.dataset.themeChoice === preference;
    button.classList.toggle("selected", selected);
    button.setAttribute("aria-checked", String(selected));
    const stateIcon = button.querySelector<HTMLElement>("i:last-child");
    if (stateIcon) stateIcon.className = `ph ${selected ? "ph-check-circle" : "ph-circle"}`;
  });
}

applyTheme();

function activeProject(): Project | undefined {
  return projectController.activeProject();
}

function recordingTargetProject(): Project | undefined {
  return projectController.project(recordingTargetProjectId);
}

function recordingActionsMenu(recording: Recording, className: string): string {
  return `
    <div class="packet-menu ${className}">
      <button data-copy-recording-context="${escapeHtml(recording.id)}"><i class="ph ph-copy"></i>Copy context</button>
      <button data-transcribe="${escapeHtml(recording.id)}" ${recording.success ? "" : "disabled"}><i class="ph ph-waveform"></i>Transcribe</button>
      <span class="packet-menu-divider"></span>
      <button data-reveal="${escapeHtml(recording.video_path)}"><i class="ph ph-folder-open"></i>Reveal</button>
      <button data-copy-video="${escapeHtml(recording.video_path)}"><i class="ph ph-copy"></i>Copy path</button>
      <span class="packet-menu-divider"></span>
      <button class="danger" data-delete="${escapeHtml(recording.id)}"><i class="ph ph-trash"></i>Delete</button>
    </div>`;
}

function elapsed(): string {
  if (!status.started_at) return "00:00";
  return formatDuration((Date.now() - new Date(status.started_at).getTime()) / 1000);
}

function mediaSrc(path: string | null | undefined): string {
  if (!path || !dictaClient.isNative) return "";
  return convertFileSrc(path);
}

function recordingMediaIdentity(recording: Recording): MediaIdentity {
  return {
    projectId: recording.project_id,
    recordingId: recording.id,
    videoPath: recording.video_path,
  };
}

function isViewingMedia(identity: MediaIdentity): boolean {
  const viewing = projectController.recordings.find((recording) => recording.id === viewingRecordingId);
  return Boolean(viewing && mediaIdentityKey(recordingMediaIdentity(viewing)) === mediaIdentityKey(identity));
}

const transcriptionLanguages = [
  { code: "nl", label: "Dutch", native: "Nederlands" },
  { code: "en", label: "English", native: "English" },
  { code: "auto", label: "Auto-detect", native: "Let Whisper decide" },
  { code: "fr", label: "French", native: "Français" },
  { code: "de", label: "German", native: "Deutsch" },
  { code: "es", label: "Spanish", native: "Español" },
];

function shortcutLabel(): string {
  return platform.shortcutOptions.find((shortcut) => shortcut.id === appSettings.shortcut_id)?.label ?? platform.shortcutOptions[0].label;
}

function showToast(message: string): void {
  toastMessage = message;
  let toast = app.querySelector<HTMLElement>(".toast");
  if (!toast) {
    toast = document.createElement("div");
    toast.className = "toast";
    app.append(toast);
  }
  const icon = document.createElement("i");
  icon.className = "ph ph-check-circle";
  toast.replaceChildren(icon, document.createTextNode(message));
  if (toastTimer !== null) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    if (toastMessage === message) {
      toastMessage = "";
      toast?.remove();
    }
  }, 1800);
}

function visibleRecordingGroups(query: string): RecordingGroup[] {
  const groups = projectController.recordings.reduce<RecordingGroup[]>((result, recording) => {
    const key = new Date(recording.started_at).toDateString();
    const existing = result.find((group) => group.key === key);
    if (existing) existing.items.push(recording);
    else result.push({ key, label: recordingDayHeading(recording.started_at), items: [recording] });
    return result;
  }, []);
  const normalized = query.trim().toLowerCase();
  if (!normalized) return groups;
  return groups.map((group) => ({
    ...group,
    items: group.items.filter((recording) => recording.id.toLowerCase().includes(normalized)
      || recording.note.toLowerCase().includes(normalized)
      || (recording.transcript ?? "").toLowerCase().includes(normalized)),
  })).filter((group) => group.items.length > 0);
}

function updateRecordingSearchResults(): void {
  const body = app.querySelector<HTMLElement>(".recording-index-body");
  if (!body) return;
  const project = activeProject();
  const branchUnavailable = Boolean(appSettings.branch_locking && project?.is_git && (!project.git_branch || project.git_error));
  const buttonDisabled = status.phase !== "recording" && (!project || branchUnavailable || status.phase === "preparing" || status.phase === "stopping");
  body.innerHTML = renderRecordingIndexBody({
    recordings: projectController.recordings,
    visibleGroups: visibleRecordingGroups(ui.recordingQuery),
    project,
    branchLocking: appSettings.branch_locking,
    buttonDisabled,
    viewingRecordingId,
  });
}

function updateRadioSelection(selector: string, selectedValue: string, dataKey: "shortcutChoice" | "defaultLanguage"): void {
  app.querySelectorAll<HTMLButtonElement>(selector).forEach((button) => {
    const selected = button.dataset[dataKey] === selectedValue;
    button.classList.toggle("selected", selected);
    button.setAttribute("aria-checked", String(selected));
    const stateIcon = button.querySelector<HTMLElement>("i:last-child");
    if (stateIcon) stateIcon.className = `ph ${selected ? "ph-check-circle" : "ph-circle"}`;
  });
}

function render(): void {
  if (elapsedTimer !== null) window.clearInterval(elapsedTimer);
  const previousPacketSection = document.querySelector<HTMLElement>(".packet-section");
  const packetScrollTop = previousPacketSection?.dataset.projectId === (projectController.selectedProjectId ?? "")
    ? previousPacketSection.scrollTop
    : 0;
  const project = activeProject();
  const targetProject = recordingTargetProject() ?? project;
  const isBusy = ["preparing", "recording", "stopping"].includes(status.phase);
  const branchUnavailable = Boolean(appSettings.branch_locking && project?.is_git && (!project.git_branch || project.git_error));
  const buttonDisabled = status.phase === "recording"
    ? false
    : !project || branchUnavailable || status.phase === "preparing" || status.phase === "stopping";
  const recordingToDelete = projectController.recordings.find((recording) => recording.id === deleteRecordingId);
  const projectToRemove = projectController.projects.find((item) => item.id === removeProjectId);
  const modalWasOpen = Boolean(app.querySelector("[role='dialog'][aria-modal='true']"));
  const modalWillOpen = Boolean(createProjectOpen || startSheetOpen || transcribeRecordingId || recordingToDelete || projectToRemove);
  if (!modalWasOpen && modalWillOpen) modalReturnFocusKey = captureFocusKey(app, document.activeElement);
  const latestRecording = projectController.recordings[0];
  const recordingGroups = visibleRecordingGroups(ui.recordingQuery);
  if (projectController.recordings.length > 0 && !projectController.recordings.some((recording) => recording.id === viewingRecordingId)) {
    viewingRecordingId = projectController.recordings[0].id;
    viewerPanel = "transcript";
  }
  const viewing = projectController.recordings.find((recording) => recording.id === viewingRecordingId);

  const projectsViewModel: ProjectsViewModel = {
    platform,
    markUrl: dictaMarkUrl,
    markLightUrl: dictaMarkLightUrl,
    projects: projectController.projects,
    selectedProjectId: projectController.selectedProjectId,
    project,
    latestRecording,
    activeView: ui.activeView,
    isBusy,
    branchLocking: appSettings.branch_locking,
    branchUnavailable,
    openProjectMenu,
    projectPickerOpen: ui.projectPickerOpen,
    recordingSearchOpen: ui.recordingSearchOpen,
    recordingQuery: ui.recordingQuery,
    statusError: status.last_error ?? project?.git_error ?? null,
  };
  const videoAsset = viewing ? mediaSrc(viewing.video_path) : "";
  const viewerHtml = renderViewer({
    recording: viewing,
    videoAsset,
    videoSource: viewing ? (platform.mediaPlayback === "direct-asset" ? videoAsset : viewerVideoCache.get(recordingMediaIdentity(viewing)) ?? "") : "",
    poster: viewing ? mediaSrc(viewing.poster_path) || (!dictaClient.isNative ? demoRecordingPosterUrl : "") : "",
    panel: viewerPanel,
    actionsMenu: viewing && openPacketMenu === viewing.id && openPacketMenuSurface === "detail" ? recordingActionsMenu(viewing, "detail-recording-menu") : "",
    markedTime: viewerMarkedTime,
    noteDraft: viewerNoteDraft,
    listening: viewerListening,
    voiceProcessing: viewerVoiceProcessing,
    recordingDrawerOpen: ui.recordingDrawerOpen,
  });
  const recordingsHtml = renderRecordings({
    selectedProjectId: projectController.selectedProjectId,
    project,
    recordings: projectController.recordings,
    visibleGroups: recordingGroups,
    viewingRecordingId,
    branchLocking: appSettings.branch_locking,
    buttonDisabled,
    status,
    shortcutLabel: shortcutLabel(),
    viewerHtml,
    recordingDrawerOpen: ui.recordingDrawerOpen,
  });

  const settingsHtml = renderSettings({
    open: ui.activeView === "settings",
    settingsSection: ui.settingsSection,
    platform,
    themePreference,
    mcpStatus,
    mcpRestarting,
    codexLightUrl,
    codexDarkUrl,
    appSettings,
    transcriptionLanguages,
    modelStatus,
    modelDownloading,
    modelDownload,
    cleanupSummary,
    cleanupRunning,
    activeProjectIsGit: Boolean(project?.is_git),
  });
  const modalsHtml = renderModals({
    createProjectOpen,
    startSheetOpen,
    targetProject,
    projects: projectController.projects,
    branchLocking: appSettings.branch_locking,
    sessionNote: sessionNote || lastSessionNote,
    transcribeRecordingId,
    selectedTranscriptionLanguage,
    transcriptionLanguages,
    recordingToDelete,
    projectToRemove,
    platform,
  });
  app.innerHTML = renderShell({
    linux: platform.isLinux,
    chrome: renderPlatformChrome(projectsViewModel),
    sidebar: renderProjectsSidebar(projectsViewModel),
    header: renderProjectHeader(projectsViewModel),
    content: recordingsHtml,
    settings: settingsHtml,
    modals: modalsHtml,
    toast: toastMessage ? `<div class="toast"><i class="ph ph-check-circle"></i>${escapeHtml(toastMessage)}</div>` : "",
  });
  bindEvents();
  appLifecycle.replaceWith("modal", () => mountModalLifecycle(app, { onEscape: closeTopModal }));
  if (modalWasOpen && !modalWillOpen) {
    restoreFocusKey(app, modalReturnFocusKey);
    modalReturnFocusKey = null;
  }
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
  const video = document.querySelector<HTMLVideoElement>("#packet-video");
  if (!video) return;
  const updateControls = () => {
    viewerTime = video.currentTime;
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
  };
  if (video.readyState >= 1) apply();
  else video.addEventListener("loadedmetadata", apply, { once: true });
  video.addEventListener("timeupdate", updateControls);
  video.addEventListener("loadeddata", clearPlaybackError);
  video.addEventListener("play", () => { viewerPaused = false; clearPlaybackError(); });
  video.addEventListener("pause", () => { viewerPaused = true; });
  video.addEventListener("error", () => {
    viewerPaused = true;
    const mediaError = video.error;
    showPlaybackError(mediaError?.message
      ? `This recording could not be loaded: ${mediaError.message}`
      : "This recording could not be loaded.");
  });
  const viewing = projectController.recordings.find((recording) => recording.id === viewingRecordingId);
  if (dictaClient.isNative && platform.mediaPlayback === "blob-fallback" && viewing) void loadLinuxViewerVideo(viewing, showPlaybackError);
}

async function loadLinuxViewerVideo(recording: Recording, showPlaybackError: (message: string) => void): Promise<void> {
  const identity = recordingMediaIdentity(recording);
  const cachedUrl = viewerVideoCache.get(identity);
  if (cachedUrl) return;
  try {
    const url = await viewerVideoCache.load(identity, mediaSrc(recording.video_path));
    if (!url || !isViewingMedia(identity)) return;
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (video && video.currentSrc !== url) {
      video.replaceChildren();
      video.src = url;
      video.load();
    }
  } catch (error) {
    if (isViewingMedia(identity)) showPlaybackError(`This recording could not be loaded: ${String(error)}`);
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
  viewingRecordingId = recordingId;
  viewerTime = 0;
  viewerPaused = true;
  viewerMarkedTime = 0;
  viewerNoteDraft = "";
  viewerNoteSource = "typed";
  viewerPanel = "transcript";
  openPacketMenu = null;
  ui.recordingDrawerOpen = false;
  render();
  window.requestAnimationFrame(() => document.querySelector<HTMLElement>("#packet-viewer")?.focus({ preventScroll: true }));
}

async function openExternalRecording(selection: RecordingSelection): Promise<boolean> {
  if (projectController.selectedProjectId !== selection.project_id) {
    await browseProject(selection.project_id);
  } else if (!projectController.recordings.some((recording) => recording.id === selection.recording_id)) {
    await refreshRecordings(selection.project_id);
  }
  if (projectController.selectedProjectId !== selection.project_id) return false;
  if (!projectController.recordings.some((recording) => recording.id === selection.recording_id)) {
    showToast("Recording is no longer available");
    return false;
  }
  openPacket(selection.recording_id);
  return true;
}

let pendingRecordingOpen: Promise<boolean> | null = null;

async function consumePendingRecordingSelection(): Promise<boolean> {
  if (pendingRecordingOpen) return pendingRecordingOpen;
  const operation = (async () => {
    const selection = await invoke<RecordingSelection | null>("take_pending_recording_selection");
    return selection ? openExternalRecording(selection) : false;
  })();
  pendingRecordingOpen = operation;
  try {
    return await operation;
  } finally {
    if (pendingRecordingOpen === operation) pendingRecordingOpen = null;
  }
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
  const updated = await dictaClient.saveTimelineNotes(recording, notes);
  projectController.updateRecording(updated);
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
    if (appDisposed) {
      activeVoiceStream.getTracks().forEach((track) => track.stop());
      activeVoiceStream = null;
      return;
    }
    const preferredTypes = ["audio/mp4", "audio/webm;codecs=opus", "audio/webm"];
    const mimeType = preferredTypes.find((type) => MediaRecorder.isTypeSupported(type));
    activeVoiceRecorder = mimeType ? new MediaRecorder(activeVoiceStream, { mimeType }) : new MediaRecorder(activeVoiceStream);
    const recordedMimeType = activeVoiceRecorder.mimeType || mimeType || "audio/mp4";
    voiceChunks = [];
    activeVoiceRecorder.addEventListener("dataavailable", (event) => {
      if (!appDisposed && event.data.size) voiceChunks.push(event.data);
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
      if (appDisposed) {
        viewerVoiceProcessing = false;
        return;
      }
      if (!viewingRecordingId) {
        viewerVoiceProcessing = false;
        return;
      }
      try {
        const audioBytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
        if (appDisposed) return;
        const transcript = await dictaClient.transcribeVoiceNote(audioBytes, blob.type, appSettings.transcription_language);
        if (appDisposed) return;
        viewerNoteDraft = [viewerNoteDraft.trim(), transcript.trim()].filter(Boolean).join(" ");
        viewerNoteSource = "voice";
        const input = document.querySelector<HTMLTextAreaElement>("#timeline-note-input");
        const submit = document.querySelector<HTMLButtonElement>(".add-note-button");
        if (input) input.value = viewerNoteDraft;
        if (submit) submit.disabled = !viewerNoteDraft;
      } catch (error) {
        if (!appDisposed) showToast(`Could not transcribe voice note: ${String(error)}`);
      } finally {
        viewerVoiceProcessing = false;
        if (!appDisposed) updateVoiceButton();
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
    if (!appDisposed) showToast(`Microphone unavailable: ${String(error)}`);
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
    showToast(`Voice dictation is unavailable on this ${platform.displayName}`);
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
  if (dictaClient.isNative && typeof MediaRecorder !== "undefined") {
    void startOfflineVoiceNote();
  } else {
    startBrowserDictation();
  }
}

function closeTopModal(): void {
  if (removeProjectId) removeProjectId = null;
  else if (deleteRecordingId) deleteRecordingId = null;
  else if (transcribeRecordingId) transcribeRecordingId = null;
  else if (startSheetOpen) startSheetOpen = false;
  else if (createProjectOpen) createProjectOpen = false;
  else return;
  render();
}

async function updateBranchLocking(enabled: boolean): Promise<void> {
  appSettings = await dictaClient.setBranchLocking(enabled);
  const updated = await dictaClient.bootstrap();
  projectController.replaceProjects(updated.projects);
  await refreshRecordings();
}

async function browseProject(projectId: string): Promise<void> {
  ui.activeView = "project";
  projectController.select(projectId);
  ui.projectPickerOpen = false;
  ui.recordingDrawerOpen = false;
  const captureBusy = ["preparing", "recording", "stopping"].includes(status.phase);
  if (!status.active_project_id || !captureBusy) {
    status.active_project_id = projectId;
    await dictaClient.selectProject(projectId);
  }
  if (!captureBusy) await refreshActiveProject(false, projectId);
  else await refreshRecordings(projectId);
  if (projectController.selectedProjectId !== projectId) return;
  openPacketMenu = null;
  openProjectMenu = null;
  render();
}

function bindEvents(): void {
  document.querySelector("#toggle-recording-search")?.addEventListener("click", () => {
    ui.recordingSearchOpen = !ui.recordingSearchOpen;
    if (ui.recordingSearchOpen) ui.recordingDrawerOpen = true;
    if (!ui.recordingSearchOpen) ui.recordingQuery = "";
    render();
    if (ui.recordingSearchOpen) window.requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#recording-search")?.focus());
  });
  document.querySelector("#focus-recording-search")?.addEventListener("click", () => {
    ui.recordingSearchOpen = true;
    ui.recordingDrawerOpen = true;
    render();
    window.requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#recording-search")?.focus());
  });
  document.querySelector("#project-switcher")?.addEventListener("click", () => { ui.projectPickerOpen = !ui.projectPickerOpen; render(); });
  document.querySelector("#toggle-recording-drawer")?.addEventListener("click", () => {
    ui.recordingDrawerOpen = !ui.recordingDrawerOpen;
    render();
    if (ui.recordingDrawerOpen) window.requestAnimationFrame(() => document.querySelector<HTMLElement>(".recording-index")?.focus({ preventScroll: true }));
  });
  const closeRecordingDrawer = () => {
    ui.recordingDrawerOpen = false;
    ui.recordingSearchOpen = false;
    ui.recordingQuery = "";
    render();
    window.requestAnimationFrame(() => document.querySelector<HTMLElement>("#toggle-recording-drawer")?.focus({ preventScroll: true }));
  };
  document.querySelector("#close-recording-drawer")?.addEventListener("click", closeRecordingDrawer);
  document.querySelector("#recording-drawer-close")?.addEventListener("click", closeRecordingDrawer);
  document.querySelector<HTMLElement>(".recording-index")?.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Escape") closeRecordingDrawer();
  });
  document.querySelector("#remove-project-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!removeProjectId) return;
    const projectId = removeProjectId;
    const wasSelected = projectController.selectedProjectId === projectId;
    removeProjectId = null;
    try {
      await dictaClient.removeProject(projectId);
      const fallbackProjectId = projectController.removeProject(projectId);
      if (wasSelected) {
        status.active_project_id = fallbackProjectId;
        await dictaClient.selectProject(fallbackProjectId);
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
    ui.activeView = "project";
    openProjectMenu = null;
    await linkProjectFolder();
  });
  document.querySelector("#open-settings")?.addEventListener("click", () => {
    ui.activeView = ui.activeView === "settings" ? "project" : "settings";
    if (ui.activeView === "settings") ui.settingsSection = "appearance";
    openPacketMenu = null;
    openProjectMenu = null;
    render();
  });
  document.querySelector("#compact-settings")?.addEventListener("click", () => {
    ui.activeView = ui.activeView === "settings" ? "project" : "settings";
    if (ui.activeView === "settings") ui.settingsSection = "appearance";
    openPacketMenu = null;
    openProjectMenu = null;
    render();
  });
  document.querySelector("#window-close")?.addEventListener("click", () => {
    if (dictaClient.isNative) void getCurrentWindow().close();
  });
  document.querySelector("#download-model")?.addEventListener("click", () => { void downloadQualityModel(); });
  const settingsContent = document.querySelector<HTMLElement>(".settings-content");
  settingsContent?.addEventListener("scroll", () => {
    const sections = ["appearance", "connections", "shortcuts", "transcription", "storage"] as const;
    const marker = settingsContent.getBoundingClientRect().top + 72;
    let visibleSection: typeof sections[number] = settingsContent.scrollTop + settingsContent.clientHeight >= settingsContent.scrollHeight - 4 ? "storage" : "appearance";
    if (visibleSection !== "storage") {
      for (const section of sections) {
        const element = document.querySelector<HTMLElement>(`#${section}-settings`);
        if (element && element.getBoundingClientRect().top <= marker) visibleSection = section;
      }
    }
    if (visibleSection === ui.settingsSection) return;
    ui.settingsSection = visibleSection;
    document.querySelectorAll<HTMLButtonElement>("[data-settings-section]").forEach((item) => item.classList.toggle("selected", item.dataset.settingsSection === ui.settingsSection));
  }, { passive: true });
  document.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach((button) => button.addEventListener("click", () => {
    setTheme((button.dataset.themeChoice ?? "system") as ThemePreference);
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-shortcut-choice]").forEach((button) => button.addEventListener("click", async () => {
    const shortcutId = button.dataset.shortcutChoice;
    if (!shortcutId || shortcutId === appSettings.shortcut_id) return;
    try {
      appSettings = await dictaClient.setShortcut(shortcutId);
      updateRadioSelection("[data-shortcut-choice]", shortcutId, "shortcutChoice");
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
      appSettings = await dictaClient.setTranscriptionLanguage(language);
      selectedTranscriptionLanguage = language;
      updateRadioSelection("[data-default-language]", language, "defaultLanguage");
      showToast(`Default language set to ${transcriptionLanguages.find((item) => item.code === language)?.label ?? language}`);
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  }));
  document.querySelector("#cleanup-toggle")?.addEventListener("click", async () => {
    const enabled = !appSettings.cleanup_merged_videos;
    try {
      appSettings = await dictaClient.setCleanupMergedVideos(enabled);
      cleanupSummary = null;
      render();
      showToast(enabled ? "Cleanup enabled" : "Cleanup disabled");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#branch-lock-settings-toggle")?.addEventListener("click", async () => {
    const enabled = !appSettings.branch_locking;
    try {
      await updateBranchLocking(enabled);
      render();
      showToast(enabled ? "New recordings are branch-specific" : "New recordings are repository-wide");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#cleanup-now")?.addEventListener("click", async () => {
    const projectId = projectController.selectedProjectId;
    if (!projectId || cleanupRunning) return;
    cleanupRunning = true;
    render();
    try {
      cleanupSummary = await dictaClient.cleanupMergedVideos(projectId);
      cleanupRunning = false;
      render();
      showToast(cleanupSummary.message);
    } catch (error) {
      cleanupRunning = false;
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#create-project-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = document.querySelector<HTMLInputElement>("#project-name")!.value.trim();
    if (!name) return;
    const project = await dictaClient.createDemoProject(name);
    if (!project) return;
    projectController.upsertProject(project);
    projectController.select(project.id);
    status.active_project_id = project.id;
    projectController.clearRecordings();
    createProjectOpen = false;
    render();
    showToast("Git project linked");
  });

  const openStart = () => {
    recordingTargetProjectId = projectController.selectedProjectId ?? "__unprojected__";
    if (!sessionNote) sessionNote = lastSessionNote;
    startSheetOpen = true;
    render();
  };
  document.querySelector("#empty-record")?.addEventListener("click", openStart);
  document.querySelector("#record-toggle")?.addEventListener("click", async () => {
    if (status.phase === "recording") await stopRecording(); else openStart();
  });
  document.querySelector<HTMLTextAreaElement>("#session-note")?.addEventListener("input", (event) => {
    sessionNote = (event.target as HTMLTextAreaElement).value;
  });
  document.querySelector<HTMLSelectElement>("#recording-project")?.addEventListener("change", (event) => {
    recordingTargetProjectId = (event.target as HTMLSelectElement).value;
    render();
  });
  document.querySelector("#branch-lock-toggle")?.addEventListener("click", async () => {
    const enabled = !appSettings.branch_locking;
    try {
      await updateBranchLocking(enabled);
      render();
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#start-recording-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const note = document.querySelector<HTMLTextAreaElement>("#session-note")?.value.trim() ?? "";
    sessionNote = note;
    lastSessionNote = note;
    startSheetOpen = false;
    await startRecording(note, recordingTargetProjectId ?? "__unprojected__");
  });

  document.querySelector("#copy-path")?.addEventListener("click", async () => {
    const project = activeProject(); if (!project) return;
    if (project.id === "__unprojected__") {
      try {
        const updated = await dictaClient.chooseGeneralPath(project);
        if (!updated) return;
        projectController.updateProject(updated);
        await refreshRecordings();
        render();
        showToast("General folder updated");
      } catch (error) {
        status.last_error = String(error);
        render();
      }
      return;
    }
    await copyText(project.path); showToast("Project path copied");
  });
  document.querySelector("#refresh-branch")?.addEventListener("click", async () => {
    await refreshActiveProject(true);
  });
  document.querySelectorAll<HTMLButtonElement>("[data-copy-recording-context]").forEach((button) => button.addEventListener("click", async (event) => {
    event.stopPropagation();
    const projectId = projectController.selectedProjectId;
    if (!projectId || !button.dataset.copyRecordingContext) return;
    const recordingId = button.dataset.copyRecordingContext;
    const context = await dictaClient.buildRecordingContext(projectId, recordingId);
    await copyText(context);
    openPacketMenu = null;
    showToast(`Context copied for ${recordingId}`);
  }));
  document.querySelector("#connect-mcp")?.addEventListener("click", async () => {
    try {
      mcpRestarting = mcpStatus.codex_configured;
      if (mcpRestarting) render();
      mcpStatus = dictaClient.isNative
        ? await invoke<McpStatus>(mcpStatus.codex_configured ? "restart_codex_mcp" : "configure_codex_mcp")
        : { installed: true, codex_configured: true, executable_path: platform.mcpExecutablePath, message: mcpRestarting ? "Dicta MCP restarted." : "Dicta is connected." };
      mcpRestarting = false;
      render();
      showToast(mcpStatus.message);
    } catch (error) {
      mcpRestarting = false;
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#use-current-time")?.addEventListener("click", () => {
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    setMarkedTime(video?.currentTime ?? viewerTime, false);
  });
  document.querySelector<HTMLTextAreaElement>("#timeline-note-input")?.addEventListener("input", (event) => {
    viewerNoteDraft = (event.target as HTMLTextAreaElement).value;
    if (!viewerListening) viewerNoteSource = "typed";
    const submit = document.querySelector<HTMLButtonElement>(".add-note-button");
    if (submit) submit.disabled = !viewerNoteDraft.trim();
  });
  document.querySelector("#dictate-note")?.addEventListener("click", toggleVoiceNote);
  document.querySelector("#timeline-note-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const recording = projectController.recordings.find((item) => item.id === viewingRecordingId);
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
    const recording = projectController.recordings.find((item) => item.id === viewingRecordingId);
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
    if (keyboardEvent.key === "Escape" && ui.recordingSearchOpen) {
      ui.recordingSearchOpen = false;
      ui.recordingQuery = "";
      render();
      return;
    }
    if (editing) return;
    const video = document.querySelector<HTMLVideoElement>("#packet-video");
    if (!video) return;
    if (keyboardEvent.key === " ") { keyboardEvent.preventDefault(); if (video.paused) void playViewerVideo(video); else video.pause(); }
    if (keyboardEvent.key.toLowerCase() === "m") { keyboardEvent.preventDefault(); setMarkedTime(video.currentTime); }
    if (keyboardEvent.key === "ArrowLeft") video.currentTime = Math.max(0, video.currentTime - 5);
    if (keyboardEvent.key === "ArrowRight") video.currentTime = Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + 5);
  });
  document.querySelectorAll<HTMLElement>("[data-copy-video]").forEach((node) => node.addEventListener("click", async () => {
    await copyText(node.dataset.copyVideo ?? ""); openPacketMenu = null; showToast("Video path copied");
  }));
  document.querySelector("#delete-recording-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const projectId = projectController.selectedProjectId;
    if (!projectId || !deleteRecordingId) return;
    const recordingId = deleteRecordingId;
    deleteRecordingId = null;
    try {
      await dictaClient.deleteRecording(projectId, recordingId);
      await refreshRecordings(projectId);
      const updated = await dictaClient.bootstrap();
      projectController.replaceProjects(updated.projects);
      render();
      showToast("Recording deleted");
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
  document.querySelector("#retranscribe-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const projectId = projectController.selectedProjectId;
    if (!projectId || !transcribeRecordingId) return;
    const recordingId = transcribeRecordingId;
    transcribeRecordingId = null;
    try {
      await dictaClient.retranscribeRecording(projectId, recordingId, selectedTranscriptionLanguage);
      await refreshRecordings(projectId);
      render();
      showToast(`Transcribing in ${transcriptionLanguages.find((item) => item.code === selectedTranscriptionLanguage)?.label ?? selectedTranscriptionLanguage}`);
    } catch (error) {
      status.last_error = String(error);
      render();
    }
  });
}

async function copyText(text: string): Promise<void> {
  if (dictaClient.isNative) await invoke("copy_to_clipboard", { text }); else await navigator.clipboard.writeText(text);
}

async function linkProjectFolder(): Promise<void> {
  try {
    const result = await dictaClient.linkProjectFromPicker();
    if (result.kind === "cancelled") return;
    if (result.kind === "manual") {
      createProjectOpen = true;
      render();
      return;
    }
    const { project } = result;
    projectController.upsertProject(project);
    projectController.select(project.id);
    status.active_project_id = project.id;
    status.last_error = null;
    await refreshRecordings();
    showToast(`${project.name} linked on ${project.git_branch ?? "Git"}`);
  } catch (error) {
    status.last_error = String(error);
    render();
  }
}

async function refreshActiveProject(showFeedback = false, projectId = projectController.selectedProjectId): Promise<boolean> {
  if (!projectId || ["preparing", "recording", "stopping"].includes(status.phase)) return false;
  const result = await projectController.refreshProject(projectId);
  if (!result.applied) return false;
  status.last_error = result.error;
  if (result.error || !showFeedback) render();
  else {
    const branchChanged = result.previousBranch && result.previousBranch !== result.project?.git_branch;
    showToast(branchChanged ? `Switched to ${result.project?.git_branch}` : `On ${result.project?.git_branch ?? "legacy storage"}`);
  }
  return true;
}

async function reveal(path?: string): Promise<void> {
  if (!path) return;
  if (dictaClient.isNative) await invoke("reveal_path", { path });
  else showToast(`${platform.revealLabel} opens in the desktop app`);
}

async function startRecording(note: string, projectId: string): Promise<void> {
  const startedStatus = await runAsyncAction(() => dictaClient.startRecording(projectId, note), {
    fallbackMessage: "Could not start recording",
    onError: (message) => {
      status.phase = "error";
      status.last_error = message;
      render();
    },
  });
  if (!startedStatus) return;
  // Native capture may emit `started` before this promise resolves. Never
  // replace that newer event state with an older `preparing` response.
  if (status.phase !== "recording" || startedStatus.phase === "recording") status = startedStatus;
  render();
}

async function stopRecording(): Promise<void> {
  try {
    status.phase = "stopping";
    render();
    const result = await dictaClient.stopRecording();
    if (!result.recording || !result.status) return;
    projectController.prependRecording(result.recording);
    status = result.status;
    showToast("Prompt packet created");
  } catch (error) {
    status.phase = "error"; status.last_error = String(error); render();
  }
}

async function refreshRecordings(projectId = projectController.selectedProjectId): Promise<void> {
  const result = await projectController.refreshRecordings(projectId);
  if (!result.applied || projectId !== projectController.selectedProjectId) return;
  if (result.error || status.phase === "idle") status.last_error = result.error;
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
    if (dictaClient.isNative) {
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
  const [initial, initialAppSettings] = await Promise.all([
    dictaClient.bootstrap(),
    dictaClient.getAppSettings(),
  ]);
  appSettings = initialAppSettings;
  selectedTranscriptionLanguage = appSettings.transcription_language;
  status = initial.status;
  const selectedProjectId = status.active_project_id
    ?? initial.projects.find((project) => project.id !== "__unprojected__")?.id
    ?? initial.projects[0]?.id
    ?? null;
  projectController.hydrate(initial.projects, selectedProjectId);
  if (selectedProjectId && selectedProjectId !== status.active_project_id) await dictaClient.selectProject(selectedProjectId);
  await refreshRecordings(selectedProjectId);

  if (dictaClient.isNative) {
    [mcpStatus, modelStatus] = await Promise.all([
      invoke<McpStatus>("mcp_status"),
      invoke<ModelStatus>("model_status"),
    ]);
    appLifecycle.add(await listen<RecorderEvent>("recorder-event", async ({ payload }) => {
      status = payload.status;
      if (payload.event === "stopping" && payload.message.includes("20-minute")) {
        showToast(payload.message);
      }
      if (["finished", "transcribed", "transcription_error", "error"].includes(payload.event)) {
        await refreshRecordings();
        const updated = await dictaClient.bootstrap();
        projectController.replaceProjects(updated.projects);
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
    }));
    appLifecycle.add(await listen<string>("project-selected", async ({ payload }) => {
      projectController.select(payload);
      status.active_project_id = payload;
      ui.activeView = "project";
      ui.projectPickerOpen = false;
      const handled = await refreshActiveProject(false, payload);
      if (projectController.selectedProjectId !== payload || handled) return;
      await refreshRecordings(payload);
      if (projectController.selectedProjectId === payload) render();
    }));
    appLifecycle.add(await listen<RecordingSelection>("recording-selected", () => {
      void consumePendingRecordingSelection();
    }));
    appLifecycle.add(await listen<ModelDownloadEvent>("model-download-progress", ({ payload }) => {
      modelDownload = payload;
      modelDownloading = payload.status === "downloading" || payload.status === "verifying";
      render();
    }));
    await consumePendingRecordingSelection();
  }
  render();
}

appLifecycle.add(mountDelegatedEvents(app, [
  {
    type: "keydown",
    selector: "[role='tab'][data-viewer-panel]",
    handle: (event, matched) => {
      handleViewerTabKeydown(event as KeyboardEvent, matched, (panel) => {
        viewerPanel = panel === "chapters" ? "chapters" : panel === "notes" ? "notes" : "transcript";
        render();
        window.requestAnimationFrame(() => app.querySelector<HTMLElement>(`#viewer-tab-${viewerPanel}`)?.focus({ preventScroll: true }));
      });
    },
  },
  {
    type: "click",
    selector: ".project-item",
    handle: async (_event, matched) => {
      if (matched.dataset.projectId) await browseProject(matched.dataset.projectId);
    },
  },
  {
    type: "click",
    selector: "[data-switch-project]",
    handle: async (_event, matched) => {
      if (matched.dataset.switchProject) await browseProject(matched.dataset.switchProject);
    },
  },
  {
    type: "click",
    selector: "[data-project-menu]",
    handle: (event, matched) => {
      event.stopPropagation();
      openProjectMenu = openProjectMenu === matched.dataset.projectMenu ? null : matched.dataset.projectMenu ?? null;
      openPacketMenu = null;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-project-reveal]",
    handle: (_event, matched) => {
      openProjectMenu = null;
      void reveal(matched.dataset.projectReveal);
    },
  },
  {
    type: "click",
    selector: "[data-project-copy-path]",
    handle: async (_event, matched) => {
      await copyText(matched.dataset.projectCopyPath ?? "");
      openProjectMenu = null;
      showToast("Project path copied");
    },
  },
  {
    type: "click",
    selector: "[data-remove-project]",
    handle: (_event, matched) => {
      removeProjectId = matched.dataset.removeProject ?? null;
      openProjectMenu = null;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-open-packet]",
    handle: (event, matched) => {
      event.stopPropagation();
      if (matched.dataset.openPacket) openPacket(matched.dataset.openPacket);
    },
  },
  {
    type: "click",
    selector: "[data-note-time], [data-transcript-time]",
    handle: (_event, matched) => {
      const video = app.querySelector<HTMLVideoElement>("#packet-video");
      if (!video) return;
      video.currentTime = Number(matched.dataset.noteTime ?? matched.dataset.transcriptTime ?? 0);
      void playViewerVideo(video);
    },
  },
  {
    type: "click",
    selector: "[data-viewer-panel]",
    handle: (_event, matched) => {
      const panel = matched.dataset.viewerPanel;
      viewerPanel = panel === "chapters" ? "chapters" : panel === "notes" ? "notes" : "transcript";
      render();
    },
  },
  {
    type: "click",
    selector: "[data-menu]",
    handle: (event, matched) => {
      event.stopPropagation();
      const recordingId = matched.dataset.menu ?? null;
      const surface = matched.dataset.menuSurface === "detail" ? "detail" : "index";
      const isOpen = openPacketMenu === recordingId && openPacketMenuSurface === surface;
      openPacketMenu = isOpen ? null : recordingId;
      openPacketMenuSurface = isOpen ? null : surface;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-reveal]",
    handle: (_event, matched) => { void reveal(matched.dataset.reveal); },
  },
  {
    type: "click",
    selector: "[data-transcribe]",
    handle: (_event, matched) => {
      const recording = projectController.recordings.find((item) => item.id === matched.dataset.transcribe);
      if (!recording) return;
      transcribeRecordingId = recording.id;
      selectedTranscriptionLanguage = recording.transcription_language ?? appSettings.transcription_language;
      openPacketMenu = null;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-delete]",
    handle: (_event, matched) => {
      deleteRecordingId = matched.dataset.delete ?? null;
      openPacketMenu = null;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-language]",
    handle: (_event, matched) => {
      selectedTranscriptionLanguage = matched.dataset.language ?? appSettings.transcription_language;
      render();
    },
  },
  {
    type: "click",
    selector: "[data-close-modal], [data-close-start], [data-close-transcribe], [data-close-delete], [data-close-remove-project]",
    handle: (event, matched) => {
      if (!shouldDismissModal(event, matched)) return;
      if (matched.matches("[data-close-modal]")) createProjectOpen = false;
      if (matched.matches("[data-close-start]")) startSheetOpen = false;
      if (matched.matches("[data-close-transcribe]")) transcribeRecordingId = null;
      if (matched.matches("[data-close-delete]")) deleteRecordingId = null;
      if (matched.matches("[data-close-remove-project]")) removeProjectId = null;
      render();
    },
  },
  {
    type: "input",
    selector: "#recording-search",
    handle: (_event, matched) => {
      ui.recordingQuery = (matched as HTMLInputElement).value;
      updateRecordingSearchResults();
    },
  },
  {
    type: "click",
    selector: "[data-settings-section]",
    handle: (_event, matched) => {
      const section = matched.dataset.settingsSection;
      ui.settingsSection = section === "connections" || section === "shortcuts" || section === "transcription" || section === "storage" ? section : "appearance";
      app.querySelectorAll<HTMLButtonElement>("[data-settings-section]").forEach((item) => item.classList.toggle("selected", item.dataset.settingsSection === ui.settingsSection));
      app.querySelector<HTMLElement>(`#${ui.settingsSection}-settings`)?.scrollIntoView({ behavior: "smooth", block: "start" });
    },
  },
]));

initialize().catch((error) => { app.innerHTML = `<div class="fatal"><strong>Dicta could not start.</strong><pre>${escapeHtml(String(error))}</pre></div>`; });

appLifecycle.listen(window, "focus", () => {
  void (async () => {
    if (dictaClient.isNative && await consumePendingRecordingSelection()) return;
    await refreshActiveProject();
  })();
});
const systemThemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
appLifecycle.listen(systemThemeQuery, "change", () => {
  if (themePreference === "system") {
    applyTheme();
  }
});

appLifecycle.add(() => {
  appDisposed = true;
  if (elapsedTimer !== null) window.clearInterval(elapsedTimer);
  if (mockModelTimer !== null) window.clearTimeout(mockModelTimer);
  if (toastTimer !== null) window.clearTimeout(toastTimer);
  if (voiceStopTimer !== null) window.clearTimeout(voiceStopTimer);
  activeSpeechRecognition?.abort();
  if (activeVoiceRecorder?.state === "recording") activeVoiceRecorder.stop();
  activeVoiceStream?.getTracks().forEach((track) => track.stop());
  viewerVideoCache.dispose();
});
appLifecycle.listen(window, "pagehide", () => appLifecycle.dispose(), { once: true });
import.meta.hot?.dispose(() => appLifecycle.dispose());
