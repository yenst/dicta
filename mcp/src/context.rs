use crate::storage::{self, ArtifactPolicy, LoadReport, Recording, RecordingSource};
use dicta_core::{branch as core_branch, git as core_git, ProjectFile};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(crate) struct Context {
    pub(crate) repo_root: PathBuf,
    pub(crate) branch: String,
    pub(crate) branch_path: PathBuf,
    pub(crate) recording_sources: Vec<RecordingSource>,
    pub(crate) project: ProjectFile,
}

pub(crate) struct FoundRecording {
    pub(crate) recording: Recording,
    pub(crate) project_name: String,
    pub(crate) scope_label: String,
}

fn branch_recording_sources(
    branches_root: &Path,
    branch: &str,
    policy: ArtifactPolicy,
) -> (PathBuf, Vec<RecordingSource>) {
    let branch_path = core_branch::preferred_dir(branches_root, branch);
    let mut paths = core_branch::existing_dirs(branches_root, branch);
    if paths.is_empty() {
        paths.push(branch_path.clone());
    }
    let sources = paths
        .into_iter()
        .map(|path| RecordingSource {
            path,
            policy: policy.clone(),
        })
        .collect();
    (branch_path, sources)
}

pub(crate) fn resolve(
    repo_path: Option<&str>,
    requested_branch: Option<&str>,
) -> Result<Context, String> {
    let requested_path = match repo_path {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => env::current_dir()
            .map_err(|error| format!("Could not determine current directory: {error}"))?,
    };
    let repo_root = core_git::root(&requested_path).map_err(|error| error.to_string())?;
    let branch = match requested_branch {
        None | Some("") | Some("current") => {
            core_git::branch(&repo_root).map_err(|error| error.to_string())?
        }
        Some(branch) => branch.to_string(),
    };
    let local_storage = repo_root.join(".dicta");
    let local_project_path = local_storage.join("project.json");
    let (project, branch_path, recording_sources) = if local_project_path.is_file() {
        reject_symlink(&local_storage, "Repository-local Dicta storage")?;
        reject_symlink(
            &local_project_path,
            "Repository-local Dicta project metadata",
        )?;
        let canonical_root = local_storage.canonicalize().map_err(|error| {
            format!(
                "Could not resolve repository-local Dicta storage at `{}`: {error}",
                local_storage.display()
            )
        })?;
        let content = fs::read_to_string(&local_project_path).map_err(|error| {
            format!(
                "Dicta found repository-local storage at `{}`, but could not read it: {error}",
                local_storage.display()
            )
        })?;
        let project = serde_json::from_str::<ProjectFile>(&content)
            .map_err(|error| format!("Invalid Dicta project metadata: {error}"))?;
        let policy = ArtifactPolicy::RepositoryLocal { canonical_root };
        let (branch_path, mut branch_sources) =
            branch_recording_sources(&local_storage.join("branches"), &branch, policy.clone());
        let mut sources = vec![RecordingSource {
            path: local_storage.clone(),
            policy,
        }];
        sources.append(&mut branch_sources);
        (project, branch_path, sources)
    } else {
        let storage_root = dicta_root()?;
        let project = find_project(&storage_root, &repo_root).map_err(|legacy_error| {
            format!("No repository-local Dicta storage was found at `{}`. Open Dicta and link this Git project once. Legacy lookup also failed: {legacy_error}", local_storage.display())
        })?;
        let repository_path = storage_root.join(project.id.as_str());
        let legacy_policy = ArtifactPolicy::LegacyProject {
            root: repository_path.clone(),
        };
        let (branch_path, mut branch_sources) = branch_recording_sources(
            &repository_path.join("branches"),
            &branch,
            legacy_policy.clone(),
        );
        let mut sources = vec![RecordingSource {
            path: repository_path,
            policy: legacy_policy,
        }];
        sources.append(&mut branch_sources);
        (project, branch_path, sources)
    };
    Ok(Context {
        repo_root,
        branch,
        branch_path,
        recording_sources,
        project,
    })
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!(
            "{label} must not be a symlink: `{}`",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn load(context: &Context) -> Result<LoadReport, String> {
    let mut report = LoadReport::default();
    for source in &context.recording_sources {
        let mut loaded = storage::load_recordings(source)?;
        report.recordings.append(&mut loaded.recordings);
        report.warnings.append(&mut loaded.warnings);
    }
    report
        .recordings
        .sort_by_key(|recording| std::cmp::Reverse(recording.started_at));
    let mut seen = std::collections::HashSet::new();
    report
        .recordings
        .retain(|recording| seen.insert(recording.id.clone()));
    Ok(report)
}

pub(crate) fn find(context: &Context, recording_id: &str) -> Result<FoundRecording, String> {
    if let Some(recording) = load(context)?
        .recordings
        .into_iter()
        .find(|recording| recording.id.as_str() == recording_id)
    {
        let scope = recording
            .metadata
            .get("recording_scope")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("branch");
        let scope_label = if scope == "repository" {
            "repository-wide".to_string()
        } else {
            format!("branch {}", context.branch)
        };
        return Ok(FoundRecording {
            recording,
            project_name: context.project.name.clone(),
            scope_label,
        });
    }
    let storage_root = dicta_root()?;
    if let Some(recording) = find_general_recording(&storage_root, recording_id)? {
        return Ok(FoundRecording {
            recording,
            project_name: "General".to_string(),
            scope_label: "General".to_string(),
        });
    }
    Err(format!(
        "Recording `{recording_id}` was not found for repository branch `{}` or in General",
        context.branch
    ))
}

pub(crate) fn dicta_root() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("DICTA_HOME") {
        return Ok(PathBuf::from(path));
    }
    let documents =
        dirs::document_dir().ok_or_else(|| "Could not locate the Documents folder".to_string())?;
    Ok(dicta_core::storage::preferred_storage_root(&documents))
}

fn general_sources(storage_root: &Path) -> Vec<RecordingSource> {
    let settings_path = storage_root.join("settings.json");
    let settings = fs::symlink_metadata(&settings_path)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .and_then(|_| fs::read_to_string(settings_path).ok())
        .and_then(|content| {
            serde_json::from_str::<dicta_core::storage::GeneralSettings>(&content).ok()
        })
        .unwrap_or_default();
    let legacy_unprojected = storage_root.join("unprojected");
    dicta_core::storage::general_storage_candidates(storage_root, settings.general_path.as_deref())
        .into_iter()
        .map(|path| {
            let policy = if path == legacy_unprojected {
                ArtifactPolicy::LegacyUnprojected { root: path.clone() }
            } else {
                ArtifactPolicy::ConfinedGeneral { root: path.clone() }
            };
            RecordingSource { path, policy }
        })
        .collect()
}

fn find_general_recording(
    storage_root: &Path,
    recording_id: &str,
) -> Result<Option<Recording>, String> {
    for source in general_sources(storage_root) {
        if let Some(recording) = storage::load_recordings(&source)?
            .recordings
            .into_iter()
            .find(|recording| recording.id.as_str() == recording_id)
        {
            return Ok(Some(recording));
        }
    }
    Ok(None)
}

fn find_project(storage_root: &Path, repo_root: &Path) -> Result<ProjectFile, String> {
    reject_symlink(storage_root, "Dicta storage root")?;
    let entries = fs::read_dir(storage_root).map_err(|_| {
        format!(
            "Dicta storage was not found at `{}`. Open Dicta and link this Git project first.",
            storage_root.display()
        )
    })?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let metadata_path = entry.path().join("project.json");
        let Ok(metadata) = fs::symlink_metadata(&metadata_path) else {
            continue;
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(content) = fs::read_to_string(metadata_path) else {
            continue;
        };
        let Ok(project) = serde_json::from_str::<ProjectFile>(&content) else {
            continue;
        };
        let Some(source) = project.source_path.as_ref() else {
            continue;
        };
        let Ok(source_path) = PathBuf::from(source).canonicalize() else {
            continue;
        };
        if source_path == repo_root {
            return Ok(project);
        }
    }
    Err(format!(
        "No Dicta project is linked to `{}`. Open Dicta and choose Link project folder.",
        repo_root.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_general_fixture(root: &Path, id: &str, value: serde_json::Value) {
        let day = root.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join(format!("{id}.json")), value.to_string()).unwrap();
    }

    #[test]
    fn general_sources_prefer_custom_then_current_then_legacy() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("Dicta");
        let custom = root.path().join("Custom General");
        fs::create_dir_all(&storage).unwrap();
        fs::write(
            storage.join("settings.json"),
            json!({ "general_path": custom }).to_string(),
        )
        .unwrap();
        let paths = general_sources(&storage)
            .into_iter()
            .map(|source| source.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![custom, storage.join("General"), storage.join("unprojected")]
        );
    }

    #[test]
    fn custom_and_current_general_artifacts_cannot_escape_their_roots() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("Dicta");
        let custom = root.path().join("Custom General");
        let current = storage.join("General");
        fs::create_dir_all(&storage).unwrap();
        fs::write(
            storage.join("settings.json"),
            json!({ "general_path": custom }).to_string(),
        )
        .unwrap();

        for (index, general_root) in [&custom, &current].into_iter().enumerate() {
            fs::create_dir_all(general_root).unwrap();
            let escaped_transcript = general_root.parent().unwrap().join("private.md");
            let escaped_video = general_root.parent().unwrap().join("private.mp4");
            fs::write(&escaped_transcript, "must remain private").unwrap();
            fs::write(&escaped_video, "must remain private").unwrap();
            let id = format!("malicious-general-{index}");
            write_general_fixture(
                general_root,
                &id,
                json!({
                    "id": id,
                    "transcript_path": "../../../private.md",
                    "video_path": escaped_video,
                }),
            );
        }

        let sources = general_sources(&storage);
        for source in sources.iter().take(2) {
            let recording = storage::load_recordings(source)
                .unwrap()
                .recordings
                .pop()
                .unwrap();
            assert!(recording.transcript.is_none());
            assert!(recording.video_path.is_empty());
        }
    }

    #[test]
    fn confined_general_accepts_absolute_artifacts_only_inside_its_root() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("Dicta");
        let general = storage.join("General");
        let transcript = general.join("inside.md");
        let video = general.join("inside.mp4");
        fs::create_dir_all(&general).unwrap();
        fs::write(&transcript, "inside transcript").unwrap();
        fs::write(&video, "inside video").unwrap();
        write_general_fixture(
            &general,
            "inside",
            json!({ "id": "inside", "transcript_path": transcript, "video_path": video }),
        );

        let recording = storage::load_recordings(&general_sources(&storage)[0])
            .unwrap()
            .recordings
            .pop()
            .unwrap();
        assert_eq!(recording.transcript.as_deref(), Some("inside transcript"));
        assert_eq!(recording.video_path, video.to_string_lossy());
    }
}
