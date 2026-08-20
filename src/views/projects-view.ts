import type { PlatformCapabilities } from "../platform";
import type { Project, Recording } from "../types";
import { escapeHtml, recordingTitle, scopeLabel } from "./view-helpers";

export interface ProjectsViewModel {
  platform: PlatformCapabilities;
  markUrl: string;
  markLightUrl: string;
  projects: Project[];
  selectedProjectId: string | null;
  project?: Project;
  latestRecording?: Recording;
  activeView: "project" | "settings";
  isBusy: boolean;
  branchLocking: boolean;
  branchUnavailable: boolean;
  openProjectMenu: string | null;
  projectPickerOpen: boolean;
  recordingSearchOpen: boolean;
  recordingQuery: string;
  statusError: string | null;
}

export function renderPlatformChrome(vm: ProjectsViewModel): string {
  if (!vm.platform.isLinux) return "";
  return `<header class="linux-titlebar" data-tauri-drag-region>
    <div class="linux-titlebar-brand" data-tauri-drag-region>${renderMarks(vm)}<strong data-tauri-drag-region>Dicta</strong></div>
    <div class="linux-titlebar-drag" data-tauri-drag-region></div>
    <button class="linux-titlebar-close" id="window-close" type="button" aria-label="Close Dicta" title="Close"><i class="ph ph-x" aria-hidden="true"></i></button>
  </header>`;
}

export function renderProjectsSidebar(vm: ProjectsViewModel): string {
  return `<aside class="sidebar">
    ${vm.platform.isMac ? `<div class="sidebar-chrome-space" data-tauri-drag-region></div>` : ""}
    <div class="sidebar-brand" data-tauri-drag-region>${renderMarks(vm)}<strong data-tauri-drag-region>Dicta</strong></div>
    <div class="sidebar-section-label">Projects</div>
    <nav class="project-list" aria-label="Projects">
      ${vm.projects.length === 0 ? `<div class="sidebar-empty">No projects yet</div>` : vm.projects.map((project) => renderProjectItem(vm, project)).join("")}
    </nav>
    <button class="sidebar-new-project" id="new-project" ${vm.isBusy ? "disabled" : ""}><i class="ph ph-plus"></i><span>New project</span></button>
    <section class="sidebar-recents" aria-labelledby="recents-title">
      <div class="sidebar-section-label" id="recents-title">Recents</div>
      ${vm.latestRecording ? `<button class="recent-item" data-open-packet="${escapeHtml(vm.latestRecording.id)}" title="Open ${escapeHtml(vm.latestRecording.id)}"><i class="ph ph-clock" aria-hidden="true"></i><span>${escapeHtml(recordingTitle(vm.latestRecording))}</span></button>` : `<div class="recent-item recent-item-empty"><i class="ph ph-clock" aria-hidden="true"></i><span>No recent recordings</span></div>`}
    </section>
    <div class="sidebar-actions"><button class="sidebar-settings ${vm.activeView === "settings" ? "selected" : ""}" id="open-settings" aria-pressed="${vm.activeView === "settings"}" ${vm.isBusy ? "disabled" : ""}><i class="ph ph-gear-six" aria-hidden="true"></i><span>Settings</span></button></div>
  </aside>`;
}

