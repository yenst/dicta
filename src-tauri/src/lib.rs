use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    ffi::{CStr, CString},
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::c_char,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static LOCAL_TRANSCRIBER: OnceLock<Mutex<()>> = OnceLock::new();
static WHISPER_MODEL: OnceLock<Mutex<Option<LoadedWhisper>>> = OnceLock::new();

const QUALITY_MODEL_FILENAME: &str = "ggml-large-v3-turbo-q5_0.bin";
const MAX_RECORDING_SECONDS: u64 = 20 * 60;
const TRAY_ID: &str = "dicta";
const ALLOWED_LANGUAGES: [&str; 6] = ["auto", "nl", "en", "fr", "de", "es"];
const DEFAULT_LANGUAGE: &str = "auto";
const QUALITY_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
const QUALITY_MODEL_SHA1: &str = "e050f7970618a659205450ad97eb95a18d69c9ee";
const QUALITY_MODEL_DOWNLOAD_BYTES: u64 = 547 * 1024 * 1024;
const DEFAULT_SHORTCUT_ID: &str = "command_shift_r";

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dicta_start(
        output_path: *const c_char,
        callback: extern "C" fn(*const c_char, *const c_char),
    );
    fn dicta_stop(callback: extern "C" fn(*const c_char, *const c_char));
    fn dicta_transcribe(
        input_path: *const c_char,
        language: *const c_char,
        callback: extern "C" fn(*const c_char, *const c_char),
    );
    fn dicta_extract_audio(input_path: *const c_char, output_path: *const c_char) -> bool;
    fn dicta_extract_poster(input_path: *const c_char, output_path: *const c_char) -> bool;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProjectFile {
    id: String,
    name: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    source_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Project {
    id: String,
    name: String,
    path: String,
    storage_path: String,
    source_path: Option<String>,
    git_branch: Option<String>,
    branch_path: Option<String>,
    is_git: bool,
    git_error: Option<String>,
    created_at: DateTime<Utc>,
    recording_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Recording {
    id: String,
    project_id: String,
    video_path: String,
    metadata_path: String,
    note: String,
    #[serde(default)]
    git_branch: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    duration_seconds: Option<f64>,
    size_bytes: Option<u64>,
    success: bool,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    transcription_status: String,
    #[serde(default)]
    transcription_error: Option<String>,
    #[serde(default)]
    transcription_language: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    timeline_notes: Vec<TimelineNote>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct TranscriptSegment {
    start_seconds: f64,
    end_seconds: f64,
    text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TimelineNote {
    id: String,
    timestamp_seconds: f64,
    text: String,
    created_at: DateTime<Utc>,
    #[serde(default = "typed_note_source")]
    source: String,
}

fn typed_note_source() -> String {
    "typed".to_string()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum RecordingPhase {
    Idle,
    Preparing,
    Recording,
    Stopping,
    Error,
}

#[derive(Clone, Debug, Serialize)]
struct RecorderStatus {
    phase: RecordingPhase,
    active_project_id: Option<String>,
    active_video_path: Option<String>,
    started_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Bootstrap {
    root_path: String,
    projects: Vec<Project>,
    status: RecorderStatus,
}

#[derive(Clone, Debug, Serialize)]
struct McpStatus {
    installed: bool,
    codex_configured: bool,
    executable_path: String,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
struct ModelStatus {
    bundled_ready: bool,
    quality_installed: bool,
    quality_path: String,
    quality_size_bytes: u64,
    download_size_bytes: u64,
    active_model: String,
    active_model_path: String,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
struct ModelDownloadEvent {
    downloaded_bytes: u64,
    total_bytes: u64,
    progress: f64,
    status: String,
    message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default = "default_shortcut_id")]
    shortcut_id: String,
    #[serde(default = "enabled_by_default")]
    cleanup_merged_videos: bool,
    #[serde(default = "default_language")]
    transcription_language: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut_id: default_shortcut_id(),
            cleanup_merged_videos: true,
            transcription_language: default_language(),
        }
    }
}

struct LoadedWhisper {
    path: PathBuf,
    context: WhisperContext,
}

#[derive(Clone, Debug, Default, Serialize)]
struct CleanupSummary {
    removed_files: usize,
    freed_bytes: u64,
    cleaned_branches: Vec<String>,
    default_branch: Option<String>,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct BranchMetadata {
    git_branch: String,
    #[serde(default)]
    head_oid: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct RecorderEventPayload {
    event: String,
    message: String,
    status: RecorderStatus,
}

#[derive(Debug, Deserialize)]
struct NativeTranscriptionPayload {
    path: String,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    error: Option<String>,
}

struct LocalTranscript {
    transcript: String,
    segments: Vec<TranscriptSegment>,
}

struct InnerState {
    status: RecorderStatus,
    session: Option<Recording>,
    last_note: String,
}

struct AppState {
    root: PathBuf,
    inner: Mutex<InnerState>,
}

impl AppState {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Mutex::new(InnerState {
                status: RecorderStatus {
                    phase: RecordingPhase::Idle,
                    active_project_id: None,
                    active_video_path: None,
                    started_at: None,
                    last_error: None,
                },
                session: None,
                last_note: String::new(),
            }),
        }
    }
}

fn default_shortcut_id() -> String {
    DEFAULT_SHORTCUT_ID.to_string()
}

fn enabled_by_default() -> bool {
    true
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}

fn is_allowed_language(language: &str) -> bool {
    ALLOWED_LANGUAGES.contains(&language)
}

fn normalize_settings(mut settings: AppSettings) -> AppSettings {
    if shortcut_for_id(&settings.shortcut_id).is_none() {
        settings.shortcut_id = default_shortcut_id();
    }
    if !is_allowed_language(&settings.transcription_language) {
        settings.transcription_language = default_language();
    }
    settings
}

fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

fn read_settings(root: &Path) -> AppSettings {
    fs::read_to_string(settings_path(root))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .map(normalize_settings)
        .unwrap_or_default()
}

fn settings_language(root: &Path) -> String {
    read_settings(root).transcription_language
}

fn write_settings(root: &Path, settings: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("Could not serialize Dicta settings: {error}"))?;
    fs::write(settings_path(root), format!("{json}\n"))
        .map_err(|error| format!("Could not save Dicta settings: {error}"))
}

fn shortcut_for_id(id: &str) -> Option<Shortcut> {
    match id {
        "command_shift_r" => Some(Shortcut::new(
            Some(Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyR,
        )),
        "command_shift_d" => Some(Shortcut::new(
            Some(Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyD,
        )),
        "option_space" => Some(Shortcut::new(Some(Modifiers::ALT), Code::Space)),
        "control_space" => Some(Shortcut::new(Some(Modifiers::CONTROL), Code::Space)),
        _ => None,
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn models_dir() -> Result<PathBuf, String> {
    let data_dir =
        dirs::data_local_dir().ok_or_else(|| "Could not locate Application Support".to_string())?;
    Ok(data_dir.join("Dicta").join("models"))
}

fn quality_model_path() -> Result<PathBuf, String> {
    Ok(models_dir()?.join(QUALITY_MODEL_FILENAME))
}

fn bundled_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not locate Dicta resources: {error}"))?
        .join("ggml-base-q5_1.bin"))
}

fn whisper_model_candidates(app: &AppHandle) -> Result<Vec<PathBuf>, String> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("DICTA_WHISPER_MODEL") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(models) = models_dir() {
        candidates.push(models.join("ggml-large-v3-turbo.bin"));
        candidates.push(models.join(QUALITY_MODEL_FILENAME));
        candidates.push(models.join("ggml-medium.bin"));
    }
    candidates.push(bundled_model_path(app)?);
    Ok(candidates)
}

fn selected_whisper_model(app: &AppHandle) -> Result<PathBuf, String> {
    whisper_model_candidates(app)?
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "Dicta's local Whisper model is missing".to_string())
}

fn model_label(path: &Path) -> String {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if filename.contains("large-v3-turbo") {
        "High quality · large-v3-turbo".to_string()
    } else if filename.contains("medium") {
        "Enhanced · medium".to_string()
    } else {
        "Compact · base".to_string()
    }
}

fn current_model_status(app: &AppHandle) -> Result<ModelStatus, String> {
    let quality_path = quality_model_path()?;
    let quality_size_bytes = fs::metadata(&quality_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let quality_installed = quality_path.is_file() && quality_size_bytes > 500 * 1024 * 1024;
    let bundled = bundled_model_path(app)?;
    let active = selected_whisper_model(app)?;
    Ok(ModelStatus {
        bundled_ready: bundled.is_file(),
        quality_installed,
        quality_path: path_string(&quality_path),
        quality_size_bytes,
        download_size_bytes: QUALITY_MODEL_DOWNLOAD_BYTES,
        active_model: model_label(&active),
        active_model_path: path_string(&active),
        message: if quality_installed {
            "The high-quality model is installed and ready.".to_string()
        } else if active
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("large-v3-turbo"))
        {
            "Dicta is using a high-quality model found on this Mac. Install a Dicta-managed copy for a portable setup.".to_string()
        } else {
            "The compact offline model is active. Download high quality for better Dutch and technical speech.".to_string()
        },
    })
}

fn emit_model_download(
    app: &AppHandle,
    downloaded_bytes: u64,
    total_bytes: u64,
    status: &str,
    message: &str,
) {
    let progress = if total_bytes == 0 {
        0.0
    } else {
        (downloaded_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0)
    };
    let _ = app.emit(
        "model-download-progress",
        ModelDownloadEvent {
            downloaded_bytes,
            total_bytes,
            progress,
            status: status.to_string(),
            message: message.to_string(),
        },
    );
}

#[tauri::command]
fn model_status(app: AppHandle) -> Result<ModelStatus, String> {
    current_model_status(&app)
}

#[tauri::command]
async fn download_quality_model(app: AppHandle) -> Result<ModelStatus, String> {
    if current_model_status(&app)?.quality_installed {
        return current_model_status(&app);
    }
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let target = quality_model_path()?;
        let parent = target
            .parent()
            .ok_or_else(|| "Invalid model installation path".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create Dicta's model folder: {error}"))?;
        let staging = parent.join(format!(".{QUALITY_MODEL_FILENAME}.download"));
        let _ = fs::remove_file(&staging);

        emit_model_download(
            &task_app,
            0,
            QUALITY_MODEL_DOWNLOAD_BYTES,
            "downloading",
            "Downloading the high-quality model…",
        );
        let mut child = std::process::Command::new("/usr/bin/curl")
            .args([
                "--location",
                "--fail",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&staging)
            .arg(QUALITY_MODEL_URL)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not start the model download: {error}"))?;

        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("Could not monitor the model download: {error}"))?
            {
                break status;
            }
            let downloaded = fs::metadata(&staging)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            emit_model_download(
                &task_app,
                downloaded,
                QUALITY_MODEL_DOWNLOAD_BYTES,
                "downloading",
                "Downloading the high-quality model…",
            );
            std::thread::sleep(std::time::Duration::from_millis(250));
        };

        if !status.success() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            let _ = fs::remove_file(&staging);
            let message = if stderr.trim().is_empty() {
                "The model download failed. Check your internet connection and try again."
                    .to_string()
            } else {
                format!("The model download failed: {}", stderr.trim())
            };
            emit_model_download(
                &task_app,
                0,
                QUALITY_MODEL_DOWNLOAD_BYTES,
                "error",
                &message,
            );
            return Err(message);
        }

        let downloaded = fs::metadata(&staging)
            .map_err(|error| format!("Could not inspect the downloaded model: {error}"))?
            .len();
        emit_model_download(
            &task_app,
            downloaded,
            downloaded,
            "verifying",
            "Verifying the model…",
        );
        let checksum = std::process::Command::new("/usr/bin/shasum")
            .args(["-a", "1"])
            .arg(&staging)
            .output()
            .map_err(|error| format!("Could not verify the downloaded model: {error}"))?;
        let actual_sha1 = String::from_utf8_lossy(&checksum.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if !checksum.status.success() || actual_sha1 != QUALITY_MODEL_SHA1 {
            let _ = fs::remove_file(&staging);
            let message =
                "The downloaded model did not pass integrity verification. Nothing was installed."
                    .to_string();
            emit_model_download(&task_app, 0, downloaded, "error", &message);
            return Err(message);
        }

        if target.exists() {
            fs::remove_file(&target)
                .map_err(|error| format!("Could not replace the previous model: {error}"))?;
        }
        fs::rename(&staging, &target)
            .map_err(|error| format!("Could not install the verified model: {error}"))?;
        emit_model_download(
            &task_app,
            downloaded,
            downloaded,
            "complete",
            "High-quality transcription is ready.",
        );
        Ok(())
    })
    .await
    .map_err(|error| format!("The model download task stopped unexpectedly: {error}"))??;
    current_model_status(&app)
}

fn mcp_install_path() -> Result<PathBuf, String> {
    let data_dir =
        dirs::data_local_dir().ok_or_else(|| "Could not locate Application Support".to_string())?;
    Ok(data_dir.join("Dicta").join("bin").join("dicta-mcp"))
}

fn atomic_install_binary(resource: &Path, target: &Path) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Invalid MCP installation path".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create the Dicta MCP folder: {error}"))?;
    let staging = parent.join(format!(".dicta-mcp-installing-{}", std::process::id()));
    fs::copy(resource, &staging)
        .map_err(|error| format!("Could not stage the Dicta MCP server: {error}"))?;
    if let Err(error) = fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Could not make the Dicta MCP server executable: {error}"
        ));
    }
    if let Err(error) = fs::rename(&staging, target) {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Could not atomically install the Dicta MCP server: {error}"
        ));
    }
    Ok(())
}

