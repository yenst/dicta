use crate::{
    context, render,
    storage::{self, ArtifactPolicy, Recording, RecordingSource},
};
use dicta_core::{storage as core_storage, ProjectFile, ProjectId, GENERAL_PROJECT_ID};
use serde::Serialize;
use std::{collections::HashSet, env, fs, path::PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelProject {
    id: String,
    name: String,
    path: String,
    branch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelContext {
    id: String,
    title: String,
    started_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelState {
    projects: Vec<PanelProject>,
    selected_project_id: Option<String>,
    contexts: Vec<PanelContext>,
    error: Option<String>,
}

pub fn run(args: Vec<String>) -> Result<bool, String> {
    if args.first().map(String::as_str) != Some("omarchy") {
        return Ok(false);
    }
    match args.get(1).map(String::as_str) {
        Some("state") => {
            let selected = parse_selected_project(&args[2..])?;
            let state = panel_state(selected.as_deref())?;
            println!(
                "{}",
                serde_json::to_string(&state)
                    .map_err(|error| format!("Could not serialize Omarchy state: {error}"))?
            );
        }
        Some("context") => {
            let project_id = args.get(2).ok_or_else(|| {
                "Usage: dicta-mcp omarchy context <project-id> <recording-id>".to_string()
            })?;
            let recording_id = args.get(3).ok_or_else(|| {
                "Usage: dicta-mcp omarchy context <project-id> <recording-id>".to_string()
            })?;
            if args.len() != 4 {
                return Err(
                    "Usage: dicta-mcp omarchy context <project-id> <recording-id>".to_string(),
                );
            }
            print!("{}", recording_context(project_id, recording_id)?);
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            println!(
                "Dicta Omarchy integration\n\n  dicta-mcp omarchy state [--project <id>]\n  dicta-mcp omarchy context <project-id> <recording-id>"
            );
        }
        Some(command) => return Err(format!("Unknown Dicta Omarchy command: {command}")),
    }
    Ok(true)
}

fn parse_selected_project(args: &[String]) -> Result<Option<String>, String> {
    if args.is_empty() {
        return Ok(None);
    }
    if args.len() == 2 && args[0] == "--project" {
        ProjectId::new(args[1].clone()).map_err(|_| "Invalid project identifier".to_string())?;
        return Ok(Some(args[1].clone()));
    }
    Err("Usage: dicta-mcp omarchy state [--project <id>]".to_string())
}

fn storage_root() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("DICTA_HOME") {
        return Ok(PathBuf::from(path));
    }
    let documents =
        dirs::document_dir().ok_or_else(|| "Could not locate the Documents folder".to_string())?;
    Ok(core_storage::preferred_storage_root(&documents))
}

fn registered_projects(root: &std::path::Path) -> Vec<ProjectFile> {
    let mut projects = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return projects;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path().join("project.json");
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        if let Ok(project) = serde_json::from_str::<ProjectFile>(&content) {
            projects.push(project);
        }
    }
    projects.sort_by_key(|project| std::cmp::Reverse(project.created_at));
    projects
}

fn general_project() -> ProjectFile {
    ProjectFile {
        id: ProjectId::new(GENERAL_PROJECT_ID).expect("General project ID is valid"),
        name: "General".to_string(),
        created_at: std::time::UNIX_EPOCH.into(),
        source_path: None,
    }
}

fn all_projects(root: &std::path::Path) -> Vec<ProjectFile> {
    let mut projects = registered_projects(root);
    projects.push(general_project());
    projects
}

fn project_branch(project: &ProjectFile) -> String {
    if project.id.as_str() == GENERAL_PROJECT_ID {
        return "General".to_string();
    }
    match project.source_path.as_deref() {
        Some(source) => dicta_core::git::branch(std::path::Path::new(source))
            .unwrap_or_else(|_| "Git unavailable".to_string()),
        None => "Repository-wide".to_string(),
    }
}

fn panel_project(project: &ProjectFile) -> PanelProject {
    PanelProject {
        id: project.id.to_string(),
        name: project.name.clone(),
        path: project
            .source_path
            .clone()
            .unwrap_or_else(|| project.id.to_string()),
        branch: project_branch(project),
    }
}

fn panel_state(selected_project_id: Option<&str>) -> Result<PanelState, String> {
    panel_state_at(&storage_root()?, selected_project_id)
}

fn panel_state_at(
    root: &std::path::Path,
    selected_project_id: Option<&str>,
) -> Result<PanelState, String> {
    let projects = all_projects(root);
    let selected = selected_project_id
        .and_then(|id| projects.iter().find(|project| project.id.as_str() == id))
        .or_else(|| {
            projects
                .iter()
                .find(|project| project.id.as_str() != GENERAL_PROJECT_ID)
        })
        .or_else(|| projects.first());
    let (contexts, error) = match selected {
        Some(project) => match project_recordings(root, project) {
            Ok(recordings) => (
                recordings
                    .into_iter()
                    .take(3)
                    .map(|recording| PanelContext {
                        id: recording.id.to_string(),
                        title: render::display_note(&recording).to_string(),
                        started_at: recording.started_at.map(|value| value.to_rfc3339()),
                    })
                    .collect(),
                None,
            ),
            Err(error) => (Vec::new(), Some(error)),
        },
        None => (Vec::new(), None),
    };
    Ok(PanelState {
        projects: projects.iter().map(panel_project).collect(),
        selected_project_id: selected.map(|project| project.id.to_string()),
        contexts,
        error,
    })
}

fn general_sources(root: &std::path::Path) -> Vec<RecordingSource> {
    let settings = fs::read_to_string(root.join("settings.json"))
        .ok()
        .and_then(|content| serde_json::from_str::<core_storage::GeneralSettings>(&content).ok())
        .unwrap_or_default();
    let legacy = root.join("unprojected");
    core_storage::general_storage_candidates(root, settings.general_path.as_deref())
        .into_iter()
        .map(|path| {
            let policy = if path == legacy {
                ArtifactPolicy::LegacyUnprojected { root: path.clone() }
            } else {
                ArtifactPolicy::ConfinedGeneral { root: path.clone() }
            };
            RecordingSource { path, policy }
        })
        .collect()
}

fn project_recordings(
    root: &std::path::Path,
    project: &ProjectFile,
) -> Result<Vec<Recording>, String> {
    let mut recordings = if project.id.as_str() == GENERAL_PROJECT_ID {
        let mut recordings = Vec::new();
        for source in general_sources(root) {
            recordings.extend(storage::load_recordings(&source)?.recordings);
        }
        recordings
    } else if let Some(source) = project.source_path.as_deref() {
        let resolved = context::resolve(Some(source), Some("current"))?;
        context::load(&resolved)?.recordings
    } else {
        storage::load_recordings(&RecordingSource {
            path: root.join(project.id.as_str()),
            policy: ArtifactPolicy::LegacyProject {
                root: root.join(project.id.as_str()),
            },
        })?
        .recordings
    };
    recordings.sort_by_key(|recording| std::cmp::Reverse(recording.started_at));
    let mut seen = HashSet::new();
    recordings.retain(|recording| seen.insert(recording.id.clone()));
    Ok(recordings)
}

fn recording_context(project_id: &str, recording_id: &str) -> Result<String, String> {
    recording_context_at(&storage_root()?, project_id, recording_id)
}

fn recording_context_at(
    root: &std::path::Path,
    project_id: &str,
    recording_id: &str,
) -> Result<String, String> {
    ProjectId::new(project_id.to_string()).map_err(|_| "Invalid project identifier".to_string())?;
    let project = all_projects(root)
        .into_iter()
        .find(|project| project.id.as_str() == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    let recording = project_recordings(root, &project)?
        .into_iter()
        .find(|recording| recording.id.as_str() == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    let scope = match recording
        .metadata
        .get("recording_scope")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("branch")
    {
        "repository" => "the repository (all branches)".to_string(),
        "unprojected" => "the unprojected Dicta library".to_string(),
        _ => recording
            .metadata
            .get("git_branch")
            .and_then(serde_json::Value::as_str)
            .filter(|branch| !branch.is_empty())
            .map(|branch| format!("branch `{branch}`"))
            .unwrap_or_else(|| "the recorded branch".to_string()),
    };
    Ok(format!(
        "Within Dicta project `{}`, look at recording `{}` from {}. Use its transcript as primary guidance and inspect timestamped frames when visual evidence matters.",
        project.name, recording.id, scope
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("demo");
        let day = project.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            project.join("project.json"),
            serde_json::to_string(&ProjectFile {
                id: ProjectId::new("demo").unwrap(),
                name: "Demo".to_string(),
                created_at: "2026-08-20T08:00:00Z".parse().unwrap(),
                source_path: None,
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            day.join("recording-1.json"),
            json!({
                "id": "recording-1",
                "note": "Panel interaction",
                "started_at": "2026-08-20T09:00:00Z",
                "recording_scope": "repository"
            })
            .to_string(),
        )
        .unwrap();
        root
    }

    #[test]
    fn state_lists_projects_and_recent_contexts() {
        let root = fixture();
        let state = panel_state_at(root.path(), Some("demo")).unwrap();
        assert_eq!(state.selected_project_id.as_deref(), Some("demo"));
        assert_eq!(state.projects[0].name, "Demo");
        assert_eq!(state.contexts[0].title, "Panel interaction");
    }

    #[test]
    fn recording_context_matches_the_desktop_copy_action() {
        let root = fixture();
        let context = recording_context_at(root.path(), "demo", "recording-1").unwrap();
        assert_eq!(
            context,
            "Within Dicta project `Demo`, look at recording `recording-1` from the repository (all branches). Use its transcript as primary guidance and inspect timestamped frames when visual evidence matters."
        );
    }
}
