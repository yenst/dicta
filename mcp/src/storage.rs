use chrono::{DateTime, Utc};
use dicta_core::{RecordingId, TranscriptSegment};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub(crate) enum ArtifactPolicy {
    RepositoryLocal { canonical_root: PathBuf },
    ConfinedGeneral { root: PathBuf },
    TrustedLegacyProject,
    TrustedLegacyUnprojected,
}

#[derive(Clone, Debug)]
pub(crate) struct RecordingSource {
    pub(crate) path: PathBuf,
    pub(crate) policy: ArtifactPolicy,
}

#[derive(Clone, Debug)]
pub(crate) struct Recording {
    pub(crate) id: RecordingId,
    pub(crate) note: String,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) video_path: String,
    pub(crate) metadata_path: String,
    pub(crate) transcript: Option<String>,
    pub(crate) transcript_segments: Vec<TranscriptSegment>,
    pub(crate) metadata: Value,
}

#[derive(Default)]
pub(crate) struct LoadReport {
    pub(crate) recordings: Vec<Recording>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn load_recordings(source: &RecordingSource) -> Result<LoadReport, String> {
    if !validate_source(source)? {
        return Ok(LoadReport {
            warnings: vec![format!(
                "Ignored symlinked Dicta storage `{}`",
                source.path.display()
            )],
            ..LoadReport::default()
        });
    }
    let recordings_root = source.path.join("recordings");
    if !recordings_root.exists() {
        return Ok(LoadReport::default());
    }
    if is_symlink(&recordings_root) {
        return match &source.policy {
            ArtifactPolicy::RepositoryLocal { .. } | ArtifactPolicy::ConfinedGeneral { .. } => {
                Err(format!(
                    "Confined recordings directory must not be a symlink: `{}`",
                    recordings_root.display()
                ))
            }
            ArtifactPolicy::TrustedLegacyProject | ArtifactPolicy::TrustedLegacyUnprojected => {
                Ok(LoadReport {
                    warnings: vec![format!(
                        "Ignored symlinked trusted legacy recordings directory `{}`",
                        recordings_root.display()
                    )],
                    ..LoadReport::default()
                })
            }
        };
    }

    let mut report = LoadReport::default();
    let days = fs::read_dir(&recordings_root)
        .map_err(|error| format!("Could not read recordings: {error}"))?;
    for day in days {
        let day = match day {
            Ok(day) => day,
            Err(error) => {
                report.warnings.push(format!(
                    "Could not inspect an entry in `{}`: {error}",
                    recordings_root.display()
                ));
                continue;
            }
        };
        let Ok(file_type) = day.file_type() else {
            report
                .warnings
                .push(format!("Could not inspect `{}`", day.path().display()));
            continue;
        };
        if file_type.is_symlink() {
            report.warnings.push(format!(
                "Ignored symlinked recording day `{}`",
                day.path().display()
            ));
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(day.path()) {
            Ok(entries) => entries,
            Err(error) => {
                report.warnings.push(format!(
                    "Could not read recording day `{}`: {error}",
                    day.path().display()
                ));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                report
                    .warnings
                    .push(format!("Could not inspect `{}`", path.display()));
                continue;
            };
            if file_type.is_symlink() {
                report
                    .warnings
                    .push(format!("Ignored symlinked artifact `{}`", path.display()));
                continue;
            }
            let is_transcript_sidecar = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".transcript.json"));
            if !file_type.is_file()
                || is_transcript_sidecar
                || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            {
                continue;
            }
            match read_recording(path.clone(), &source.policy) {
                Ok(recording) => report.recordings.push(recording),
                Err(error) => report
                    .warnings
                    .push(format!("Ignored `{}`: {error}", path.display())),
            }
        }
    }
    report
        .recordings
        .sort_by_key(|recording| std::cmp::Reverse(recording.started_at));
    Ok(report)
}

