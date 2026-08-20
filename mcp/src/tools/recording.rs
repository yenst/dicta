use crate::{context, render};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordingArgs {
    repo_path: Option<String>,
    branch: Option<String>,
    recording_id: String,
}

pub(super) fn get(args: RecordingArgs) -> Result<(String, Value), String> {
    if args.recording_id.trim().is_empty() {
        return Err("recording_id is required".to_string());
    }
    let context = context::resolve(args.repo_path.as_deref(), args.branch.as_deref())?;
    let found = context::find(&context, args.recording_id.trim())?;
    let recording = found.recording;
    let mut text = format!(
        "# {}\n\nRecording ID: `{}`\nProject: `{}`\nScope: `{}`\n",
        render::display_note(&recording),
        recording.id,
        found.project_name,
        found.scope_label
    );
    if let Some(started_at) = recording.started_at {
        text.push_str(&format!("Recorded: `{started_at}`\n"));
    }
    if let Some(duration) = recording.duration_seconds {
        text.push_str(&format!("Duration: `{duration:.1}s`\n"));
    }
    text.push_str(&format!(
        "Video: `{}`\nMetadata: `{}`\n",
        recording.video_path, recording.metadata_path
    ));
    render::append_timeline_notes(&mut text, &recording, false);
    render::append_transcript(&mut text, &recording);
    Ok((text, render::recording_json(&recording)))
}

pub(super) fn context(args: RecordingArgs) -> Result<(String, Value), String> {
    if args.recording_id.trim().is_empty() {
        return Err("recording_id is required".to_string());
    }
    let context = context::resolve(args.repo_path.as_deref(), args.branch.as_deref())?;
    let found = context::find(&context, args.recording_id.trim())?;
    let guidance = format!(
        "Within Dicta project `{}`, look at recording `{}` from {}. Use its transcript as primary guidance and inspect timestamped frames when visual evidence matters.",
        found.project_name, found.recording.id, found.scope_label
    );
    Ok((
        guidance.clone(),
        serde_json::json!({
            "context": guidance,
            "project": found.project_name,
            "scope": found.scope_label,
            "recording": render::recording_json(&found.recording)
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicta_core::{storage, ProjectFile, ProjectId};
    use std::{fs, path::Path, process::Command};

    fn init_git(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn recording_context_reads_repository_fixture_without_a_daemon() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        init_git(&repo);
        let local = repo.join(".dicta");
        storage::write_json_atomic(
            &local.join("project.json"),
            &ProjectFile {
                id: ProjectId::new("demo").unwrap(),
                name: "Demo".to_owned(),
                created_at: std::time::UNIX_EPOCH.into(),
                source_path: Some(repo.to_string_lossy().into_owned()),
            },
        )
        .unwrap();
        let day = local.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("take-one.json"),
            serde_json::json!({
                "id": "take-one",
                "note": "Use the narrow layout",
                "recording_scope": "repository",
                "transcript": "Keep the native surface small."
            })
            .to_string(),
        )
        .unwrap();

        let (text, structured) = context(RecordingArgs {
            repo_path: Some(repo.to_string_lossy().into_owned()),
            branch: Some("main".to_owned()),
            recording_id: "take-one".to_owned(),
        })
        .unwrap();

        assert!(text.contains("Within Dicta project `Demo`"));
        assert!(text.contains("repository-wide"));
        assert_eq!(structured["recording"]["id"], "take-one");
        assert_eq!(
            structured["recording"]["transcript"],
            "Keep the native surface small."
        );
    }
}