fn install_mcp_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let target = mcp_install_path()?;
    let resource = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not locate Dicta resources: {error}"))?
        .join("dicta-mcp");
    if resource.exists() {
        atomic_install_binary(&resource, &target)?;
    }
    if target.exists() {
        Ok(target)
    } else {
        Err("The Dicta MCP server is not bundled in this build".to_string())
    }
}

fn find_codex_command() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("codex");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".bun/bin/codex"));
        candidates.push(home.join(".npm-global/bin/codex"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn codex_has_dicta(codex: &Path, executable: &Path) -> bool {
    let Ok(output) = std::process::Command::new(codex)
        .args(["mcp", "get", "dicta", "--json"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()
        .and_then(|value| {
            value
                .pointer("/transport/command")
                .or_else(|| value.get("command"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .is_some_and(|command| command == path_string(executable))
}

fn register_codex_mcp(codex: &Path, executable: &Path, force_reload: bool) -> Result<(), String> {
    if force_reload || !codex_has_dicta(codex, executable) {
        let _ = std::process::Command::new(codex)
            .args(["mcp", "remove", "dicta"])
            .output();
        if force_reload {
            // Give Codex's config watcher time to observe the disabled state before
            // restoring the server. This replaces closed transports in loaded tasks.
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let output = std::process::Command::new(codex)
            .args(["mcp", "add", "dicta", "--"])
            .arg(executable)
            .output()
            .map_err(|error| format!("Could not configure Codex: {error}"))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if message.is_empty() {
                "Codex could not save the Dicta MCP configuration".to_string()
            } else {
                message
            });
        }
    }
    Ok(())
}

fn connected_mcp_status(executable: &Path, message: &str) -> McpStatus {
    McpStatus {
        installed: true,
        codex_configured: true,
        executable_path: path_string(executable),
        message: message.to_string(),
    }
}

#[tauri::command]
fn mcp_status(app: AppHandle) -> Result<McpStatus, String> {
    let executable = mcp_install_path()?;
    let installed = executable.is_file();
    let codex_configured = find_codex_command()
        .as_deref()
        .is_some_and(|codex| installed && codex_has_dicta(codex, &executable));
    Ok(McpStatus {
        installed,
        codex_configured,
        executable_path: path_string(&executable),
        message: if codex_configured {
            "Dicta is connected to Codex".to_string()
        } else if installed {
            "The Dicta MCP server is ready to connect".to_string()
        } else {
            let _ = app;
            "The Dicta MCP server is not installed".to_string()
        },
    })
}

#[tauri::command]
fn configure_codex_mcp(app: AppHandle) -> Result<McpStatus, String> {
    let executable = install_mcp_binary(&app)?;
    let codex = find_codex_command().ok_or_else(|| {
        format!(
            "Codex CLI was not found. Run: codex mcp add dicta -- {}",
            executable.display()
        )
    })?;
    register_codex_mcp(&codex, &executable, false)?;
    Ok(connected_mcp_status(
        &executable,
        "Dicta is connected to Codex.",
    ))
}

#[tauri::command]
fn restart_codex_mcp(app: AppHandle) -> Result<McpStatus, String> {
    let executable = install_mcp_binary(&app)?;
    let codex = find_codex_command().ok_or_else(|| {
        "Codex CLI was not found. Open Codex Settings → Plugins → MCPs to restart Dicta."
            .to_string()
    })?;
    register_codex_mcp(&codex, &executable, true)?;
    Ok(connected_mcp_status(
        &executable,
        "Dicta MCP restarted. Existing Codex tasks are reconnecting now.",
    ))
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in name.trim().to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

fn project_dir(root: &Path, project_id: &str) -> PathBuf {
    root.join(project_id)
}

fn linked_storage_dir(metadata: &ProjectFile) -> Option<PathBuf> {
    metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .map(|source| source.join(".dicta"))
}

fn project_storage_dir(root: &Path, metadata: &ProjectFile) -> PathBuf {
    linked_storage_dir(metadata).unwrap_or_else(|| project_dir(root, &metadata.id))
}

fn copy_directory_missing(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("Could not read {}: {error}", source.display()))?
        .flatten()
    {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_missing(&source_path, &destination_path)?;
        } else if !destination_path.exists() {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                format!(
                    "Could not copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn rewrite_migrated_paths(directory: &Path, old_root: &Path, new_root: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rewrite_migrated_paths(&path, old_root, new_root);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let mut changed = false;
        if value.get("metadata_path").is_some() {
            let local_metadata_path = path_string(&path);
            if value.get("metadata_path").and_then(|item| item.as_str())
                != Some(local_metadata_path.as_str())
            {
                value["metadata_path"] = serde_json::Value::String(local_metadata_path);
                changed = true;
            }
        }
        let local_video_path = path.with_extension("mp4");
        if local_video_path.is_file() {
            let local_video_path = path_string(&local_video_path);
            if value.get("video_path").and_then(|item| item.as_str())
                != Some(local_video_path.as_str())
            {
                value["video_path"] = serde_json::Value::String(local_video_path);
                changed = true;
            }
        }
        for key in ["video_path", "metadata_path", "transcript_path"] {
            let Some(raw_path) = value.get(key).and_then(|item| item.as_str()) else {
                continue;
            };
            let original = Path::new(raw_path);
            let Ok(relative) = original.strip_prefix(old_root) else {
                continue;
            };
            value[key] = serde_json::Value::String(path_string(&new_root.join(relative)));
            changed = true;
        }
        if changed {
            if let Ok(json) = serde_json::to_string_pretty(&value) {
                let _ = fs::write(&path, format!("{json}\n"));
            }
        }
    }
}

fn exclude_dicta_from_git(source: &Path) -> Result<(), String> {
    let raw_path = git_output(source, &["rev-parse", "--git-path", "info/exclude"])?;
    let exclude_path = {
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            path
        } else {
            source.join(path)
        }
    };
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".dicta/") {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not open Git exclude rules: {error}"))?;
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(
        &exclude_path,
        format!("{existing}{separator}# Dicta local recordings\n.dicta/\n"),
    )
    .map_err(|error| format!("Could not exclude .dicta from Git: {error}"))
}

fn prepare_linked_storage(root: &Path, metadata: &ProjectFile) -> Result<PathBuf, String> {
    let source = metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "This project is not linked to Git".to_string())?;
    let destination = source.join(".dicta");
    fs::create_dir_all(&destination)
        .map_err(|error| format!("Could not create repository-local Dicta storage: {error}"))?;

    let json = serde_json::to_string_pretty(metadata)
        .map_err(|error| format!("Could not serialize linked project: {error}"))?;
    fs::write(destination.join("project.json"), format!("{json}\n")).map_err(|error| {
        format!("Could not publish project metadata to the repository: {error}")
    })?;

    let legacy = project_dir(root, &metadata.id);
    let legacy_branches = legacy.join("branches");
    let destination_branches = destination.join("branches");
    copy_directory_missing(&legacy_branches, &destination_branches)?;
    rewrite_migrated_paths(&destination_branches, &legacy, &destination);
    exclude_dicta_from_git(source)?;
    Ok(destination)
}

fn git_output(source_path: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not run Git: {error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.is_empty() {
            "The selected folder is not a Git working copy".to_string()
        } else {
            message
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_root(source_path: &Path) -> Result<PathBuf, String> {
    let root = git_output(source_path, &["rev-parse", "--show-toplevel"])?;
    PathBuf::from(root)
        .canonicalize()
        .map_err(|error| format!("Could not resolve the Git working copy: {error}"))
}

fn git_branch(source_path: &Path) -> Result<String, String> {
    match git_output(source_path, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if !branch.is_empty() => Ok(branch),
        _ => {
            let revision = git_output(source_path, &["rev-parse", "--short", "HEAD"])?;
            Ok(format!("detached@{revision}"))
        }
    }
}

fn git_revision(source_path: &Path) -> Option<String> {
    git_output(source_path, &["rev-parse", "HEAD"])
        .ok()
        .filter(|revision| !revision.is_empty())
}

fn git_ref_exists(source_path: &Path, reference: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn default_git_branch(source_path: &Path) -> Option<String> {
    if let Ok(remote_head) = git_output(
        source_path,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        if let Some(branch) = remote_head.strip_prefix("origin/") {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|branch| git_ref_exists(source_path, &format!("refs/heads/{branch}")))
        .map(str::to_string)
}

fn revision_is_merged(source_path: &Path, revision: &str, default_branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(["merge-base", "--is-ancestor", revision, default_branch])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn branch_folder_name(branch: &str) -> String {
    let mut folder = String::new();
    for character in branch.chars() {
        match character {
            '/' => folder.push_str("__"),
            character
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') =>
            {
                folder.push(character)
            }
            _ => folder.push('-'),
        }
    }
    let folder = folder.trim_matches(['.', '-']).to_string();
    if folder.is_empty() {
        "unknown".to_string()
    } else {
        folder
    }
}

fn linked_branch_dir(root: &Path, metadata: &ProjectFile) -> Result<(String, PathBuf), String> {
    let source_path = metadata
        .source_path
        .as_ref()
        .ok_or_else(|| "This is a legacy unlinked project".to_string())?;
    let branch = git_branch(Path::new(source_path))?;
    let _ = root;
    let path = linked_storage_dir(metadata)
        .ok_or_else(|| "This is a legacy unlinked project".to_string())?
        .join("branches")
        .join(branch_folder_name(&branch));
    Ok((branch, path))
}

fn active_recording_root(
    root: &Path,
    metadata: &ProjectFile,
) -> Result<(Option<String>, PathBuf), String> {
    if metadata.source_path.is_some() {
        let (branch, path) = linked_branch_dir(root, metadata)?;
        Ok((Some(branch), path))
    } else {
        Ok((None, project_dir(root, &metadata.id)))
    }
}

fn write_branch_metadata(
    path: &Path,
    branch: &str,
    source_path: Option<&Path>,
) -> Result<(), String> {
    fs::create_dir_all(path.join("recordings"))
        .map_err(|error| format!("Could not create branch packet folder: {error}"))?;
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "git_branch": branch,
        "folder_name": branch_folder_name(branch),
        "head_oid": source_path.and_then(git_revision),
        "updated_at": Utc::now(),
    }))
    .map_err(|error| format!("Could not serialize branch metadata: {error}"))?;
    fs::write(path.join("branch.json"), format!("{json}\n"))
        .map_err(|error| format!("Could not save branch metadata: {error}"))
}

fn remove_video_files(directory: &Path) -> Result<(usize, u64), String> {
    let mut removed_files = 0;
    let mut freed_bytes = 0;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("Could not read {}: {error}", directory.display())),
    };
    for entry in entries.flatten() {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let (nested_files, nested_bytes) = remove_video_files(&entry.path())?;
            removed_files += nested_files;
            freed_bytes += nested_bytes;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("mp4")
        {
            let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "Could not remove merged recording {}: {error}",
                    entry.path().display()
                )
            })?;
            removed_files += 1;
            freed_bytes += size;
        }
    }
    Ok((removed_files, freed_bytes))
}

fn cleanup_merged_videos_for_project(
    root: &Path,
    metadata: &ProjectFile,
) -> Result<CleanupSummary, String> {
    let settings = read_settings(root);
    if !settings.cleanup_merged_videos {
        return Ok(CleanupSummary {
            message: "Merged-video cleanup is off.".to_string(),
            ..CleanupSummary::default()
        });
    }
    let source_path = metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "This project is not linked to Git".to_string())?;
    let default_branch = default_git_branch(source_path)
        .ok_or_else(|| "Could not determine the repository default branch".to_string())?;
    let active_branch = git_branch(source_path).ok();
    let branches_path = linked_storage_dir(metadata)
        .ok_or_else(|| "This project is not linked to Git".to_string())?
        .join("branches");
    let mut summary = CleanupSummary {
        default_branch: Some(default_branch.clone()),
        ..CleanupSummary::default()
    };
    let entries = match fs::read_dir(&branches_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            summary.message = "No branch recordings to clean.".to_string();
            return Ok(summary);
        }
        Err(error) => return Err(format!("Could not inspect branch recordings: {error}")),
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let branch_path = entry.path();
        let Ok(content) = fs::read_to_string(branch_path.join("branch.json")) else {
            continue;
        };
        let Ok(branch_metadata) = serde_json::from_str::<BranchMetadata>(&content) else {
            continue;
        };
        if branch_metadata.git_branch == default_branch
            || active_branch.as_deref() == Some(branch_metadata.git_branch.as_str())
        {
            continue;
        }
        let revision = branch_metadata.head_oid.or_else(|| {
            let reference = format!("refs/heads/{}", branch_metadata.git_branch);
            git_ref_exists(source_path, &reference).then_some(reference)
        });
        let Some(revision) = revision else {
            continue;
        };
        if !revision_is_merged(source_path, &revision, &default_branch) {
            continue;
        }
        let (removed_files, freed_bytes) = remove_video_files(&branch_path.join("recordings"))?;
        if removed_files > 0 {
            summary.removed_files += removed_files;
            summary.freed_bytes += freed_bytes;
            summary.cleaned_branches.push(branch_metadata.git_branch);
        }
    }
    summary.message = if summary.removed_files == 0 {
        format!("No merged videos found for {default_branch}.")
    } else {
        format!(
            "Removed {} merged video{}.",
            summary.removed_files,
            if summary.removed_files == 1 { "" } else { "s" }
        )
    };
    Ok(summary)
}

fn project_view(root: &Path, metadata: ProjectFile) -> Project {
    let storage_path = project_storage_dir(root, &metadata);
    let active_result = active_recording_root(root, &metadata);
    let git_error = active_result.as_ref().err().cloned();
    let active = active_result.ok();
    let git_branch = active.as_ref().and_then(|(branch, _)| branch.clone());
    let branch_path = active.as_ref().map(|(_, path)| path_string(path));
    let recording_count = active
        .as_ref()
        .map(|(_, path)| recording_files(path).len())
        .unwrap_or(0);
    let source_path = metadata.source_path.clone();
    Project {
        id: metadata.id,
        name: metadata.name,
        path: source_path
            .clone()
            .unwrap_or_else(|| path_string(&storage_path)),
        storage_path: path_string(&storage_path),
        source_path: source_path.clone(),
        git_branch,
        branch_path,
        is_git: source_path.is_some(),
        git_error,
        created_at: metadata.created_at,
        recording_count,
    }
}

fn read_project(root: &Path, project_id: &str) -> Result<ProjectFile, String> {
    let path = project_dir(root, project_id).join("project.json");
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_str(&content).map_err(|error| format!("Invalid project metadata: {error}"))
}

fn recording_files(project_path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let recordings_dir = project_path.join("recordings");
    let Ok(days) = fs::read_dir(recordings_dir) else {
        return files;
    };
    for day in days.flatten() {
        if !day.path().is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(day.path()) {
            files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
            }));
        }
    }
    files
}