fn validate_source(source: &RecordingSource) -> Result<bool, String> {
    if is_symlink(&source.path) {
        return match &source.policy {
            ArtifactPolicy::RepositoryLocal { .. } | ArtifactPolicy::ConfinedGeneral { .. } => {
                Err(format!(
                    "Confined recording storage must not be a symlink: `{}`",
                    source.path.display()
                ))
            }
            ArtifactPolicy::TrustedLegacyProject | ArtifactPolicy::TrustedLegacyUnprojected => {
                Ok(false)
            }
        };
    }
    if !source.path.exists() {
        return Ok(true);
    }
    let canonical_root = match &source.policy {
        ArtifactPolicy::RepositoryLocal { canonical_root } => canonical_root.clone(),
        ArtifactPolicy::ConfinedGeneral { root } => root.canonicalize().map_err(|error| {
            format!(
                "Could not resolve confined General storage at `{}`: {error}",
                root.display()
            )
        })?,
        ArtifactPolicy::TrustedLegacyProject | ArtifactPolicy::TrustedLegacyUnprojected => {
            return Ok(true);
        }
    };
    let canonical_source = source.path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve confined recording storage at `{}`: {error}",
            source.path.display()
        )
    })?;
    if !canonical_source.starts_with(&canonical_root) {
        return Err(format!(
            "Confined recording storage escapes `{}`: `{}`",
            canonical_root.display(),
            source.path.display()
        ));
    }
    Ok(true)
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn read_recording(metadata_path: PathBuf, policy: &ArtifactPolicy) -> Result<Recording, String> {
    if !valid_artifact_file(&metadata_path, policy) {
        return Err("metadata is not a permitted regular file".to_string());
    }
    let content = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("could not read metadata: {error}"))?;
    let metadata = serde_json::from_str::<Value>(&content)
        .map_err(|error| format!("invalid recording metadata: {error}"))?;
    let raw_id = metadata
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "recording id is missing".to_string())?;
    let id = RecordingId::new(raw_id).map_err(|error| error.to_string())?;
    let note = metadata
        .get("note")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let started_at = metadata
        .get("started_at")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok());
    let duration_seconds = metadata.get("duration_seconds").and_then(Value::as_f64);
    let recorded_video_path = metadata
        .get("video_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let local_video_path = metadata_path.with_extension("mp4");
    let video_path = if valid_artifact_file(&local_video_path, policy) {
        local_video_path.to_string_lossy().into_owned()
    } else {
        resolve_metadata_artifact(recorded_video_path, &metadata_path, policy)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let transcript = metadata
        .get("transcript")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| read_transcript(&metadata, &metadata_path, policy));
    let transcript_segments = metadata
        .get("transcript_segments")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<TranscriptSegment>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(TranscriptSegment::is_valid)
        .collect();
    Ok(Recording {
        id,
        note,
        started_at,
        duration_seconds,
        video_path,
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        transcript,
        transcript_segments,
        metadata,
    })
}

fn read_transcript(
    metadata: &Value,
    metadata_path: &Path,
    policy: &ArtifactPolicy,
) -> Option<String> {
    if let Some(path) = metadata.get("transcript_path").and_then(Value::as_str) {
        if let Some(path) = resolve_metadata_artifact(path, metadata_path, policy) {
            return fs::read_to_string(path).ok();
        }
    }
    let stem = metadata_path.file_stem()?.to_str()?;
    for file_name in [format!("{stem}.transcript.md"), format!("{stem}.md")] {
        let path = metadata_path.with_file_name(file_name);
        if valid_artifact_file(&path, policy) {
            return fs::read_to_string(path).ok();
        }
    }
    None
}

fn resolve_metadata_artifact(
    raw_path: &str,
    metadata_path: &Path,
    policy: &ArtifactPolicy,
) -> Option<PathBuf> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw_path);
    if path.is_absolute() && matches!(policy, ArtifactPolicy::RepositoryLocal { .. }) {
        return None;
    }
    let candidate = if path.is_absolute() {
        path
    } else {
        metadata_path.parent()?.join(path)
    };
    valid_artifact_file(&candidate, policy).then(|| candidate.canonicalize().ok())?
}

