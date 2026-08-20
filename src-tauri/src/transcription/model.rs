use crate::*;

pub(crate) fn models_dir() -> Result<PathBuf, String> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not locate the local application data folder".to_string())?;
    Ok(data_dir.join("Dicta").join("models"))
}

pub(crate) fn quality_model_path() -> Result<PathBuf, String> {
    Ok(models_dir()?.join(QUALITY_MODEL_FILENAME))
}

pub(crate) fn bundled_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not locate Dicta resources: {error}"))?
        .join("ggml-base-q5_1.bin"))
}

pub(crate) fn whisper_model_candidates(app: &AppHandle) -> Result<Vec<PathBuf>, String> {
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

pub(crate) fn selected_whisper_model(app: &AppHandle) -> Result<PathBuf, String> {
    whisper_model_candidates(app)?
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "Dicta's local Whisper model is missing".to_string())
}

pub(crate) fn model_label(path: &Path) -> String {
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

pub(crate) fn current_model_status(app: &AppHandle) -> Result<ModelStatus, String> {
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
            "Dicta is using a high-quality model found on this computer. Install a Dicta-managed copy for a portable setup.".to_string()
        } else {
            "The compact offline model is active. Download high quality for better Dutch and technical speech.".to_string()
        },
    })
}

pub(crate) fn emit_model_download(
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
pub(crate) fn model_status(app: AppHandle) -> Result<ModelStatus, String> {
    current_model_status(&app)
}

#[tauri::command]
pub(crate) async fn download_quality_model(app: AppHandle) -> Result<ModelStatus, String> {
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
        let mut child = std::process::Command::new("curl")
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
        #[cfg(target_os = "macos")]
        let checksum = std::process::Command::new("shasum")
            .args(["-a", "1"])
            .arg(&staging)
            .output()
            .map_err(|error| format!("Could not verify the downloaded model: {error}"))?;
        #[cfg(not(target_os = "macos"))]
        let checksum = std::process::Command::new("sha1sum")
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