fn load_recordings(root: &Path, project_id: &str) -> Result<Vec<Recording>, String> {
    let metadata = read_project(root, project_id)?;
    let (_, recording_root) = active_recording_root(root, &metadata)?;
    let mut recordings: Vec<Recording> = recording_files(&recording_root)
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|content| serde_json::from_str(&content).ok())
        .collect();
    recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(recordings)
}

fn load_projects(root: &Path) -> Vec<Project> {
    let mut projects = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return projects;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let metadata_path = entry.path().join("project.json");
        let Ok(content) = fs::read_to_string(metadata_path) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<ProjectFile>(&content) else {
            continue;
        };
        if metadata.source_path.is_some() {
            let _ = prepare_linked_storage(root, &metadata);
        }
        projects.push(project_view(root, metadata));
    }
    projects.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    projects
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let status = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?
        .status
        .clone();
    Ok(Bootstrap {
        root_path: path_string(&state.root),
        projects: load_projects(&state.root),
        status,
    })
}

#[tauri::command]
fn create_project(name: String, state: State<'_, AppState>) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".to_string());
    }
    let created_at = Utc::now();
    let id = format!("{}-{}", slugify(name), created_at.format("%y%m%d%H%M%S"));
    let path = project_dir(&state.root, &id);
    fs::create_dir_all(path.join("recordings"))
        .map_err(|error| format!("Could not create project: {error}"))?;
    let metadata = ProjectFile {
        id: id.clone(),
        name: name.to_string(),
        created_at,
        source_path: None,
    };
    let json = serde_json::to_string_pretty(&metadata)
        .map_err(|error| format!("Could not serialize project: {error}"))?;
    fs::write(path.join("project.json"), format!("{json}\n"))
        .map_err(|error| format!("Could not save project: {error}"))?;

    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    inner.status.active_project_id = Some(id.clone());
    Ok(project_view(&state.root, metadata))
}

