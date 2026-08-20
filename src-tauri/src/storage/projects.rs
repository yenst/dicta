use crate::*;

pub(crate) fn slugify(name: &str) -> String {
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

pub(crate) fn unique_path_suffix(timestamp_nanos: i64) -> String {
    let sequence = UNIQUE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:x}-{:x}-{:x}",
        timestamp_nanos,
        std::process::id(),
        sequence
    )
}

pub(crate) fn reserve_project_directory(
    root: &Path,
    name: &str,
    created_at: &DateTime<Utc>,
) -> Result<(ProjectId, PathBuf), String> {
    let timestamp_nanos = created_at
        .timestamp_nanos_opt()
        .unwrap_or_else(|| created_at.timestamp_micros().saturating_mul(1_000));
    for _ in 0..100 {
        let id = ProjectId::new(format!(
            "{}-{}-{}",
            slugify(name),
            created_at.format("%y%m%d%H%M%S"),
            unique_path_suffix(timestamp_nanos)
        ))
        .map_err(|error| error.to_string())?;
        let path = project_dir(root, id.as_str());
        match fs::create_dir(&path) {
            Ok(()) => return Ok((id, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not create project: {error}")),
        }
    }
    Err("Could not reserve a unique project folder".to_string())
}

pub(crate) fn reserve_recording_paths(
    day_dir: &Path,
    started_at: &DateTime<Local>,
) -> Result<(String, PathBuf, PathBuf), String> {
    let timestamp_nanos = started_at
        .timestamp_nanos_opt()
        .unwrap_or_else(|| started_at.timestamp_micros().saturating_mul(1_000));
    for _ in 0..100 {
        let stem = format!(
            "{}-{}",
            started_at.format("%H-%M-%S"),
            unique_path_suffix(timestamp_nanos)
        );
        let metadata_path = day_dir.join(format!("{stem}.json"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metadata_path)
        {
            Ok(_) => {
                let video_path = day_dir.join(format!("{stem}.mp4"));
                return Ok((stem, video_path, metadata_path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Could not reserve recording metadata: {error}"));
            }
        }
    }
    Err("Could not reserve unique recording files".to_string())
}

pub(crate) fn project_dir(root: &Path, project_id: &str) -> PathBuf {
    if project_id == UNPROJECTED_ID {
        let settings = read_settings(root);
        dicta_core::storage::general_storage_path(root, settings.general_path.as_deref())
    } else {
        root.join(project_id)
    }
}

pub(crate) fn unprojected_metadata() -> ProjectFile {
    ProjectFile {
        id: ProjectId::new(UNPROJECTED_ID).expect("General project ID is valid"),
        name: "General".to_string(),
        created_at: DateTime::<Utc>::from(std::time::UNIX_EPOCH),
        source_path: None,
    }
}

#[tauri::command]
pub(crate) fn set_general_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<Project, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("Choose a folder for General recordings".to_string());
    }
    fs::create_dir_all(&target)
        .map_err(|error| format!("Could not create {}: {error}", target.display()))?;
    if !target.is_dir() {
        return Err(format!("{} is not a folder", target.display()));
    }
    let canonical = target.canonicalize().unwrap_or(target);
    let mut settings = read_settings(&state.root);
    settings.general_path = Some(path_string(&canonical));
    write_settings(&state.root, &settings)?;
    Ok(project_view(&state.root, unprojected_metadata()))
}

pub(crate) fn project_view(root: &Path, metadata: ProjectFile) -> Project {
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
        id: metadata.id.into_string(),
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

pub(crate) fn read_project(root: &Path, project_id: &str) -> Result<ProjectFile, String> {
    let requested_id = ProjectId::new(project_id.to_string())
        .map_err(|_| "Invalid project identifier".to_string())?;
    if requested_id.as_str() == UNPROJECTED_ID {
        return Ok(unprojected_metadata());
    }
    let path = project_dir(root, requested_id.as_str()).join("project.json");
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let metadata: ProjectFile = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid project metadata: {error}"))?;
    if metadata.id != requested_id {
        return Err("Project registration does not match the requested project".to_string());
    }
    Ok(metadata)
}

pub(crate) fn recording_files(project_path: &Path) -> Vec<PathBuf> {
    scan_recording_files(&project_path.join("recordings"), false).unwrap_or_default()
}

#[derive(Debug)]
pub(crate) struct LocatedRecording {
    pub(crate) recording: Recording,
    pub(crate) metadata_path: PathBuf,
    pub(crate) recordings_root: PathBuf,
    pub(crate) repository_local: bool,
}

fn path_is_symlink(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Could not inspect {}: {error}", path.display())),
    }
}

fn reject_symlinks_between(base: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(base)
        .map_err(|_| "Recording storage is outside the linked repository".to_string())?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if path_is_symlink(&current)? {
            return Err(format!(
                "Refusing symlinked recording storage: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn scan_recording_files(recordings_dir: &Path, strict: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let days = match fs::read_dir(recordings_dir) {
        Ok(days) => days,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(error) => {
            return Err(format!(
                "Could not read recording folder {}: {error}",
                recordings_dir.display()
            ));
        }
    };
    for day in days {
        let day = day.map_err(|error| format!("Could not inspect recording day: {error}"))?;
        let day_type = day
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", day.path().display()))?;
        if day_type.is_symlink() {
            if strict {
                return Err(format!(
                    "Refusing symlinked recording day: {}",
                    day.path().display()
                ));
            }
            continue;
        }
        if !day_type.is_dir() {
            continue;
        }
        let entries = fs::read_dir(day.path())
            .map_err(|error| format!("Could not read {}: {error}", day.path().display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("Could not inspect recording: {error}"))?;
            let entry_type = entry.file_type().map_err(|error| {
                format!("Could not inspect {}: {error}", entry.path().display())
            })?;
            if entry_type.is_symlink() {
                if strict {
                    return Err(format!(
                        "Refusing symlinked recording artifact: {}",
                        entry.path().display()
                    ));
                }
                continue;
            }
            let is_transcript_sidecar = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".transcript.json"));
            if entry_type.is_file()
                && !is_transcript_sidecar
                && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

fn recording_tree(
    root: &Path,
    project: &ProjectFile,
) -> Result<(PathBuf, Vec<PathBuf>, bool), String> {
    let repository_local = project.source_path.is_some();
    if repository_local {
        let source = Path::new(project.source_path.as_deref().expect("checked above"));
        let storage = project_storage_dir(root, project);
        reject_symlinks_between(source, &storage)?;
        let branches = storage.join("branches");
        if path_is_symlink(&branches)? {
            return Err("Refusing symlinked branch recording storage".to_string());
        }
        if read_settings(root).branch_locking {
            let branch = git_branch(source)?;
            let paths = core_branch::paths(&branches, &branch);
            if path_is_symlink(&paths.current)? || path_is_symlink(&paths.legacy)? {
                return Err("Refusing symlinked branch recording storage".to_string());
            }
        }
    }
    let (_, active_root) = active_recording_root(root, project)?;
    if repository_local {
        let source = Path::new(project.source_path.as_deref().expect("checked above"));
        let storage = project_storage_dir(root, project);
        reject_symlinks_between(&storage, &active_root)?;
        let canonical_source = source
            .canonicalize()
            .map_err(|error| format!("Could not resolve linked project: {error}"))?;
        let canonical_storage = storage
            .canonicalize()
            .map_err(|error| format!("Could not resolve linked recording storage: {error}"))?;
        if !canonical_storage.starts_with(&canonical_source) {
            return Err("Recording storage escaped the linked repository".to_string());
        }
    }
    let recordings_dir = active_root.join("recordings");
    if repository_local && path_is_symlink(&recordings_dir)? {
        return Err("Refusing symlinked recordings folder".to_string());
    }
    let files = scan_recording_files(&recordings_dir, repository_local)?;
    let canonical_root = if recordings_dir.exists() {
        recordings_dir
            .canonicalize()
            .map_err(|error| format!("Could not resolve recording folder: {error}"))?
    } else {
        recordings_dir
    };
    if repository_local {
        let canonical_storage = project_storage_dir(root, project)
            .canonicalize()
            .map_err(|error| format!("Could not resolve linked recording storage: {error}"))?;
        if !canonical_root.starts_with(&canonical_storage) {
            return Err("Recording folder escaped the linked repository".to_string());
        }
    }
    Ok((canonical_root, files, repository_local))
}

fn confined_artifact(
    raw_path: &str,
    metadata_path: &Path,
    recordings_root: &Path,
) -> Result<PathBuf, String> {
    let raw = Path::new(raw_path);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        metadata_path
            .parent()
            .ok_or_else(|| "Recording metadata has no parent folder".to_string())?
            .join(raw)
    };
    if path_is_symlink(&candidate)? {
        return Err(format!(
            "Refusing symlinked recording artifact: {}",
            candidate.display()
        ));
    }
    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("Could not resolve {}: {error}", candidate.display()))?;
        if !canonical.starts_with(recordings_root) || !canonical.is_file() {
            return Err("Recording artifact escaped the active recording folder".to_string());
        }
        return Ok(canonical);
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| "Recording artifact has no parent folder".to_string())?
        .canonicalize()
        .map_err(|error| format!("Could not resolve recording artifact folder: {error}"))?;
    if !parent.starts_with(recordings_root) {
        return Err("Recording artifact escaped the active recording folder".to_string());
    }
    Ok(parent.join(
        candidate
            .file_name()
            .ok_or_else(|| "Recording artifact has no file name".to_string())?,
    ))
}

fn load_located_recordings(
    root: &Path,
    project: &ProjectFile,
) -> Result<Vec<LocatedRecording>, String> {
    let (recordings_root, files, repository_local) = recording_tree(root, project)?;
    let mut recordings = Vec::new();
    for path in files {
        let canonical_metadata = path
            .canonicalize()
            .map_err(|error| format!("Could not resolve recording metadata: {error}"))?;
        if !canonical_metadata.starts_with(&recordings_root) {
            return Err("Recording metadata escaped the active recording folder".to_string());
        }
        let content = fs::read_to_string(&canonical_metadata)
            .map_err(|error| format!("Could not read recording metadata: {error}"))?;
        let mut recording: Recording = match serde_json::from_str(&content) {
            Ok(recording) => recording,
            Err(error) if repository_local => {
                return Err(format!("Invalid recording metadata: {error}"));
            }
            Err(_) => continue,
        };
        if repository_local {
            if recording.project_id != project.id {
                return Err("Recording metadata belongs to another project".to_string());
            }
            recording.video_path = path_string(&confined_artifact(
                &recording.video_path,
                &canonical_metadata,
                &recordings_root,
            )?);
            if let Some(transcript) = recording.transcript_path.as_deref() {
                recording.transcript_path = Some(path_string(&confined_artifact(
                    transcript,
                    &canonical_metadata,
                    &recordings_root,
                )?));
            }
            let poster = recording
                .poster_path
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| poster_path_for_video(&recording.video_path));
            if poster.exists() || path_is_symlink(&poster)? {
                recording.poster_path = Some(path_string(&confined_artifact(
                    &path_string(&poster),
                    &canonical_metadata,
                    &recordings_root,
                )?));
            } else {
                recording.poster_path = None;
            }
        }
        recording.metadata_path = path_string(&canonical_metadata);
        recordings.push(LocatedRecording {
            recording,
            metadata_path: canonical_metadata,
            recordings_root: recordings_root.clone(),
            repository_local,
        });
    }
    Ok(recordings)
}

pub(crate) fn locate_recording(
    root: &Path,
    project: &ProjectFile,
    recording_id: &str,
) -> Result<LocatedRecording, String> {
    let requested = RecordingId::new(recording_id.to_string())
        .map_err(|_| "Invalid recording identifier".to_string())?;
    load_located_recordings(root, project)?
        .into_iter()
        .find(|located| located.recording.id == requested)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))
}

