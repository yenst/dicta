use crate::*;

#[tauri::command]
pub(crate) fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
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
pub(crate) fn create_project(
    app: AppHandle,
    name: String,
    state: State<'_, AppState>,
) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".to_string());
    }
    let created_at = Utc::now();
    let (id, path) = reserve_project_directory(&state.root, name, &created_at)?;
    if let Err(error) = fs::create_dir(path.join("recordings")) {
        let _ = fs::remove_dir(&path);
        return Err(format!("Could not create project: {error}"));
    }
    let metadata = ProjectFile {
        id: id.clone(),
        name: name.to_string(),
        created_at,
        source_path: None,
    };
    let json = serde_json::to_string_pretty(&metadata)
        .map_err(|error| format!("Could not serialize project: {error}"))?;
    if let Err(error) = fs::write(path.join("project.json"), format!("{json}\n")) {
        let _ = fs::remove_dir(path.join("recordings"));
        let _ = fs::remove_dir(&path);
        return Err(format!("Could not save project: {error}"));
    }

    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    inner.status.active_project_id = Some(id.to_string());
    drop(inner);
    let _ = sync_tray_menu(&app);
    Ok(project_view(&state.root, metadata))
}

#[tauri::command]
pub(crate) fn link_project(
    app: AppHandle,
    source_path: String,
    state: State<'_, AppState>,
) -> Result<Project, String> {
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
            drop(inner);
            let _ = sync_tray_menu(&app);
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
    let id = ProjectId::new(id).map_err(|error| error.to_string())?;
    let created_at = Utc::now();
    let metadata = ProjectFile {
        id: id.clone(),
        name,
        created_at,
        source_path: Some(source_string),
    };
    let project_path = project_dir(&state.root, id.as_str());
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
    inner.status.active_project_id = Some(id.to_string());
    drop(inner);
    let _ = sync_tray_menu(&app);
    Ok(project_view(&state.root, metadata))
}

pub(crate) fn remove_project_registration(root: &Path, project_id: &str) -> Result<(), String> {
    if project_id == UNPROJECTED_ID {
        return Err("General cannot be removed".to_string());
    }
    if project_id.is_empty()
        || Path::new(project_id)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(project_id)
    {
        return Err("Invalid project identifier".to_string());
    }
    let metadata = read_project(root, project_id)?;
    if metadata.id.as_str() != project_id {
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
pub(crate) fn remove_project(
    app: AppHandle,
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
    drop(inner);
    ensure_default_project_selection(&app);
    let _ = sync_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub(crate) fn refresh_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Project, String> {
    let metadata = read_project(&state.root, &project_id)?;
    Ok(project_view(&state.root, metadata))
}

#[tauri::command]
pub(crate) fn get_app_settings(state: State<'_, AppState>) -> AppSettings {
    read_settings(&state.root)
}

#[tauri::command]
pub(crate) fn set_shortcut(
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
pub(crate) fn set_cleanup_merged_videos(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = read_settings(&state.root);
    settings.cleanup_merged_videos = enabled;
    write_settings(&state.root, &settings)?;
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_branch_locking(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if matches!(
        inner.status.phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    ) {
        return Err("Recording scope cannot change during a recording".to_string());
    }
    drop(inner);
    let mut settings = read_settings(&state.root);
    settings.branch_locking = enabled;
    write_settings(&state.root, &settings)?;
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_transcription_language(
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
pub(crate) fn cleanup_merged_videos(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<CleanupSummary, String> {
    let metadata = read_project(&state.root, &project_id)?;
    cleanup_merged_videos_for_project(&state.root, &metadata)
}

#[tauri::command]
pub(crate) fn select_project(
    app: AppHandle,
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
    drop(inner);
    let _ = sync_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub(crate) fn list_recordings(
    app: AppHandle,
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Recording>, String> {
    let mut recordings = load_recordings(&state.root, &project_id)?;
    let asset_scope = app.asset_protocol_scope();
    for recording in &mut recordings {
        if fs::symlink_metadata(&recording.video_path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            asset_scope
                .allow_file(&recording.video_path)
                .map_err(|error| format!("Could not grant video playback access: {error}"))?;
        }
        if let Some(poster_path) = recording.poster_path.as_deref() {
            if fs::symlink_metadata(poster_path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            {
                asset_scope
                    .allow_file(poster_path)
                    .map_err(|error| format!("Could not grant poster access: {error}"))?;
            }
        }
    }
    Ok(recordings)
}

#[tauri::command]
pub(crate) fn ensure_recording_poster(
    app: AppHandle,
    project_id: String,
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let recording = load_recordings(&state.root, &project_id)?
        .into_iter()
        .find(|recording| recording.id.as_str() == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    let recording = attach_poster(recording)?;
    if let Some(poster_path) = recording.poster_path.as_deref() {
        app.asset_protocol_scope()
            .allow_file(poster_path)
            .map_err(|error| format!("Could not grant poster access: {error}"))?;
    }
    Ok(recording.poster_path)
}

pub(crate) fn recording_artifact_paths(metadata_path: &Path) -> Vec<PathBuf> {
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

pub(crate) fn discard_recording_artifacts(recording: &Recording) {
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
pub(crate) fn delete_recording(
    project_id: String,
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let project = read_project(&state.root, &project_id)?;
    let located = locate_recording(&state.root, &project, &recording_id)?;
    let artifacts = recording_artifact_paths(&located.metadata_path);
    if located.repository_local {
        for artifact in &artifacts {
            let metadata = match fs::symlink_metadata(artifact) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!("Could not inspect {}: {error}", artifact.display()));
                }
            };
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Refusing symlinked recording artifact: {}",
                    artifact.display()
                ));
            }
            let canonical = artifact
                .canonicalize()
                .map_err(|error| format!("Could not resolve {}: {error}", artifact.display()))?;
            if !canonical.starts_with(&located.recordings_root) || !metadata.is_file() {
                return Err("Refusing to delete an artifact outside the active branch".to_string());
            }
        }
    }
    for artifact in artifacts {
        if fs::symlink_metadata(&artifact).is_ok_and(|metadata| metadata.is_file()) {
            fs::remove_file(&artifact)
                .map_err(|error| format!("Could not delete {}: {error}", artifact.display()))?;
        }
    }
    if let Some(day_dir) = located.metadata_path.parent() {
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
pub(crate) fn save_timeline_notes(
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
    let located = locate_recording(&state.root, &project, &recording_id)?;
    let canonical_metadata = located.metadata_path;
    let canonical_metadata_string = path_string(&canonical_metadata);
    let (recording, ()) =
        storage::recordings::update::<Recording, _>(&canonical_metadata, move |recording| {
            if recording.duration_seconds.is_some_and(|duration| {
                timeline_notes
                    .iter()
                    .any(|note| note.timestamp_seconds > duration + 0.5)
            }) {
                return Err(
                    "A timeline note cannot be placed beyond the end of the recording".to_string(),
                );
            }
            recording.metadata_path = canonical_metadata_string;
            recording.timeline_notes = timeline_notes;
            recording.timeline_notes.sort_by(|left, right| {
                left.timestamp_seconds
                    .partial_cmp(&right.timestamp_seconds)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(())
        })?;
    Ok(recording)
}