#[tauri::command]
fn link_project(source_path: String, state: State<'_, AppState>) -> Result<Project, String> {
    let selected = PathBuf::from(source_path);
    if !selected.is_dir() {
        return Err("Choose an existing project folder".to_string());
    }
    let source = git_root(&selected)?;
    let source_string = path_string(&source);

    for existing in load_projects(&state.root) {
        if existing.source_path.as_deref() == Some(source_string.as_str()) {
            let metadata = read_project(&state.root, &existing.id)?;
            prepare_linked_storage(&state.root, &metadata)?;
            let mut inner = state
                .inner
                .lock()
                .map_err(|_| "Recorder state is unavailable".to_string())?;
            inner.status.active_project_id = Some(existing.id.clone());
            return Ok(project_view(&state.root, metadata));
        }
    }

    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Could not determine the project name from this folder".to_string())?
        .to_string();
    let mut id = slugify(&name);
    if project_dir(&state.root, &id).join("project.json").exists() {
        let mut hasher = DefaultHasher::new();
        source_string.hash(&mut hasher);
        id = format!("{}-{:06x}", id, hasher.finish() & 0x00ff_ffff);
    }
    let created_at = Utc::now();
    let metadata = ProjectFile {
        id: id.clone(),
        name,
        created_at,
        source_path: Some(source_string),
    };
    let project_path = project_dir(&state.root, &id);
    fs::create_dir_all(&project_path)
        .map_err(|error| format!("Could not create linked project: {error}"))?;
    let (branch, branch_path) = linked_branch_dir(&state.root, &metadata)?;
    write_branch_metadata(
        &branch_path,
        &branch,
        metadata.source_path.as_deref().map(Path::new),
    )?;
    let json = serde_json::to_string_pretty(&metadata)
        .map_err(|error| format!("Could not serialize linked project: {error}"))?;
    fs::write(project_path.join("project.json"), format!("{json}\n"))
        .map_err(|error| format!("Could not save linked project: {error}"))?;
    prepare_linked_storage(&state.root, &metadata)?;

    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    inner.status.active_project_id = Some(id);
    Ok(project_view(&state.root, metadata))
}

fn remove_project_registration(root: &Path, project_id: &str) -> Result<(), String> {
    if project_id.is_empty()
        || Path::new(project_id)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(project_id)
    {
        return Err("Invalid project identifier".to_string());
    }
    let metadata = read_project(root, project_id)?;
    if metadata.id != project_id {
        return Err("Project registration does not match the requested project".to_string());
    }
    let registration = project_dir(root, project_id).join("project.json");
    let archived = project_dir(root, project_id).join(format!(
        "project.removed-{}.json",
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    fs::rename(&registration, &archived).map_err(|error| {
        format!(
            "Could not remove project registration {}: {error}",
            registration.display()
        )
    })
}

#[tauri::command]
fn remove_project(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    {
        let inner = state
            .inner
            .lock()
            .map_err(|_| "Recorder state is unavailable".to_string())?;
        if matches!(
            inner.status.phase,
            RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
        ) {
            return Err("Stop the current recording before removing a project".to_string());
        }
    }
    remove_project_registration(&state.root, &project_id)?;
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if inner.status.active_project_id.as_deref() == Some(project_id.as_str()) {
        inner.status.active_project_id = None;
    }
    Ok(())
}

#[tauri::command]
fn refresh_project(project_id: String, state: State<'_, AppState>) -> Result<Project, String> {
    let metadata = read_project(&state.root, &project_id)?;
    Ok(project_view(&state.root, metadata))
}

#[tauri::command]
fn get_app_settings(state: State<'_, AppState>) -> AppSettings {
    read_settings(&state.root)
}

#[tauri::command]
fn set_shortcut(
    app: AppHandle,
    shortcut_id: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let next_shortcut =
        shortcut_for_id(&shortcut_id).ok_or_else(|| format!("Unknown shortcut: {shortcut_id}"))?;
    let previous_settings = read_settings(&state.root);
    let previous_shortcut = shortcut_for_id(&previous_settings.shortcut_id)
        .expect("stored shortcut is always validated");
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| format!("Could not release the current shortcut: {error}"))?;
    if let Err(error) = app.global_shortcut().register(next_shortcut) {
        let _ = app.global_shortcut().register(previous_shortcut);
        return Err(format!("Could not register that shortcut: {error}"));
    }
    let mut next_settings = previous_settings.clone();
    next_settings.shortcut_id = shortcut_id;
    if let Err(error) = write_settings(&state.root, &next_settings) {
        let _ = app.global_shortcut().unregister_all();
        let _ = app.global_shortcut().register(previous_shortcut);
        return Err(error);
    }
    Ok(next_settings)
}

#[tauri::command]
fn set_cleanup_merged_videos(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = read_settings(&state.root);
    settings.cleanup_merged_videos = enabled;
    write_settings(&state.root, &settings)?;
    Ok(settings)
}

#[tauri::command]
fn set_transcription_language(
    language: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    if !is_allowed_language(&language) {
        return Err(format!("Unsupported transcription language: {language}"));
    }
    let mut settings = read_settings(&state.root);
    settings.transcription_language = language;
    write_settings(&state.root, &settings)?;
    Ok(settings)
}

#[tauri::command]
fn cleanup_merged_videos(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<CleanupSummary, String> {
    let metadata = read_project(&state.root, &project_id)?;
    cleanup_merged_videos_for_project(&state.root, &metadata)
}

#[tauri::command]
fn select_project(project_id: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(id) = project_id.as_ref() {
        let _ = read_project(&state.root, id)?;
    }
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if matches!(
        inner.status.phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    ) {
        return Err("Cannot change projects while recording".to_string());
    }
    inner.status.active_project_id = project_id;
    Ok(())
}

#[tauri::command]
fn list_recordings(
    app: AppHandle,
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Recording>, String> {
    let mut recordings = load_recordings(&state.root, &project_id)?;
    let asset_scope = app.asset_protocol_scope();
    for recording in &mut recordings {
        if recording
            .poster_path
            .as_deref()
            .is_none_or(|path| !Path::new(path).is_file())
        {
            let poster = poster_path_for_video(&recording.video_path);
            if poster.is_file() {
                recording.poster_path = Some(path_string(&poster));
            }
        }
        if Path::new(&recording.video_path).is_file() {
            asset_scope
                .allow_file(&recording.video_path)
                .map_err(|error| format!("Could not grant video playback access: {error}"))?;
        }
        if let Some(poster_path) = recording.poster_path.as_deref() {
            if Path::new(poster_path).is_file() {
                asset_scope
                    .allow_file(poster_path)
                    .map_err(|error| format!("Could not grant poster access: {error}"))?;
            }
        }
    }
    Ok(recordings)
}

#[tauri::command]
fn ensure_recording_poster(
    app: AppHandle,
    project_id: String,
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let mut recording = load_recordings(&state.root, &project_id)?
        .into_iter()
        .find(|recording| recording.id == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    attach_poster(&mut recording);
    if let Some(poster_path) = recording.poster_path.as_deref() {
        app.asset_protocol_scope()
            .allow_file(poster_path)
            .map_err(|error| format!("Could not grant poster access: {error}"))?;
    }
    Ok(recording.poster_path)
}

fn recording_artifact_paths(metadata_path: &Path) -> Vec<PathBuf> {
    let Some(parent) = metadata_path.parent() else {
        return vec![metadata_path.to_path_buf()];
    };
    let Some(stem) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
        return vec![metadata_path.to_path_buf()];
    };
    [
        format!("{stem}.mp4"),
        format!("{stem}.poster.jpg"),
        format!("{stem}.transcript.md"),
        format!("{stem}.transcript.base.md"),
        format!("{stem}.transcript.json"),
        format!("{stem}.md"),
        format!("{stem}.json"),
    ]
    .into_iter()
    .map(|name| parent.join(name))
    .collect()
}

fn discard_recording_artifacts(recording: &Recording) {
    let metadata_path = Path::new(&recording.metadata_path);
    for artifact in recording_artifact_paths(metadata_path) {
        if artifact.is_file() {
            let _ = fs::remove_file(artifact);
        }
    }
    if let Some(day_dir) = metadata_path.parent() {
        let is_empty = fs::read_dir(day_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(day_dir);
        }
    }
}

#[tauri::command]
fn delete_recording(
    project_id: String,
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let project = read_project(&state.root, &project_id)?;
    let (_, recording_root) = active_recording_root(&state.root, &project)?;
    let recordings_root = recording_root.join("recordings");
    let metadata_path = recording_files(&recording_root)
        .into_iter()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<Recording>(&content).ok())
                .is_some_and(|recording| recording.id == recording_id)
        })
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;

    let canonical_recordings = recordings_root
        .canonicalize()
        .map_err(|error| format!("Could not resolve the recording folder: {error}"))?;
    let canonical_metadata = metadata_path
        .canonicalize()
        .map_err(|error| format!("Could not resolve the recording metadata: {error}"))?;
    if !canonical_metadata.starts_with(&canonical_recordings) {
        return Err("Refusing to delete a recording outside the active branch".to_string());
    }

    for artifact in recording_artifact_paths(&canonical_metadata) {
        if artifact.is_file() {
            fs::remove_file(&artifact)
                .map_err(|error| format!("Could not delete {}: {error}", artifact.display()))?;
        }
    }
    if let Some(day_dir) = canonical_metadata.parent() {
        let is_empty = fs::read_dir(day_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(day_dir);
        }
    }
    Ok(())
}

