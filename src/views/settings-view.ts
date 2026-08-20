import type { PlatformCapabilities } from "../platform";
import type { AppSettings, CleanupSummary, McpStatus, ModelDownloadEvent, ModelStatus } from "../types";
import { escapeHtml, formatBytes } from "./view-helpers";

export type SettingsSection = "appearance" | "connections" | "shortcuts" | "transcription" | "storage";
export type ThemePreference = "system" | "light" | "dark";

export interface SettingsViewModel {
  open: boolean;
  settingsSection: SettingsSection;
  platform: PlatformCapabilities;
  themePreference: ThemePreference;
  mcpStatus: McpStatus;
  mcpRestarting: boolean;
  codexLightUrl: string;
  codexDarkUrl: string;
  appSettings: AppSettings;
  transcriptionLanguages: Array<{ code: string; label: string; native: string }>;
  modelStatus: ModelStatus;
  modelDownloading: boolean;
  modelDownload: ModelDownloadEvent | null;
  cleanupSummary: CleanupSummary | null;
  cleanupRunning: boolean;
  activeProjectIsGit: boolean;
}

export function renderSettings(vm: SettingsViewModel): string {
  if (!vm.open) return "";
  const {
    settingsSection, platform, themePreference, mcpStatus, mcpRestarting,
    codexLightUrl, codexDarkUrl, appSettings, transcriptionLanguages,
    modelStatus, modelDownloading, modelDownload, cleanupSummary,
    cleanupRunning, activeProjectIsGit,
  } = vm;
  return `
          <section class="settings-page" aria-label="Settings">
            <div class="settings-layout">
              <nav class="settings-nav" aria-label="Settings sections">
                <button class="${settingsSection === "appearance" ? "selected" : ""}" data-settings-section="appearance"><i class="ph ph-palette"></i><span>Appearance</span></button>
                <button class="${settingsSection === "connections" ? "selected" : ""}" data-settings-section="connections"><i class="ph ph-plugs-connected"></i><span>Local AI</span></button>
                <button class="${settingsSection === "shortcuts" ? "selected" : ""}" data-settings-section="shortcuts"><i class="ph ph-keyboard"></i><span>Shortcuts</span></button>
                <button class="${settingsSection === "transcription" ? "selected" : ""}" data-settings-section="transcription"><i class="ph ph-waveform"></i><span>Transcription</span></button>
                <button class="${settingsSection === "storage" ? "selected" : ""}" data-settings-section="storage"><i class="ph ph-hard-drives"></i><span>Storage</span></button>
              </nav>

              <div class="settings-content">
                <section class="settings-section-block" id="appearance-settings">
                  <div class="settings-content-heading">
                    <h2>Appearance</h2>
                    <p>Choose how Dicta looks on this ${platform.displayName}.</p>
                  </div>
                  <div class="settings-group" aria-label="Theme">
                    <div class="settings-group-label">Theme</div>
                    <div class="theme-options" role="radiogroup" aria-label="Theme">
                      ${([
                        ["system", "ph-desktop", "System", platform.systemThemeDetail],
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

                <section class="settings-section-block" id="connections-settings">
                  <div class="settings-content-heading">
                    <h2>Local AI connections</h2>
                    <p>Choose which local AI tools can use Dicta context.</p>
                  </div>
                  <div class="settings-group" aria-label="Model Context Protocol connections">
                    <button class="connection-tile ${mcpStatus.codex_configured ? "connected" : ""}" id="connect-mcp" ${mcpRestarting ? "disabled" : ""}>
                      <span class="codex-icon-wrap"><img class="codex-icon codex-icon-light" src="${codexLightUrl}" alt="" /><img class="codex-icon codex-icon-dark" src="${codexDarkUrl}" alt="" /></span>
                      <span class="connection-tile-copy"><strong>Codex</strong><small>${mcpRestarting ? "Restarting…" : mcpStatus.codex_configured ? "Connected" : "Connect"}</small></span>
                      <span class="connection-tile-state ${mcpStatus.codex_configured ? "connected" : ""}">${mcpStatus.codex_configured ? '<i class="ph ph-check-circle"></i>' : '<i class="ph ph-arrow-right"></i>'}</span>
                    </button>
                    <div class="coming-soon-row"><span>More local AI tools</span><small>Coming soon</small></div>
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
                      ${platform.shortcutOptions.map((shortcut) => `
                        <button class="shortcut-option ${appSettings.shortcut_id === shortcut.id ? "selected" : ""}" type="button" data-shortcut-choice="${shortcut.id}" role="radio" aria-checked="${appSettings.shortcut_id === shortcut.id}">
                          <span><strong>${escapeHtml(shortcut.label)}</strong><small>${escapeHtml(shortcut.detail)}</small></span>
                          <i class="ph ${appSettings.shortcut_id === shortcut.id ? "ph-check-circle" : "ph-circle"}"></i>
                        </button>
                      `).join("")}
                    </div>
                    <p class="settings-help"><i class="ph ph-info"></i>${platform.shortcutHelp}</p>
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
                        <span><i class="ph ph-lock-key"></i>Runs entirely on your ${platform.displayName}</span>
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
                    <div><strong>${escapeHtml(modelStatus.active_model)}</strong><small>${escapeHtml(platform.compactPath(modelStatus.active_model_path))}</small></div>
                    <span class="active-badge">Active</span>
                  </div>
                  <p class="engine-message">${escapeHtml(modelStatus.message)}</p>
                </section>

                </section>

                <section class="settings-section-block" id="storage-settings">
                  <div class="settings-content-heading">
                    <h2>Storage</h2>
                    <p>Choose how Git recordings are scoped and remove large files after a branch has landed.</p>
                  </div>
                  <div class="settings-group" aria-label="Git recording scope">
                    <div class="settings-group-label">Recording scope</div>
                    <article class="preference-row">
                      <div class="preference-icon"><i class="ph ph-git-branch"></i></div>
                      <div class="preference-copy"><strong>Lock recordings to Git branches</strong><p>Turn this off to make new project recordings available from every branch in the repository.</p></div>
                      <button class="switch ${appSettings.branch_locking ? "on" : ""}" type="button" id="branch-lock-settings-toggle" role="switch" aria-checked="${appSettings.branch_locking}" aria-label="Lock recordings to Git branches"><span></span></button>
                    </article>
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
                      <button class="secondary-action" id="cleanup-now" ${!activeProjectIsGit || !appSettings.cleanup_merged_videos || cleanupRunning ? "disabled" : ""}><i class="ph ${cleanupRunning ? "ph-spinner-gap mcp-spin" : "ph-broom"}"></i>${cleanupRunning ? "Checking…" : "Clean"}</button>
                    </div>
                  </div>
                </section>
              </div>
            </div>
          </section>

  `;
}