export function renderProjectHeader(vm: ProjectsViewModel): string {
  const project = vm.project;
  return `<header class="project-header split-project-header ${vm.recordingSearchOpen ? "searching" : ""}" data-tauri-drag-region>
    <div class="project-heading" data-tauri-drag-region>
      <div class="project-switcher-wrap">
        <button class="project-switcher" id="project-switcher" type="button" aria-haspopup="listbox" aria-expanded="${vm.projectPickerOpen}"><span>${escapeHtml(project?.name ?? "Choose a project")}</span><i class="ph ph-caret-down"></i></button>
        ${vm.projectPickerOpen ? `<div class="project-switcher-menu" role="listbox">${vm.projects.map((item) => `<button type="button" role="option" aria-selected="${item.id === vm.selectedProjectId}" data-switch-project="${escapeHtml(item.id)}"><i class="ph ${item.id === "__unprojected__" ? "ph-tray" : "ph-folder"}"></i><span><strong>${escapeHtml(item.name)}</strong>${item.id === "__unprojected__" ? "" : `<small>${escapeHtml(vm.branchLocking ? item.git_branch ?? "Git unavailable" : "Repository-wide")}</small>`}</span>${item.id === vm.selectedProjectId ? '<i class="ph ph-check"></i>' : ""}</button>`).join("")}</div>` : ""}
      </div>
      <div class="project-context">
        <button class="path-button" id="copy-path" ${project ? "" : "disabled"} title="${project?.id === "__unprojected__" ? "Change the General recordings folder" : "Copy working-copy path"}"><i class="ph ph-folder" aria-hidden="true"></i><span>${project ? escapeHtml(vm.platform.compactPath(project.path)) : "Link a Git project to begin"}</span>${project ? `<i class="ph ${project.id === "__unprojected__" ? "ph-pencil-simple" : "ph-copy"}" aria-hidden="true"></i>` : ""}</button>
        ${project && project.id !== "__unprojected__" ? `<button class="branch-pill ${vm.branchUnavailable ? "unavailable" : ""}" id="refresh-branch" title="Refresh recording scope"><i class="ph ${vm.branchLocking ? "ph-git-branch" : "ph-git-fork"}"></i><span>${escapeHtml(scopeLabel(project, vm.branchLocking))}</span></button>` : ""}
      </div>
    </div>
    <div class="split-header-actions"><div class="recording-search ${vm.recordingSearchOpen ? "open" : ""}">${vm.recordingSearchOpen ? '<i class="ph ph-magnifying-glass search-field-icon" aria-hidden="true"></i>' : ""}<input id="recording-search" type="search" value="${escapeHtml(vm.recordingQuery)}" placeholder="Search IDs, notes, and transcripts" aria-label="Search recording IDs, notes, and transcripts" /><button id="toggle-recording-search" type="button" aria-label="${vm.recordingSearchOpen ? "Close recording search" : "Search recordings"}"><i class="ph ${vm.recordingSearchOpen ? "ph-x" : "ph-magnifying-glass"}"></i></button></div></div>
    ${vm.statusError ? `<div class="error-banner"><i class="ph ph-warning-circle"></i><span>${escapeHtml(vm.statusError)}</span></div>` : ""}
  </header>`;
}

function renderMarks(vm: ProjectsViewModel): string {
  return `<img class="dicta-mark-default" src="${vm.markUrl}" alt="" aria-hidden="true" data-tauri-drag-region /><img class="dicta-mark-dark" src="${vm.markLightUrl}" alt="" aria-hidden="true" data-tauri-drag-region />`;
}

function renderProjectItem(vm: ProjectsViewModel, project: Project): string {
  const isGeneral = project.id === "__unprojected__";
  return `<div class="project-entry ${vm.openProjectMenu === project.id ? "menu-open" : ""}">
    <button class="project-item ${vm.activeView === "project" && project.id === vm.selectedProjectId ? "selected" : ""}" data-project-id="${escapeHtml(project.id)}"><i class="ph ${isGeneral ? "ph-tray" : "ph-folder"}" aria-hidden="true"></i><span class="project-label"><span>${escapeHtml(project.name)}</span>${isGeneral ? "" : `<small>${escapeHtml(vm.branchLocking ? project.git_branch ?? "Git unavailable" : "Repository-wide")}</small>`}</span></button>
    ${isGeneral ? "" : `<button class="project-more" type="button" data-project-menu="${escapeHtml(project.id)}" aria-label="Project actions for ${escapeHtml(project.name)}" ${vm.isBusy ? "disabled" : ""}><i class="ph ph-dots-three"></i></button>`}
    ${!isGeneral && vm.openProjectMenu === project.id ? `<div class="packet-menu project-menu"><button data-project-reveal="${escapeHtml(project.source_path ?? project.storage_path)}"><i class="ph ph-folder-open"></i>${vm.platform.revealLabel}</button><button data-project-copy-path="${escapeHtml(project.path)}"><i class="ph ph-copy"></i>Copy path</button><span class="packet-menu-divider"></span><button class="danger" data-remove-project="${escapeHtml(project.id)}"><i class="ph ph-minus-circle"></i>Remove from Dicta</button></div>` : ""}
  </div>`;
}