#[tauri::command]
fn save_timeline_notes(
    project_id: String,
    recording_id: String,
    timeline_notes: Vec<TimelineNote>,
    state: State<'_, AppState>,
) -> Result<Recording, String> {
    if timeline_notes.len() > 500 {
        return Err("A recording can contain at most 500 timeline notes".to_string());
    }
    for note in &timeline_notes {
        if note.id.trim().is_empty()
            || note.text.trim().is_empty()
            || note.text.chars().count() > 2_000
            || !note.timestamp_seconds.is_finite()
            || note.timestamp_seconds < 0.0
            || !matches!(note.source.as_str(), "typed" | "voice")
        {
            return Err("One or more timeline notes are invalid".to_string());
        }
    }

    let project = read_project(&state.root, &project_id)?;
    let (_, recording_root) = active_recording_root(&state.root, &project)?;
    let metadata_path = recording_files(&recording_root)
        .into_iter()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<Recording>(&content).ok())
                .is_some_and(|recording| recording.id == recording_id)
        })
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    let canonical_recordings = recording_root
        .join("recordings")
        .canonicalize()
        .map_err(|error| format!("Could not resolve the recording folder: {error}"))?;
    let canonical_metadata = metadata_path
        .canonicalize()
        .map_err(|error| format!("Could not resolve the recording metadata: {error}"))?;
    if !canonical_metadata.starts_with(&canonical_recordings) {
        return Err("Refusing to update a recording outside the active branch".to_string());
    }
    let content = fs::read_to_string(&canonical_metadata)
        .map_err(|error| format!("Could not read {}: {error}", canonical_metadata.display()))?;
    let mut recording: Recording = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid recording metadata: {error}"))?;
    if recording.duration_seconds.is_some_and(|duration| {
        timeline_notes
            .iter()
            .any(|note| note.timestamp_seconds > duration + 0.5)
    }) {
        return Err("A timeline note cannot be placed beyond the end of the recording".to_string());
    }
    recording.metadata_path = path_string(&canonical_metadata);
    recording.timeline_notes = timeline_notes;
    recording.timeline_notes.sort_by(|left, right| {
        left.timestamp_seconds
            .partial_cmp(&right.timestamp_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    write_recording(&recording)?;
    Ok(recording)
}

fn start_recording_inner(app: &AppHandle, note: String) -> Result<RecorderStatus, String> {
    let state = app.state::<AppState>();
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if matches!(
        inner.status.phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    ) {
        return Err("A recording is already in progress".to_string());
    }
    let project_id = inner
        .status
        .active_project_id
        .clone()
        .ok_or_else(|| "Select a project before recording".to_string())?;
    let project = read_project(&state.root, &project_id)?;
    let (git_branch, recording_root) = active_recording_root(&state.root, &project)?;
    if let Some(branch) = git_branch.as_deref() {
        write_branch_metadata(
            &recording_root,
            branch,
            project.source_path.as_deref().map(Path::new),
        )?;
    }

    let now_local = Local::now();
    let started_at = Utc::now();
    let day_dir = recording_root
        .join("recordings")
        .join(now_local.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&day_dir)
        .map_err(|error| format!("Could not create recording folder: {error}"))?;
    let stem = now_local.format("%H-%M-%S").to_string();
    let video_path = day_dir.join(format!("{stem}.mp4"));
    let metadata_path = day_dir.join(format!("{stem}.json"));
    let video_path_string = path_string(&video_path);
    let note = {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            inner.last_note.clone()
        } else {
            inner.last_note = trimmed.to_string();
            trimmed.to_string()
        }
    };

    inner.session = Some(Recording {
        id: format!("{}-{}", now_local.format("%Y%m%d"), stem),
        project_id: project_id.clone(),
        video_path: video_path_string.clone(),
        metadata_path: path_string(&metadata_path),
        note,
        git_branch,
        started_at,
        ended_at: None,
        duration_seconds: None,
        size_bytes: None,
        success: false,
        transcript: None,
        transcript_path: None,
        transcript_segments: Vec::new(),
        transcription_status: "pending".to_string(),
        transcription_error: None,
        transcription_language: None,
        poster_path: None,
        timeline_notes: Vec::new(),
    });
    inner.status = RecorderStatus {
        phase: RecordingPhase::Preparing,
        active_project_id: Some(project_id),
        active_video_path: Some(video_path_string.clone()),
        started_at: None,
        last_error: None,
    };
    let status = inner.status.clone();
    drop(inner);

    emit_recorder_event(
        app,
        "preparing",
        "Waiting for screen capture",
        status.clone(),
    );

    #[cfg(target_os = "macos")]
    {
        let path = CString::new(video_path_string)
            .map_err(|_| "The recording path contains an unsupported character".to_string())?;
        unsafe { dicta_start(path.as_ptr(), native_recorder_callback) };
    }
    #[cfg(not(target_os = "macos"))]
    {
        return Err("Dicta recording currently supports macOS only".to_string());
    }

    Ok(status)
}

#[tauri::command]
fn start_recording(app: AppHandle, note: String) -> Result<RecorderStatus, String> {
    start_recording_inner(&app, note)
}

fn stop_recording_inner(app: &AppHandle) -> Result<RecorderStatus, String> {
    let state = app.state::<AppState>();
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if !matches!(inner.status.phase, RecordingPhase::Recording) {
        return Err("No active recording to stop".to_string());
    }
    inner.status.phase = RecordingPhase::Stopping;
    let status = inner.status.clone();
    drop(inner);
    emit_recorder_event(app, "stopping", "Finalizing recording", status.clone());
    #[cfg(target_os = "macos")]
    unsafe {
        dicta_stop(native_recorder_callback);
    }
    Ok(status)
}

#[tauri::command]
fn stop_recording(app: AppHandle) -> Result<RecorderStatus, String> {
    stop_recording_inner(&app)
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("Path does not exist: {path}"));
    }
    let mut command = std::process::Command::new("open");
    if target.is_file() {
        command.arg("-R");
    }
    command
        .arg(target)
        .spawn()
        .map_err(|error| format!("Could not open Finder: {error}"))?;
    Ok(())
}

#[tauri::command]
fn build_context(project_id: String, state: State<'_, AppState>) -> Result<String, String> {
    let metadata = read_project(&state.root, &project_id)?;
    let project = project_view(&state.root, metadata);
    let recordings = load_recordings(&state.root, &project_id)?;
    let mut output = format!("# Dicta context: {}\n\n", project.name);
    if let Some(source_path) = project.source_path.as_deref() {
        output.push_str(&format!("Working copy: `{source_path}`\n"));
    }
    if let Some(branch) = project.git_branch.as_deref() {
        output.push_str(&format!("Git branch: `{branch}`\n"));
    }
    if let Some(branch_path) = project.branch_path.as_deref() {
        output.push_str(&format!("Branch packet folder: `{branch_path}`\n\n"));
    } else {
        output.push_str(&format!("Project folder: `{}`\n\n", project.storage_path));
    }
    if recordings.is_empty() {
        output.push_str("No recordings yet.\n");
        return Ok(output);
    }
    output.push_str("Review these screen-and-voice recordings as context for this task:\n\n");
    for recording in recordings.iter().take(50) {
        output.push_str(&format!(
            "- **{}** ({})\n  - Video: `{}`\n  - Metadata: `{}`\n{}",
            if recording.note.is_empty() {
                "Untitled context"
            } else {
                &recording.note
            },
            recording
                .started_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M"),
            recording.video_path,
            recording.metadata_path,
            recording
                .transcript_path
                .as_deref()
                .map(|path| format!("  - Transcript: `{path}`\n"))
                .unwrap_or_else(|| "  - Transcript: processing\n".to_string())
        ));
        for note in &recording.timeline_notes {
            let total_seconds = note.timestamp_seconds.max(0.0).floor() as u64;
            output.push_str(&format!(
                "  - Note at {:02}:{:02}: {}\n",
                total_seconds / 60,
                total_seconds % 60,
                note.text.replace(['\r', '\n'], " ")
            ));
        }
    }
    output.push_str("\nUse the transcript as primary guidance and the original video when visual evidence is necessary. Ask if any referenced detail is ambiguous.\n");
    Ok(output)
}

#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not access the clipboard: {error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Could not open the clipboard".to_string())?
        .write_all(text.as_bytes())
        .map_err(|error| format!("Could not write to the clipboard: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("Clipboard command failed: {error}"))?;
    if !status.success() {
        return Err("Clipboard command did not complete successfully".to_string());
    }
    Ok(())
}

fn emit_recorder_event(app: &AppHandle, event: &str, message: &str, status: RecorderStatus) {
    sync_tray(app, &status.phase);
    let _ = app.emit(
        "recorder-event",
        RecorderEventPayload {
            event: event.to_string(),
            message: message.to_string(),
            status,
        },
    );
}

fn sync_tray(app: &AppHandle, phase: &RecordingPhase) {
    let recording = matches!(
        phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    );
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(if recording {
            "Dicta — Recording"
        } else {
            "Dicta"
        }));
        let _ = tray.set_title(Some(if recording { "●" } else { "" }));
    }
}

fn schedule_recording_limit(app: &AppHandle, started_at: DateTime<Utc>) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(MAX_RECORDING_SECONDS));
        let should_stop = {
            let state = app.state::<AppState>();
            let inner = state
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            matches!(inner.status.phase, RecordingPhase::Recording)
                && inner.status.started_at == Some(started_at)
        };
        if should_stop {
            if let Ok(status) = stop_recording_inner(&app) {
                emit_recorder_event(
                    &app,
                    "stopping",
                    "Reached the 20-minute recording limit",
                    status,
                );
            }
        }
    });
}

fn write_recording(recording: &Recording) -> Result<(), String> {
    let json = serde_json::to_string_pretty(recording)
        .map_err(|error| format!("Could not serialize recording metadata: {error}"))?;
    fs::write(&recording.metadata_path, format!("{json}\n"))
        .map_err(|error| format!("Could not write recording metadata: {error}"))
}

fn clean_segment_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_transcript_segments(segments: &[TranscriptSegment]) -> Vec<TranscriptSegment> {
    let mut normalized = segments
        .iter()
        .filter_map(|segment| {
            let text = clean_segment_text(&segment.text);
            if text.is_empty()
                || !segment.start_seconds.is_finite()
                || !segment.end_seconds.is_finite()
                || segment.start_seconds < 0.0
            {
                return None;
            }
            Some(TranscriptSegment {
                start_seconds: segment.start_seconds,
                end_seconds: segment.end_seconds.max(segment.start_seconds),
                text,
            })
        })
        .take(10_000)
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        left.start_seconds
            .partial_cmp(&right.start_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut grouped: Vec<TranscriptSegment> = Vec::new();
    for segment in normalized {
        let should_join = grouped.last().is_some_and(|current| {
            let gap = segment.start_seconds - current.end_seconds;
            let current_duration = current.end_seconds - current.start_seconds;
            gap <= 1.0 && current_duration < 6.0 && !current.text.ends_with(['.', '?', '!'])
        });
        if should_join {
            let current = grouped.last_mut().expect("checked above");
            current.text.push(' ');
            current.text.push_str(&segment.text);
            current.end_seconds = current.end_seconds.max(segment.end_seconds);
        } else {
            grouped.push(segment);
        }
    }
    grouped
}

fn transcript_timestamp(seconds: f64) -> String {
    let total_tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let hours = total_tenths / 36_000;
    let minutes = (total_tenths / 600) % 60;
    let seconds = (total_tenths / 10) % 60;
    let tenths = total_tenths % 10;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{tenths}")
    } else {
        format!("{minutes:02}:{seconds:02}.{tenths}")
    }
}

