use crate::*;

pub(crate) fn start_recording_inner(
    app: &AppHandle,
    note: String,
) -> Result<RecorderStatus, String> {
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
    let (stem, video_path, metadata_path) = reserve_recording_paths(&day_dir, &now_local)?;
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

    let recording = Recording {
        id: RecordingId::new(format!("{}-{}", now_local.format("%Y%m%d"), stem))
            .map_err(|error| error.to_string())?,
        project_id: ProjectId::new(project_id.clone()).map_err(|error| error.to_string())?,
        video_path: video_path_string.clone(),
        metadata_path: path_string(&metadata_path),
        note,
        recording_scope: if project_id == UNPROJECTED_ID {
            RecordingScope::Unprojected
        } else if git_branch.is_some() {
            RecordingScope::Branch
        } else {
            RecordingScope::Repository
        },
        git_branch,
        started_at,
        ended_at: None,
        duration_seconds: None,
        size_bytes: None,
        success: false,
        transcript: None,
        transcript_path: None,
        transcript_segments: Vec::new(),
        transcription_status: TranscriptionStatus::Pending,
        transcription_error: None,
        transcription_language: None,
        poster_path: None,
        timeline_notes: Vec::new(),
    };
    if let Err(error) = write_recording(&recording) {
        let _ = fs::remove_file(&metadata_path);
        return Err(error);
    }
    inner.session = Some(recording);
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

    if let Err(error) = platform::start_recording(&video_path_string, native_recorder_callback) {
        let (status, _) = finalize_session(app, Some(error.clone()));
        emit_recorder_event(app, "error", &error, status);
        return Err(error);
    }

    let current_status = state
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .status
        .clone();
    Ok(current_status)
}

#[tauri::command]
pub(crate) fn start_recording(app: AppHandle, note: String) -> Result<RecorderStatus, String> {
    start_recording_inner(&app, note)
}

pub(crate) fn stop_recording_inner(app: &AppHandle) -> Result<RecorderStatus, String> {
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
    match platform::stop_recording(native_recorder_callback) {
        Ok(()) => Ok(status),
        Err(error) => {
            let status = {
                let mut inner = state
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                restore_after_stop_rejection(&mut inner, &error);
                inner.status.clone()
            };
            emit_recorder_event(app, "error", &error, status);
            Err(error)
        }
    }
}

pub(crate) fn restore_after_stop_rejection(inner: &mut InnerState, error: &str) {
    if matches!(inner.status.phase, RecordingPhase::Stopping) {
        inner.status.phase = RecordingPhase::Recording;
    }
    inner.status.last_error = Some(error.to_string());
}

#[tauri::command]
pub(crate) fn stop_recording(app: AppHandle) -> Result<RecorderStatus, String> {
    stop_recording_inner(&app)
}

pub(crate) fn emit_recorder_event(
    app: &AppHandle,
    event: &str,
    message: &str,
    status: RecorderStatus,
) {
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

pub(crate) fn sync_tray(app: &AppHandle, phase: &RecordingPhase) {
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
    let _ = sync_tray_menu(app);
}

pub(crate) fn schedule_recording_limit(app: &AppHandle, started_at: DateTime<Utc>) {
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

pub(crate) fn finalize_session(
    app: &AppHandle,
    error: Option<String>,
) -> (RecorderStatus, Option<String>) {
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
                recording.transcription_status = TranscriptionStatus::Processing;
                recording.transcription_error = None;
                recording.transcription_language = Some(language);
                poster_source = Some(recording.video_path.clone());
                video_path = Some(recording.video_path.clone());
            } else {
                recording.transcription_status = TranscriptionStatus::Failed;
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
            let _ = storage::recordings::update::<Recording, _>(&metadata_path, |recording| {
                recording.poster_path = Some(poster);
                Ok(())
            });
        }
    }
    (status, video_path)
}

pub(crate) fn accepts_capture_event(inner: &InnerState, event: &str) -> bool {
    if inner.session.is_none() {
        return false;
    }
    match event {
        "started" => matches!(inner.status.phase, RecordingPhase::Preparing),
        "finished" => matches!(
            inner.status.phase,
            RecordingPhase::Recording | RecordingPhase::Stopping
        ),
        "error" => matches!(
            inner.status.phase,
            RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
        ),
        _ => true,
    }
}

pub(crate) extern "C" fn native_recorder_callback(event: *const c_char, message: *const c_char) {
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
                if !accepts_capture_event(&inner, "started") {
                    return;
                }
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
            let is_current_session = {
                let inner = state
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                accepts_capture_event(&inner, "finished")
            };
            if !is_current_session {
                return;
            }
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
            let is_current_session = {
                let inner = state
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                accepts_capture_event(&inner, "error")
            };
            if !is_current_session {
                return;
            }
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

pub(crate) fn toggle_from_shortcut(app: &AppHandle) {
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