fn valid_artifact_file(path: &Path, policy: &ArtifactPolicy) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(canonical_path) = path.canonicalize() else {
        return false;
    };
    match policy {
        ArtifactPolicy::RepositoryLocal { canonical_root } => {
            canonical_path.starts_with(canonical_root)
        }
        ArtifactPolicy::ConfinedGeneral { root } => root
            .canonicalize()
            .is_ok_and(|canonical_root| canonical_path.starts_with(canonical_root)),
        ArtifactPolicy::TrustedLegacyProject | ArtifactPolicy::TrustedLegacyUnprojected => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_metadata(root: &Path, name: &str, value: Value) -> PathBuf {
        let day = root.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        let path = day.join(format!("{name}.json"));
        fs::write(&path, value.to_string()).unwrap();
        path
    }

    fn confined_source(path: PathBuf, storage_root: &Path) -> RecordingSource {
        RecordingSource {
            path,
            policy: ArtifactPolicy::RepositoryLocal {
                canonical_root: storage_root.canonicalize().unwrap(),
            },
        }
    }

    #[test]
    fn repository_metadata_cannot_escape_the_dicta_root() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("repo/.dicta");
        let branch = storage.join("branches/main");
        let secret = root.path().join("secret.txt");
        let video = root.path().join("secret.mp4");
        fs::write(&secret, "private transcript").unwrap();
        fs::write(&video, "private video").unwrap();
        let metadata = write_metadata(
            &branch,
            "malicious",
            json!({ "id": "malicious", "transcript_path": secret, "video_path": video }),
        );
        let source = confined_source(branch, &storage);
        let recording = load_recordings(&source).unwrap().recordings.pop().unwrap();
        assert!(recording.transcript.is_none());
        assert!(recording.video_path.is_empty());
        assert!(resolve_metadata_artifact(
            "../../../../../../secret.txt",
            &metadata,
            &source.policy
        )
        .is_none());
    }

    #[test]
    fn corrupt_and_symlinked_recordings_are_reported_not_loaded() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("repo/.dicta");
        let branch = storage.join("branches/main");
        write_metadata(&branch, "valid", json!({ "id": "valid", "video_path": "" }));
        let day = branch.join("recordings/2026-08-20");
        fs::write(
            day.join("valid.transcript.json"),
            json!({ "segments": [] }).to_string(),
        )
        .unwrap();
        fs::write(day.join("corrupt.json"), "{").unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, branch.join("recordings/symlink-day")).unwrap();

        let report = load_recordings(&confined_source(branch, &storage)).unwrap();
        assert_eq!(report.recordings.len(), 1);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("corrupt.json")));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("symlink-day")));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.contains("transcript.json")));
    }

    #[test]
    fn trusted_legacy_project_metadata_may_use_absolute_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("PromptReel/project");
        let transcript = root.path().join("legacy.md");
        fs::write(&transcript, "trusted legacy transcript").unwrap();
        write_metadata(
            &storage,
            "legacy",
            json!({ "id": "legacy", "transcript_path": transcript, "video_path": "" }),
        );
        let source = RecordingSource {
            path: storage,
            policy: ArtifactPolicy::TrustedLegacyProject,
        };
        assert_eq!(
            load_recordings(&source).unwrap().recordings[0]
                .transcript
                .as_deref(),
            Some("trusted legacy transcript")
        );
    }

    #[test]
    fn trusted_legacy_unprojected_metadata_may_use_absolute_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("Dicta/unprojected");
        let transcript = root.path().join("legacy-general.md");
        let video = root.path().join("legacy-general.mp4");
        fs::write(&transcript, "trusted legacy General transcript").unwrap();
        fs::write(&video, "trusted legacy General video").unwrap();
        write_metadata(
            &storage,
            "legacy-general",
            json!({ "id": "legacy-general", "transcript_path": transcript, "video_path": video }),
        );
        let source = RecordingSource {
            path: storage,
            policy: ArtifactPolicy::TrustedLegacyUnprojected,
        };
        let recording = &load_recordings(&source).unwrap().recordings[0];
        assert_eq!(
            recording.transcript.as_deref(),
            Some("trusted legacy General transcript")
        );
        assert_eq!(recording.video_path, video.to_string_lossy());
    }

    #[test]
    fn trusted_legacy_symlinked_storage_is_ignored() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let actual = root.path().join("actual");
        fs::create_dir_all(&actual).unwrap();
        let linked = root.path().join("linked");
        symlink(&actual, &linked).unwrap();
        let source = RecordingSource {
            path: linked,
            policy: ArtifactPolicy::TrustedLegacyUnprojected,
        };
        let report = load_recordings(&source).unwrap();
        assert!(report.recordings.is_empty());
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn confined_general_rejects_symlinked_root_day_and_file() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let actual = root.path().join("actual-general");
        fs::create_dir_all(&actual).unwrap();
        let linked = root.path().join("linked-general");
        symlink(&actual, &linked).unwrap();
        let linked_source = RecordingSource {
            path: linked.clone(),
            policy: ArtifactPolicy::ConfinedGeneral { root: linked },
        };
        assert!(load_recordings(&linked_source).is_err());

        let general = root.path().join("General");
        let day = general.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        let outside_day = root.path().join("outside-day");
        fs::create_dir_all(&outside_day).unwrap();
        symlink(&outside_day, general.join("recordings/symlink-day")).unwrap();
        let outside_file = root.path().join("outside.json");
        fs::write(&outside_file, json!({ "id": "outside" }).to_string()).unwrap();
        symlink(&outside_file, day.join("symlink-file.json")).unwrap();
        let source = RecordingSource {
            path: general.clone(),
            policy: ArtifactPolicy::ConfinedGeneral { root: general },
        };
        let report = load_recordings(&source).unwrap();
        assert!(report.recordings.is_empty());
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("symlink-day")));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("symlink-file")));
    }
}