fn timestamped_transcript(transcript: &str, segments: &[TranscriptSegment]) -> String {
    if segments.is_empty() {
        return transcript.trim().to_string();
    }
    segments
        .iter()
        .map(|segment| {
            format!(
                "[{}–{}] {}",
                transcript_timestamp(segment.start_seconds),
                transcript_timestamp(segment.end_seconds),
                segment.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn update_transcription(payload: &NativeTranscriptionPayload) -> Result<Recording, String> {
    let metadata_path = PathBuf::from(&payload.path).with_extension("json");
    let content = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("Could not read {}: {error}", metadata_path.display()))?;
    let mut recording: Recording = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid recording metadata: {error}"))?;
    if let Some(transcript) = payload.transcript.as_deref() {
        let transcript_segments = normalize_transcript_segments(&payload.transcript_segments);
        let transcript_path = metadata_path.with_extension("transcript.md");
        fs::write(
            &transcript_path,
            format!(
                "{}\n",
                timestamped_transcript(transcript, &transcript_segments)
            ),
        )
        .map_err(|error| format!("Could not write transcript: {error}"))?;
        let transcript_json_path = metadata_path.with_extension("transcript.json");
        let transcript_json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "transcript": transcript,
            "transcript_segments": &transcript_segments,
        }))
        .map_err(|error| format!("Could not encode timed transcript: {error}"))?;
        fs::write(&transcript_json_path, format!("{transcript_json}\n"))
            .map_err(|error| format!("Could not write timed transcript: {error}"))?;
        recording.transcript = Some(transcript.to_string());
        recording.transcript_path = Some(path_string(&transcript_path));
        recording.transcript_segments = transcript_segments;
        recording.transcription_status = "complete".to_string();
        recording.transcription_error = None;
    } else {
        recording.transcription_status = "failed".to_string();
        recording.transcription_error = payload.error.clone();
    }
    write_recording(&recording)?;
    Ok(recording)
}

fn mark_transcription_processing(video_path: &str, language: &str) -> Result<(), String> {
    let metadata_path = PathBuf::from(video_path).with_extension("json");
    let content = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("Could not read {}: {error}", metadata_path.display()))?;
    let mut recording: Recording = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid recording metadata: {error}"))?;
    recording.transcription_status = "processing".to_string();
    recording.transcription_error = None;
    recording.transcription_language = Some(language.to_string());
    write_recording(&recording)
}

fn whisper_prompt(language: &str) -> &'static str {
    if language == "nl" {
        "Nederlandse technische uitleg over softwareontwikkeling, API-integraties, broncode en implementatiedetails."
    } else {
        "Technical software explanation about APIs, source code, and implementation details."
    }
}

fn loaded_whisper(
    app: &AppHandle,
) -> Result<std::sync::MutexGuard<'static, Option<LoadedWhisper>>, String> {
    let model_path = selected_whisper_model(app)?;
    let mut slot = WHISPER_MODEL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let needs_load = slot
        .as_ref()
        .map(|loaded| loaded.path != model_path)
        .unwrap_or(true);
    if needs_load {
        let context =
            WhisperContext::new_with_params(&model_path, WhisperContextParameters::default())
                .map_err(|error| format!("Could not load Dicta's Whisper model: {error}"))?;
        *slot = Some(LoadedWhisper {
            path: model_path,
            context,
        });
    }
    Ok(slot)
}

fn local_whisper_transcript(
    app: &AppHandle,
    video_path: &str,
    language: &str,
) -> Result<LocalTranscript, String> {
    let mut hasher = DefaultHasher::new();
    video_path.hash(&mut hasher);
    let wav_path = std::env::temp_dir().join(format!("dicta-{}.wav", hasher.finish()));
    let input = CString::new(video_path)
        .map_err(|_| "The recording path contains an unsupported character".to_string())?;
    let output = CString::new(path_string(&wav_path))
        .map_err(|_| "The temporary audio path contains an unsupported character".to_string())?;
    let extracted = unsafe { dicta_extract_audio(input.as_ptr(), output.as_ptr()) };
    if !extracted {
        return Err("Dicta could not extract narration from the recording".to_string());
    }

    let result = (|| {
        let mut reader = hound::WavReader::open(&wav_path)
            .map_err(|error| format!("Could not read extracted narration: {error}"))?;
        let spec = reader.spec();
        if spec.channels != 1 || spec.sample_rate != 16_000 {
            return Err("Extracted narration was not 16 kHz mono audio".to_string());
        }
        let samples = reader
            .samples::<i16>()
            .map(|sample| {
                sample
                    .map(|value| value as f32 / i16::MAX as f32)
                    .map_err(|error| format!("Invalid narration sample: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if samples.is_empty() {
            return Err("No narration audio was found in this recording".to_string());
        }

        let loaded = loaded_whisper(app)?;
        let context = loaded
            .as_ref()
            .ok_or_else(|| "Dicta's Whisper model failed to load".to_string())?;
        let mut state = context
            .context
            .create_state()
            .map_err(|error| format!("Could not start local transcription: {error}"))?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        params.set_language(if language == "auto" {
            None
        } else {
            Some(language)
        });
        params.set_initial_prompt(whisper_prompt(language));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state
            .full(params, &samples)
            .map_err(|error| format!("Local transcription failed: {error}"))?;
        let segments = state
            .as_iter()
            .filter_map(|segment| {
                let text = clean_segment_text(&segment.to_string());
                if text.is_empty() {
                    return None;
                }
                Some(TranscriptSegment {
                    // whisper.cpp exposes segment timestamps in centiseconds.
                    start_seconds: segment.start_timestamp() as f64 / 100.0,
                    end_seconds: segment.end_timestamp() as f64 / 100.0,
                    text,
                })
            })
            .collect::<Vec<_>>();
        let segments = normalize_transcript_segments(&segments);
        let transcript = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if transcript.is_empty() {
            Err("No speech was detected in this recording".to_string())
        } else {
            Ok(LocalTranscript {
                transcript,
                segments,
            })
        }
    })();
    let _ = fs::remove_file(wav_path);
    result
}

#[tauri::command]
async fn transcribe_voice_note(
    app: AppHandle,
    audio_bytes: Vec<u8>,
    mime_type: String,
    language: String,
) -> Result<String, String> {
    if !is_allowed_language(&language) {
        return Err(format!("Unsupported transcription language: {language}"));
    }
    if audio_bytes.len() < 128 {
        return Err("The voice note did not contain enough audio".to_string());
    }
    if audio_bytes.len() > 16 * 1024 * 1024 {
        return Err("Voice notes must be shorter than 16 MB".to_string());
    }
    let normalized_mime = mime_type.split(';').next().unwrap_or("");
    let extension = match normalized_mime {
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => return Err("Dicta does not support this microphone audio format".to_string()),
    };
    let mut hasher = DefaultHasher::new();
    audio_bytes.hash(&mut hasher);
    let audio_path = std::env::temp_dir().join(format!(
        "dicta-voice-{}-{}.{}",
        Utc::now().timestamp_millis(),
        hasher.finish(),
        extension
    ));
    fs::write(&audio_path, &audio_bytes)
        .map_err(|error| format!("Could not prepare the voice note: {error}"))?;
    let audio_path_string = path_string(&audio_path);
    let app_for_transcription = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let lock = LOCAL_TRANSCRIBER.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        local_whisper_transcript(&app_for_transcription, &audio_path_string, &language)
            .map(|result| result.transcript)
    })
    .await;
    let _ = fs::remove_file(audio_path);
    joined.map_err(|error| format!("Voice transcription stopped unexpectedly: {error}"))?
}

fn queue_local_transcription(app: &AppHandle, video_path: String, language: String) {
    let _ = mark_transcription_processing(&video_path, &language);
    let app = app.clone();
    std::thread::spawn(move || {
        let lock = LOCAL_TRANSCRIBER.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = local_whisper_transcript(&app, &video_path, &language);
        let payload = match result {
            Ok(result) => NativeTranscriptionPayload {
                path: video_path,
                transcript: Some(result.transcript),
                transcript_segments: result.segments,
                error: None,
            },
            Err(error) => NativeTranscriptionPayload {
                path: video_path,
                transcript: None,
                transcript_segments: Vec::new(),
                error: Some(error),
            },
        };
        let updated = update_transcription(&payload);
        let state = app.state::<AppState>();
        let status = state
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            .clone();
        match updated {
            Ok(recording) if recording.transcription_status == "complete" => {
                emit_recorder_event(&app, "transcribed", "Transcript ready for agents", status)
            }
            Ok(recording) => emit_recorder_event(
                &app,
                "transcription_error",
                recording
                    .transcription_error
                    .as_deref()
                    .unwrap_or("Local transcription failed"),
                status,
            ),
            Err(error) => emit_recorder_event(&app, "transcription_error", &error, status),
        }
    });
}

#[tauri::command]
fn retranscribe_recording(
    app: AppHandle,
    project_id: String,
    recording_id: String,
    language: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !is_allowed_language(&language) {
        return Err(format!("Unsupported transcription language: {language}"));
    }
    let recording = load_recordings(&state.root, &project_id)?
        .into_iter()
        .find(|recording| recording.id == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    if !recording.success || !Path::new(&recording.video_path).exists() {
        return Err("This recording has no usable video to transcribe".to_string());
    }
    mark_transcription_processing(&recording.video_path, &language)?;
    emit_recorder_event(
        &app,
        "transcribing",
        "Transcribing with the selected language…",
        state
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            .clone(),
    );
    queue_local_transcription(&app, recording.video_path, language);
    Ok(())
}

fn queue_transcription(video_path: &str, language: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = CString::new(video_path)
            .map_err(|_| "The transcription path contains an unsupported character".to_string())?;
        let spoken = CString::new(language)
            .unwrap_or_else(|_| CString::new(DEFAULT_LANGUAGE).expect("default language is ascii"));
        unsafe { dicta_transcribe(path.as_ptr(), spoken.as_ptr(), native_recorder_callback) };
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = video_path;
        let _ = language;
        Err("Automatic transcription currently supports macOS only".to_string())
    }
}

fn should_retry_transcription(recording: &Recording) -> bool {
    if !recording.success || !Path::new(&recording.video_path).exists() {
        return false;
    }
    if !recording
        .transcript
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return false;
    }
    matches!(
        recording.transcription_status.as_str(),
        "pending" | "processing" | ""
    )
}

fn language_for_recording(root: &Path, recording: &Recording) -> String {
    recording
        .transcription_language
        .as_deref()
        .filter(|language| is_allowed_language(language))
        .map(str::to_string)
        .unwrap_or_else(|| settings_language(root))
}

fn queue_pending_transcriptions(root: &Path) {
    for project in load_projects(root) {
        let Ok(recordings) = load_recordings(root, &project.id) else {
            continue;
        };
        for recording in recordings {
            if should_retry_transcription(&recording) {
                let language = language_for_recording(root, &recording);
                let _ = queue_transcription(&recording.video_path, &language);
            }
        }
    }
}

fn poster_path_for_video(video_path: &str) -> PathBuf {
    PathBuf::from(video_path).with_extension("poster.jpg")
}

fn extract_poster(video_path: &str) -> Option<String> {
    let poster = poster_path_for_video(video_path);
    if poster.is_file() {
        return Some(path_string(&poster));
    }
    if !Path::new(video_path).is_file() {
        return None;
    }
    let input = CString::new(video_path).ok()?;
    let output = CString::new(path_string(&poster)).ok()?;
    #[cfg(target_os = "macos")]
    {
        if unsafe { dicta_extract_poster(input.as_ptr(), output.as_ptr()) } && poster.is_file() {
            return Some(path_string(&poster));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, output);
    }
    None
}

