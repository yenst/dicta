//! Shared confined catalog discovery for native storage, the CLI, and MCP.

use crate::{
    git,
    storage::{self, read_json},
    ProjectFile, ProjectId, RecordingFile, GENERAL_PROJECT_ID,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[must_use]
pub fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSource {
    pub path: PathBuf,
    pub project_id: ProjectId,
    pub project_name: String,
    pub include_branches: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CatalogReport {
    pub recordings: Vec<RecordingFile>,
    pub warnings: Vec<String>,
}

#[must_use]
pub fn load_general_settings(root: &Path) -> storage::GeneralSettings {
    let path = root.join("settings.json");
    if is_symlink(&path) {
        return storage::GeneralSettings::default();
    }
    read_json(&path).unwrap_or_default()
}

#[must_use]
pub fn registered_sources(root: &Path) -> Vec<CatalogSource> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return None;
            }
            let metadata_path = entry.path().join("project.json");
            if is_symlink(&metadata_path) {
                return None;
            }
            let project = read_json::<ProjectFile>(&metadata_path).ok()?;
            if project.id.as_str() == GENERAL_PROJECT_ID || project.name.trim().is_empty() {
                return None;
            }
            if project.id.as_str() != entry.file_name().to_string_lossy().as_ref() {
                return None;
            }
            let path = project
                .source_path
                .as_deref()
                .map(|source| PathBuf::from(source).join(".dicta"))
                .unwrap_or_else(|| entry.path());
            Some(CatalogSource {
                path,
                project_id: project.id,
                project_name: project.name,
                include_branches: project.source_path.is_some(),
            })
        })
        .collect()
}

#[must_use]
pub fn repository_local_sources(working_directory: &Path) -> Vec<CatalogSource> {
    let Ok(repo_root) = git::root(working_directory) else {
        return Vec::new();
    };
    let storage_path = repo_root.join(".dicta");
    let project_path = storage_path.join("project.json");
    if is_symlink(&storage_path) || is_symlink(&project_path) {
        return Vec::new();
    }
    let Ok(project) = read_json::<ProjectFile>(&project_path) else {
        return Vec::new();
    };
    vec![CatalogSource {
        path: storage_path,
        project_id: project.id,
        project_name: project.name,
        include_branches: true,
    }]
}

#[must_use]
pub fn general_sources(root: &Path) -> Vec<CatalogSource> {
    let settings = load_general_settings(root);
    let Ok(project_id) = ProjectId::new(GENERAL_PROJECT_ID) else {
        return Vec::new();
    };
    storage::general_storage_candidates(root, settings.general_path.as_deref())
        .into_iter()
        .map(|path| CatalogSource {
            path,
            project_id: project_id.clone(),
            project_name: "General".to_owned(),
            include_branches: false,
        })
        .collect()
}

pub fn deduplicate_sources(sources: &mut Vec<CatalogSource>) {
    let mut seen = HashSet::new();
    sources.retain(|source| {
        let path = source
            .path
            .canonicalize()
            .unwrap_or_else(|_| source.path.clone());
        seen.insert((path, source.project_id.clone()))
    });
}

#[must_use]
pub fn recording_trees(source: &CatalogSource) -> Vec<PathBuf> {
    if is_symlink(&source.path) {
        return Vec::new();
    }
    let mut trees = vec![source.path.clone()];
    if !source.include_branches {
        return trees;
    }
    let Ok(branches) = fs::read_dir(source.path.join("branches")) else {
        return trees;
    };
    trees.extend(branches.flatten().filter_map(|entry| {
        let file_type = entry.file_type().ok()?;
        (file_type.is_dir() && !file_type.is_symlink()).then(|| entry.path())
    }));
    trees
}

pub fn scan_recording_tree(
    source: &CatalogSource,
    tree: &Path,
    warnings: &mut Vec<String>,
) -> Vec<RecordingFile> {
    scan_tree_filtered(tree, Some(&source.project_id), warnings)
}

#[must_use]
pub fn load_recordings(sources: &[CatalogSource]) -> CatalogReport {
    let mut report = CatalogReport::default();
    for source in sources {
        for tree in recording_trees(source) {
            report
                .recordings
                .extend(scan_recording_tree(source, &tree, &mut report.warnings));
        }
    }
    report.recordings.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.id.as_str().cmp(left.id.as_str()))
    });
    let mut seen = HashSet::new();
    report.recordings.retain(|recording| {
        seen.insert((
            recording.project_id.clone(),
            recording.id.clone(),
            recording.metadata_path.clone(),
        ))
    });
    report
}

/// Loads recording metadata from one tree without requiring a project match.
/// Callers that confine artifacts themselves still get symlink-safe discovery.
#[must_use]
pub fn scan_tree(tree: &Path, warnings: &mut Vec<String>) -> Vec<RecordingFile> {
    scan_tree_filtered(tree, None, warnings)
}

