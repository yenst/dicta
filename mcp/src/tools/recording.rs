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