fn attach_poster(recording: &mut Recording) {
    if recording
        .poster_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file())
    {
        return;
    }
    if let Some(poster) = extract_poster(&recording.video_path) {
        recording.poster_path = Some(poster);
        let _ = write_recording(recording);
    }
}

fn finalize_session(app: &AppHandle, error: Option<String>) -> (RecorderStatus, Option<String>) {
    let state = app.state::<AppState>();
    let mut inner = state
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut video_path = None;
    let mut poster_source = None;
    let language = settings_language(&state.root);
    let capture_started = matches!(
        inner.status.phase,
        RecordingPhase::Recording | RecordingPhase::Stopping
    );
    if let Some(mut recording) = inner.session.take() {
        if error.is_some() && !capture_started {
            discard_recording_artifacts(&recording);
        } else {
            let ended_at = Utc::now();
            recording.ended_at = Some(ended_at);
            recording.duration_seconds = Some(
                ended_at
                    .signed_duration_since(recording.started_at)
                    .num_milliseconds() as f64
                    / 1000.0,
            );
            recording.size_bytes = fs::metadata(&recording.video_path)
                .ok()
                .map(|metadata| metadata.len());
            recording.success = error.is_none();
            if recording.success {
                recording.transcription_status = "processing".to_string();
                recording.transcription_error = None;
                recording.transcription_language = Some(language);
                poster_source = Some(recording.video_path.clone());
                video_path = Some(recording.video_path.clone());
            } else {
                recording.transcription_status = "failed".to_string();
                recording.transcription_error = error.clone();
            }
            let _ = write_recording(&recording);
        }
    }
    inner.status.phase = if error.is_some() {
        RecordingPhase::Error
    } else {
        RecordingPhase::Idle
    };
    inner.status.active_video_path = None;
    inner.status.started_at = None;
    inner.status.last_error = error;
    let status = inner.status.clone();
    drop(inner);
    if let Some(video) = poster_source.as_deref() {
        if let Some(poster) = extract_poster(video) {
            let metadata_path = PathBuf::from(video).with_extension("json");
            if let Ok(content) = fs::read_to_string(&metadata_path) {
                if let Ok(mut recording) = serde_json::from_str::<Recording>(&content) {
                    recording.poster_path = Some(poster);
                    let _ = write_recording(&recording);
                }
            }
        }
    }
    (status, video_path)
}

extern "C" fn native_recorder_callback(event: *const c_char, message: *const c_char) {
    if event.is_null() || message.is_null() {
        return;
    }
    let event = unsafe { CStr::from_ptr(event) }
        .to_string_lossy()
        .into_owned();
    let message = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    let state = app.state::<AppState>();
    match event.as_str() {
        "started" => {
            let status = {
                let mut inner = state
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let started_at = Utc::now();
                if let Some(recording) = inner.session.as_mut() {
                    recording.started_at = started_at;
                }
                inner.status.phase = RecordingPhase::Recording;
                inner.status.started_at = Some(started_at);
                inner.status.last_error = None;
                inner.status.clone()
            };
            emit_recorder_event(app, "started", &message, status.clone());
            if let Some(started_at) = status.started_at {
                schedule_recording_limit(app, started_at);
            }
        }
        "finished" => {
            let (status, video_path) = finalize_session(app, None);
            emit_recorder_event(
                app,
                "finished",
                "Recording saved. Transcribing narration…",
                status.clone(),
            );
            if let Some(video_path) = video_path {
                let language = settings_language(&app.state::<AppState>().root);
                if let Err(error) = queue_transcription(&video_path, &language) {
                    let payload = NativeTranscriptionPayload {
                        path: video_path,
                        transcript: None,
                        transcript_segments: Vec::new(),
                        error: Some(error.clone()),
                    };
                    let _ = update_transcription(&payload);
                    emit_recorder_event(app, "transcription_error", &error, status);
                }
            }
        }
        "error" => {
            let (status, _) = finalize_session(app, Some(message.clone()));
            emit_recorder_event(app, "error", &message, status);
        }
        "transcribing" => {
            let status = state
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .status
                .clone();
            emit_recorder_event(app, "transcribing", "Transcribing narration…", status);
        }
        "transcript" | "transcription_error" => {
            let status = state
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .status
                .clone();
            let parsed = serde_json::from_str::<NativeTranscriptionPayload>(&message)
                .map_err(|error| format!("Invalid transcription response: {error}"));
            if event == "transcription_error" {
                match parsed {
                    Ok(payload) => {
                        emit_recorder_event(
                            app,
                            "transcribing",
                            "Using Dicta's local Whisper fallback…",
                            status,
                        );
                        let language = settings_language(&app.state::<AppState>().root);
                        queue_local_transcription(app, payload.path, language);
                    }
                    Err(error) => {
                        emit_recorder_event(app, "transcription_error", &error, status);
                    }
                }
                return;
            }
            match parsed.and_then(|payload| update_transcription(&payload)) {
                Ok(recording) => {
                    emit_recorder_event(
                        app,
                        "transcribed",
                        if recording.transcript.is_some() {
                            "Transcript ready for agents"
                        } else {
                            "Transcription finished"
                        },
                        status,
                    );
                }
                Err(error) => {
                    emit_recorder_event(app, "transcription_error", &error, status);
                }
            }
        }
        _ => {}
    }
}

