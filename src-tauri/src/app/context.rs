use crate::*;

#[tauri::command]
pub(crate) fn reveal_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    platform::shell::reveal(&target)
}

#[tauri::command]
pub(crate) fn build_context(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
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
            recording.id,
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
pub(crate) fn build_recording_context(
    project_id: String,
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let project = project_view(&state.root, read_project(&state.root, &project_id)?);
    let recording = load_recordings(&state.root, &project_id)?
        .into_iter()
        .find(|recording| recording.id.as_str() == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    let scope = match recording.recording_scope {
        RecordingScope::Branch => recording
            .git_branch
            .as_deref()
            .map(|branch| format!("branch `{branch}`"))
            .unwrap_or_else(|| "the recorded branch".to_string()),
        RecordingScope::Repository => "the repository (all branches)".to_string(),
        _ => "the unprojected Dicta library".to_string(),
    };
    Ok(format!(
        "Within Dicta project `{}`, look at recording `{}` from {}. Use its transcript as primary guidance and inspect timestamped frames when visual evidence matters.",
        project.name, recording.id, scope
    ))
}

#[tauri::command]
pub(crate) fn copy_to_clipboard(text: String) -> Result<(), String> {
    platform::shell::copy_to_clipboard(&text)
}