pub(crate) fn load_recordings(root: &Path, project_id: &str) -> Result<Vec<Recording>, String> {
    let metadata = read_project(root, project_id)?;
    let mut recordings: Vec<Recording> = load_located_recordings(root, &metadata)?
        .into_iter()
        .map(|located| located.recording)
        .collect();
    recordings.sort_by_key(|recording| std::cmp::Reverse(recording.started_at));
    Ok(recordings)
}

pub(crate) fn load_projects(root: &Path) -> Vec<Project> {
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
    projects.sort_by_key(|project| std::cmp::Reverse(project.created_at));
    projects.insert(0, project_view(root, unprojected_metadata()));
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dicta-project-security-{label}-{}",
            unique_path_suffix(Utc::now().timestamp_nanos_opt().unwrap_or_default())
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn project_file(id: &str) -> ProjectFile {
        ProjectFile {
            id: ProjectId::new(id).unwrap(),
            name: "Security test".to_string(),
            created_at: Utc::now(),
            source_path: None,
        }
    }

    #[test]
    fn read_project_rejects_traversal_before_touching_disk() {
        let root = test_directory("traversal");
        assert_eq!(
            read_project(&root, "../outside").unwrap_err(),
            "Invalid project identifier"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_project_rejects_metadata_id_mismatch() {
        let root = test_directory("mismatch");
        let requested = root.join("requested");
        fs::create_dir_all(&requested).unwrap();
        fs::write(
            requested.join("project.json"),
            serde_json::to_vec(&project_file("different")).unwrap(),
        )
        .unwrap();

        assert_eq!(
            read_project(&root, "requested").unwrap_err(),
            "Project registration does not match the requested project"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn strict_scan_rejects_symlinked_day_and_metadata() {
        use std::os::unix::fs::symlink;

        let root = test_directory("symlinks");
        let recordings = root.join("recordings");
        let outside_day = root.join("outside-day");
        fs::create_dir_all(&recordings).unwrap();
        fs::create_dir_all(&outside_day).unwrap();
        symlink(&outside_day, recordings.join("2026-08-20")).unwrap();
        assert!(scan_recording_files(&recordings, true)
            .unwrap_err()
            .contains("symlinked recording day"));

        fs::remove_file(recordings.join("2026-08-20")).unwrap();
        let day = recordings.join("2026-08-20");
        fs::create_dir(&day).unwrap();
        let outside_metadata = root.join("outside.json");
        fs::write(&outside_metadata, b"{}").unwrap();
        symlink(&outside_metadata, day.join("recording.json")).unwrap();
        assert!(scan_recording_files(&recordings, true)
            .unwrap_err()
            .contains("symlinked recording artifact"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confined_artifact_rejects_escape_from_recordings_root() {
        let root = test_directory("artifact-escape");
        let recordings = root.join("recordings");
        let day = recordings.join("2026-08-20");
        fs::create_dir_all(&day).unwrap();
        let metadata = day.join("recording.json");
        fs::write(&metadata, b"{}").unwrap();
        let outside = root.join("outside.mp4");
        fs::write(&outside, b"outside").unwrap();

        let error = confined_artifact(
            &path_string(&outside),
            &metadata,
            &recordings.canonicalize().unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("escaped the active recording folder"));
        fs::remove_dir_all(root).unwrap();
    }
}