fn toggle_from_shortcut(app: &AppHandle) {
    let phase = {
        let state = app.state::<AppState>();
        let phase = state
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            .phase
            .clone();
        phase
    };
    let result = match phase {
        RecordingPhase::Recording => stop_recording_inner(app).map(|_| ()),
        RecordingPhase::Idle | RecordingPhase::Error => {
            start_recording_inner(app, String::new()).map(|_| ())
        }
        _ => Ok(()),
    };
    if let Err(error) = result {
        let state = app.state::<AppState>();
        let status = {
            let mut inner = state
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.status.last_error = Some(error.clone());
            inner.status.clone()
        };
        emit_recorder_event(app, "error", &error, status);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let documents = dirs::document_dir().unwrap_or_else(|| PathBuf::from("."));
    let legacy_root = documents.join("PromptReel");
    let preferred_root = documents.join("Dicta");
    let root = if preferred_root.exists() {
        preferred_root
    } else if legacy_root.exists() {
        match fs::rename(&legacy_root, &preferred_root) {
            Ok(()) => preferred_root,
            Err(_) => legacy_root,
        }
    } else {
        preferred_root
    };
    fs::create_dir_all(&root).expect("failed to create Dicta folder");

    let settings = read_settings(&root);
    let shortcut = shortcut_for_id(&settings.shortcut_id)
        .unwrap_or_else(|| shortcut_for_id(DEFAULT_SHORTCUT_ID).expect("default shortcut exists"));

    tauri::Builder::default()
        .manage(AppState::new(root))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _triggered, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_from_shortcut(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            mcp_status,
            configure_codex_mcp,
            restart_codex_mcp,
            model_status,
            download_quality_model,
            create_project,
            link_project,
            remove_project,
            refresh_project,
            get_app_settings,
            set_shortcut,
            set_cleanup_merged_videos,
            set_transcription_language,
            cleanup_merged_videos,
            ensure_recording_poster,
            select_project,
            list_recordings,
            delete_recording,
            save_timeline_notes,
            transcribe_voice_note,
            retranscribe_recording,
            start_recording,
            stop_recording,
            reveal_path,
            build_context,
            copy_to_clipboard
        ])
        .setup(move |app| {
            let _ = APP_HANDLE.set(app.handle().clone());
            let _ = install_mcp_binary(app.handle());
            let root = app.state::<AppState>().root.clone();
            queue_pending_transcriptions(&root);
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            app.global_shortcut().register(shortcut.clone())?;

            let show = MenuItem::with_id(app, "show", "Show Dicta", true, None::<&str>)?;
            let record =
                MenuItem::with_id(app, "record", "Start / Stop Recording", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Dicta", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &record, &quit])?;
            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Dicta")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "record" => toggle_from_shortcut(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                });
            if let Ok(icon) =
                tauri::image::Image::from_bytes(include_bytes!("../assets/dicta-tray@2x.png"))
            {
                tray = tray.icon(icon).icon_as_template(true);
            } else if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Dicta");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_safe_project_slugs() {
        assert_eq!(
            slugify(" API Integration / Tickets "),
            "api-integration-tickets"
        );
        assert_eq!(slugify("✨"), "project");
    }

    #[test]
    fn removing_a_project_preserves_its_files_and_archives_registration() {
        let root = std::env::temp_dir().join(format!(
            "dicta-remove-project-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let project_path = root.join("demo");
        let recording_path = project_path.join("recordings/2026-08-14/demo.mp4");
        fs::create_dir_all(recording_path.parent().unwrap()).unwrap();
        fs::write(&recording_path, "video").unwrap();
        let metadata = ProjectFile {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            created_at: Utc::now(),
            source_path: None,
        };
        fs::write(
            project_path.join("project.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        remove_project_registration(&root, "demo").unwrap();

        assert!(!project_path.join("project.json").exists());
        assert!(recording_path.is_file());
        assert!(fs::read_dir(&project_path)
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("project.removed-")));
        assert!(load_projects(&root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mcp_binary_install_replaces_the_file_atomically() {
        let root = std::env::temp_dir().join(format!(
            "dicta-mcp-install-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let resource = root.join("resource/dicta-mcp");
        let target = root.join("installed/dicta-mcp");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&resource, "new executable").unwrap();
        fs::write(&target, "running executable").unwrap();
        let open_old_file = fs::File::open(&target).unwrap();

        atomic_install_binary(&resource, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new executable");
        let mut old_contents = String::new();
        use std::io::Read;
        (&open_old_file).read_to_string(&mut old_contents).unwrap();
        assert_eq!(old_contents, "running executable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forced_mcp_registration_removes_then_adds_the_server() {
        let root = std::env::temp_dir().join(format!(
            "dicta-mcp-restart-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let fake_codex = root.join("codex");
        let executable = root.join("dicta-mcp");
        let log = root.join("calls.log");
        fs::write(&executable, "server").unwrap();
        fs::write(
            &fake_codex,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                path_string(&log)
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();

        register_codex_mcp(&fake_codex, &executable, true).unwrap();

        let calls = fs::read_to_string(log).unwrap();
        let lines = calls.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "mcp remove dicta");
        assert_eq!(
            lines[1],
            format!("mcp add dicta -- {}", path_string(&executable))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_file_scan_ignores_non_metadata_files() {
        let root = std::env::temp_dir().join(format!(
            "dicta-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let day = root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("test.json"), "{}").unwrap();
        fs::write(day.join("test.mp4"), "video").unwrap();
        assert_eq!(recording_files(&root), vec![day.join("test.json")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_artifacts_stay_beside_the_metadata() {
        let metadata = PathBuf::from("/tmp/dicta/14-25-09.json");
        let artifacts = recording_artifact_paths(&metadata);
        assert_eq!(artifacts.len(), 7);
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.mp4")));
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.poster.jpg")));
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.transcript.base.md")));
        assert!(artifacts
            .iter()
            .all(|path| path.parent() == metadata.parent()));
    }

    #[test]
    fn permission_denied_start_discards_pending_recording_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "dicta-permission-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let day = root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        let video_path = day.join("20-48-00.mp4");
        let metadata_path = day.join("20-48-00.json");
        fs::write(&video_path, []).unwrap();
        fs::write(&metadata_path, "pending").unwrap();
        let recording = Recording {
            id: "20260813-20-48-00".to_string(),
            project_id: "peepel".to_string(),
            video_path: path_string(&video_path),
            metadata_path: path_string(&metadata_path),
            note: String::new(),
            git_branch: Some("securex-quota".to_string()),
            started_at: Utc::now(),
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: false,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: "pending".to_string(),
            transcription_error: None,
            transcription_language: None,
            poster_path: None,
            timeline_notes: Vec::new(),
        };

        discard_recording_artifacts(&recording);

        assert!(!video_path.exists());
        assert!(!metadata_path.exists());
        assert!(!day.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_safe_branch_folder_names() {
        assert_eq!(branch_folder_name("main"), "main");
        assert_eq!(
            branch_folder_name("feature/oauth-flow"),
            "feature__oauth-flow"
        );
        assert_eq!(branch_folder_name("detached@a1b2c3d"), "detached-a1b2c3d");
    }

    #[test]
    fn settings_round_trip_shortcut_and_cleanup_preferences() {
        let root = std::env::temp_dir().join(format!(
            "dicta-settings-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(read_settings(&root).shortcut_id, DEFAULT_SHORTCUT_ID);
        assert!(read_settings(&root).cleanup_merged_videos);
        assert_eq!(
            read_settings(&root).transcription_language,
            DEFAULT_LANGUAGE
        );

        let settings = AppSettings {
            shortcut_id: "option_space".to_string(),
            cleanup_merged_videos: false,
            transcription_language: "en".to_string(),
        };
        write_settings(&root, &settings).unwrap();
        let restored = read_settings(&root);
        assert_eq!(restored.shortcut_id, "option_space");
        assert!(!restored.cleanup_merged_videos);
        assert_eq!(restored.transcription_language, "en");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merged_branch_cleanup_only_removes_videos() {
        let root = std::env::temp_dir().join(format!(
            "dicta-cleanup-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("repository");
        let library = root.join("library");
        fs::create_dir_all(&library).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        for arguments in [
            ["config", "user.email", "dicta@example.com"],
            ["config", "user.name", "Dicta Tests"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(arguments)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repository.join("README.md"), "main").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["commit", "-m", "main"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "-b", "feature/context"])
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("feature.txt"), "feature").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["add", "feature.txt"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["commit", "-m", "feature"])
            .status()
            .unwrap()
            .success());

        let branch_root = repository.join(".dicta/branches/feature__context");
        write_branch_metadata(&branch_root, "feature/context", Some(&repository)).unwrap();
        let day = branch_root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("packet.mp4"), vec![7_u8; 128]).unwrap();
        fs::write(day.join("packet.transcript.md"), "keep me").unwrap();
        fs::write(day.join("packet.json"), "{}").unwrap();

        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "main"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["merge", "--no-ff", "feature/context", "-m", "merge feature"])
            .status()
            .unwrap()
            .success());

        let project = ProjectFile {
            id: "repository".to_string(),
            name: "Repository".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };
        let summary = cleanup_merged_videos_for_project(&library, &project).unwrap();
        assert_eq!(summary.removed_files, 1);
        assert_eq!(summary.freed_bytes, 128);
        assert_eq!(summary.cleaned_branches, vec!["feature/context"]);
        assert!(!day.join("packet.mp4").exists());
        assert!(day.join("packet.transcript.md").is_file());
        assert!(day.join("packet.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_root_follows_the_checked_out_branch() {
        let root = std::env::temp_dir().join(format!(
            "dicta-git-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("peepel");
        let storage = root.join("storage");
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        let project = ProjectFile {
            id: "peepel".to_string(),
            name: "peepel".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };

        let (branch, path) = active_recording_root(&storage, &project).unwrap();
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(path, repository.join(".dicta/branches/main"));

        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "-b", "feature/oauth"])
            .status()
            .unwrap()
            .success());
        let (branch, path) = active_recording_root(&storage, &project).unwrap();
        assert_eq!(branch.as_deref(), Some("feature/oauth"));
        assert_eq!(path, repository.join(".dicta/branches/feature__oauth"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_storage_migrates_packets_and_rewrites_paths() {
        let root = std::env::temp_dir().join(format!(
            "dicta-migration-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("securex");
        let library = root.join("library");
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        let metadata = ProjectFile {
            id: "securex".to_string(),
            name: "securex".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };
        let legacy = library.join("securex");
        let old_day = legacy.join("branches/main/recordings/2026-08-13");
        fs::create_dir_all(&old_day).unwrap();
        let old_video = old_day.join("securex-quotas.mp4");
        let old_metadata = old_day.join("securex-quotas.json");
        fs::write(&old_video, "video").unwrap();
        fs::write(
            &old_metadata,
            serde_json::json!({
                "id": "securex-quotas",
                "video_path": path_string(&old_video),
                "metadata_path": path_string(&old_metadata)
            })
            .to_string(),
        )
        .unwrap();

        let destination = prepare_linked_storage(&library, &metadata).unwrap();
        let migrated_metadata =
            destination.join("branches/main/recordings/2026-08-13/securex-quotas.json");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(migrated_metadata).unwrap()).unwrap();
        assert_eq!(
            value["video_path"].as_str().unwrap(),
            path_string(
                &destination.join("branches/main/recordings/2026-08-13/securex-quotas.mp4")
            )
        );
        assert!(destination.join("project.json").is_file());
        let raw_exclude = PathBuf::from(
            git_output(&repository, &["rev-parse", "--git-path", "info/exclude"]).unwrap(),
        );
        let exclude_path = if raw_exclude.is_absolute() {
            raw_exclude
        } else {
            repository.join(raw_exclude)
        };
        assert!(fs::read_to_string(exclude_path)
            .unwrap()
            .contains(".dicta/"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn whisper_prompts_stay_generic() {
        assert!(!whisper_prompt("nl").to_lowercase().contains("securex"));
        assert!(!whisper_prompt("en").to_lowercase().contains("securex"));
        assert!(whisper_prompt("nl").contains("broncode"));
    }

    #[test]
    fn transcript_segments_are_grouped_and_timestamped() {
        let segments = normalize_transcript_segments(&[
            TranscriptSegment {
                start_seconds: 1.0,
                end_seconds: 1.4,
                text: "Open".into(),
            },
            TranscriptSegment {
                start_seconds: 1.5,
                end_seconds: 2.0,
                text: "the retry menu.".into(),
            },
            TranscriptSegment {
                start_seconds: 7.0,
                end_seconds: 9.25,
                text: "Inspect the response.".into(),
            },
        ]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Open the retry menu.");
        assert_eq!(
            timestamped_transcript("fallback", &segments),
            "[00:01.0–00:02.0] Open the retry menu.\n[00:07.0–00:09.3] Inspect the response."
        );
    }

    #[test]
    fn update_transcription_persists_timed_sidecars() {
        let root = std::env::temp_dir().join(format!(
            "dicta-timed-transcript-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let video_path = root.join("recording.mp4");
        let metadata_path = root.join("recording.json");
        fs::write(&video_path, []).unwrap();
        let recording = Recording {
            id: "timed".into(),
            project_id: "demo".into(),
            video_path: path_string(&video_path),
            metadata_path: path_string(&metadata_path),
            note: String::new(),
            git_branch: Some("main".into()),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_seconds: Some(30.0),
            size_bytes: Some(0),
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: "processing".into(),
            transcription_error: None,
            transcription_language: Some("en".into()),
            poster_path: None,
            timeline_notes: Vec::new(),
        };
        write_recording(&recording).unwrap();
        let updated = update_transcription(&NativeTranscriptionPayload {
            path: path_string(&video_path),
            transcript: Some("Open the retry menu".into()),
            transcript_segments: vec![TranscriptSegment {
                start_seconds: 12.0,
                end_seconds: 15.5,
                text: "Open the retry menu".into(),
            }],
            error: None,
        })
        .unwrap();
        assert_eq!(updated.transcript_segments.len(), 1);
        assert!(fs::read_to_string(root.join("recording.transcript.md"))
            .unwrap()
            .contains("[00:12.0–00:15.5]"));
        assert!(fs::read_to_string(root.join("recording.transcript.json"))
            .unwrap()
            .contains("transcript_segments"));
        let stored: Recording =
            serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
        assert_eq!(stored.transcript_segments[0].start_seconds, 12.0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_transcripts_are_not_retried() {
        let recording = Recording {
            id: "one".into(),
            project_id: "demo".into(),
            video_path: "/missing.mp4".into(),
            metadata_path: "/missing.json".into(),
            note: String::new(),
            git_branch: None,
            started_at: Utc::now(),
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: "failed".into(),
            transcription_error: Some("no speech".into()),
            transcription_language: Some("en".into()),
            poster_path: None,
            timeline_notes: Vec::new(),
        };
        assert!(!should_retry_transcription(&recording));
        let mut pending = recording.clone();
        pending.transcription_status = "pending".into();
        pending.video_path = path_string(&std::env::temp_dir().join("dicta-retry-missing.mp4"));
        assert!(!should_retry_transcription(&pending));
    }

    #[test]
    fn invalid_settings_language_falls_back_to_auto() {
        let settings = normalize_settings(AppSettings {
            shortcut_id: "command_shift_r".into(),
            cleanup_merged_videos: true,
            transcription_language: "xx".into(),
        });
        assert_eq!(settings.transcription_language, DEFAULT_LANGUAGE);
    }
}