/// Discovers recording metadata files under `tree` without parsing them.
///
/// The walk ignores symlinked storage, days, and artifacts and reports those
/// skips through `warnings`. Callers apply their own parse and confinement
/// policy to the returned paths.
#[must_use]
pub fn walk_recording_metadata(tree: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    if is_symlink(tree) {
        warnings.push(format!("ignored symlinked storage `{}`", tree.display()));
        return Vec::new();
    }
    let recordings_root = tree.join("recordings");
    if is_symlink(&recordings_root) {
        warnings.push(format!(
            "ignored symlinked recordings storage `{}`",
            recordings_root.display()
        ));
        return Vec::new();
    }
    let Ok(days) = fs::read_dir(&recordings_root) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for day in days.flatten() {
        let Ok(file_type) = day.file_type() else {
            warnings.push(format!("could not inspect `{}`", day.path().display()));
            continue;
        };
        if file_type.is_symlink() {
            warnings.push(format!(
                "ignored symlinked recording day `{}`",
                day.path().display()
            ));
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(day.path()) else {
            warnings.push(format!("could not read `{}`", day.path().display()));
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                warnings.push(format!("could not inspect `{}`", path.display()));
                continue;
            };
            if file_type.is_symlink() {
                warnings.push(format!("ignored symlinked artifact `{}`", path.display()));
                continue;
            }
            let is_transcript = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".transcript.json"));
            if !file_type.is_file()
                || is_transcript
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            paths.push(path);
        }
    }
    paths
}

fn scan_tree_filtered(
    tree: &Path,
    project_id: Option<&ProjectId>,
    warnings: &mut Vec<String>,
) -> Vec<RecordingFile> {
    let paths = walk_recording_metadata(tree, warnings);
    if paths.is_empty() {
        return Vec::new();
    }
    let Ok(canonical_root) = tree.join("recordings").canonicalize() else {
        return Vec::new();
    };
    paths
        .into_iter()
        .filter_map(
            |path| match read_catalog_recording(&path, &canonical_root, project_id) {
                Ok(recording) => Some(recording),
                Err(error) => {
                    warnings.push(format!("ignored `{}`: {error}", path.display()));
                    None
                }
            },
        )
        .collect()
}

fn read_catalog_recording(
    path: &Path,
    recordings_root: &Path,
    project_id: Option<&ProjectId>,
) -> Result<RecordingFile, String> {
    let mut recording = read_json::<RecordingFile>(path)?;
    if !recording.is_valid() {
        return Err("recording metadata failed validation".to_owned());
    }
    if let Some(project_id) = project_id {
        if recording.project_id != *project_id {
            return Err(format!(
                "recording belongs to project `{}` instead of `{project_id}`",
                recording.project_id
            ));
        }
    }
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("could not resolve metadata: {error}"))?;
    if !canonical_path.starts_with(recordings_root) {
        return Err("metadata escaped the recordings root".to_owned());
    }
    recording.metadata_path = canonical_path.to_string_lossy().into_owned();
    attach_transcript(&mut recording, &canonical_path, recordings_root);
    Ok(recording)
}

fn attach_transcript(recording: &mut RecordingFile, metadata: &Path, recordings_root: &Path) {
    if recording.transcript.is_some() {
        return;
    }
    let mut candidates = recording
        .transcript_path
        .as_deref()
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                metadata.parent().unwrap_or(recordings_root).join(path)
            }
        })
        .into_iter()
        .collect::<Vec<_>>();
    let Some(stem) = metadata.file_stem().and_then(|value| value.to_str()) else {
        return;
    };
    candidates.push(metadata.with_file_name(format!("{stem}.transcript.md")));
    candidates.push(metadata.with_file_name(format!("{stem}.md")));
    for candidate in candidates {
        let Ok(file_type) = fs::symlink_metadata(&candidate).map(|metadata| metadata.file_type())
        else {
            continue;
        };
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(recordings_root) {
            continue;
        }
        let Ok(transcript) = fs::read_to_string(&canonical) else {
            continue;
        };
        recording.transcript_path = Some(canonical.to_string_lossy().into_owned());
        recording.transcript = Some(transcript);
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::write_json_atomic;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dicta-core-catalog-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn load_general_settings_ignores_symlinked_settings() {
        let root = unique_root();
        fs::create_dir_all(&root).unwrap();
        let target = root.join("outside.json");
        fs::write(&target, r#"{"general_path":"/tmp/escaped"}"#).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, root.join("settings.json")).unwrap();
        let settings = load_general_settings(&root);
        assert_eq!(settings.general_path, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_recordings_skips_escaped_and_mismatched_metadata() {
        let root = unique_root();
        let day = root.join("General/recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        write_json_atomic(
            &day.join("good.json"),
            &json!({
                "id": "20260820-12-00-00",
                "project_id": GENERAL_PROJECT_ID,
                "note": "ok",
                "success": true
            }),
        )
        .unwrap();
        write_json_atomic(
            &day.join("wrong-project.json"),
            &json!({
                "id": "20260820-13-00-00",
                "project_id": "other",
                "success": true
            }),
        )
        .unwrap();
        let sources = general_sources(&root);
        let report = load_recordings(&sources);
        assert_eq!(report.recordings.len(), 1);
        assert_eq!(report.recordings[0].id.as_str(), "20260820-12-00-00");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("wrong-project")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_tree_keeps_mismatched_project_metadata_for_confined_callers() {
        let root = unique_root();
        let day = root.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        write_json_atomic(
            &day.join("other.json"),
            &json!({
                "id": "20260820-13-00-00",
                "project_id": "other",
                "success": true
            }),
        )
        .unwrap();
        let mut warnings = Vec::new();
        let recordings = scan_tree(&root, &mut warnings);
        assert_eq!(recordings.len(), 1);
        assert_eq!(recordings[0].project_id.as_str(), "other");
        assert!(warnings.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
