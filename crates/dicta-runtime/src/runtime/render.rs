//! Snapshot and catalog rendering helpers for [`super::Runtime`].

use super::recording_id_from_state;
use crate::{
    error::RuntimeError,
    ports::{PortError, PortErrorKind},
};
use dicta_control::{
    protocol::{AppPhase, StatusSnapshot, TranscriptionState},
    AnnotationTool, ModelInstallStage, ModelState, ModelStatusSummary, ProjectSummary,
    RecordingSelector, RecordingSummary,
};
use dicta_core::{
    ProjectFile, ProjectId, RecordingFile, RecordingId, TimelineNote, TranscriptionStatus,
};
use dicta_engine::{AppSnapshot, AppState};
use dicta_transcribe::{ModelFileState, ModelPreparationStage, ModelStatus};
use std::fmt::Write as _;
use std::path::Path;

pub(super) fn status_from_snapshot(snapshot: &AppSnapshot, tool: AnnotationTool) -> StatusSnapshot {
    let annotations_enabled = matches!(snapshot.state, AppState::Annotating(_));
    let phase = match snapshot.state {
        AppState::Idle => AppPhase::Idle,
        AppState::Preparing(_) => AppPhase::Preparing,
        AppState::Recording(_) | AppState::Annotating(_) => AppPhase::Recording,
        AppState::Stopping(_) => AppPhase::Stopping,
        AppState::Transcribing { .. } => AppPhase::Transcribing,
        AppState::Failed(_) => AppPhase::Failed,
    };
    StatusSnapshot {
        phase,
        project: snapshot.selected_project.as_ref().map(ToString::to_string),
        recording_id: recording_id_from_state(&snapshot.state).map(ToString::to_string),
        annotations_enabled,
        annotation_tool: annotations_enabled.then_some(tool),
    }
}

pub(super) fn project_summary(
    project: &ProjectFile,
    selected: Option<&ProjectId>,
) -> ProjectSummary {
    let branch = project
        .source_path
        .as_deref()
        .and_then(|path| dicta_core::git::branch(Path::new(path)).ok());
    ProjectSummary {
        id: project.id.to_string(),
        name: project.name.clone(),
        path: project.source_path.clone(),
        branch,
        selected: selected == Some(&project.id),
    }
}

pub(super) fn model_status_summary(status: ModelStatus) -> ModelStatusSummary {
    let install_stage = status
        .install_progress
        .as_ref()
        .map(|progress| match progress.stage {
            ModelPreparationStage::Locating => ModelInstallStage::Locating,
            ModelPreparationStage::Downloading => ModelInstallStage::Downloading,
            ModelPreparationStage::Verifying => ModelInstallStage::Verifying,
            ModelPreparationStage::Ready => ModelInstallStage::Ready,
        });
    let quality_state = if install_stage.is_some_and(|stage| stage != ModelInstallStage::Ready) {
        ModelState::Installing
    } else {
        match status.quality.state {
            ModelFileState::Missing => ModelState::Missing,
            ModelFileState::Partial => ModelState::Partial,
            ModelFileState::Ready => ModelState::Ready,
            ModelFileState::Invalid => ModelState::Invalid,
            ModelFileState::Unverified => ModelState::Unverified,
        }
    };
    let downloaded_bytes = status
        .install_progress
        .as_ref()
        .map(|progress| progress.completed_bytes);
    let message = status.install_progress.as_ref().map_or_else(
        || status.quality.detail.clone(),
        |progress| progress.message.clone(),
    );
    ModelStatusSummary {
        active_model: status
            .active_model
            .as_ref()
            .map(|model| model.kind.label().to_owned()),
        active_model_path: status
            .active_model
            .map(|model| model.path.to_string_lossy().into_owned()),
        quality_state,
        quality_path: status.quality.path.to_string_lossy().into_owned(),
        quality_size_bytes: status.quality.size_bytes,
        expected_download_bytes: status.quality.expected_download_bytes,
        install_stage,
        downloaded_bytes,
        message,
        last_error: status.install_error,
    }
}

pub(super) fn recording_summary(recording: &RecordingFile) -> RecordingSummary {
    RecordingSummary {
        id: recording.id.to_string(),
        project: Some(recording.project_id.to_string()),
        branch: recording.git_branch.clone(),
        started_at: recording.started_at.map(|value| value.to_rfc3339()),
        note: recording.note.clone(),
        transcript_preview: recording
            .transcript
            .as_deref()
            .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(180).collect()),
        success: recording.success,
        recording_scope: recording.recording_scope.to_string(),
        timeline_note_count: u32::try_from(recording.timeline_notes.len()).unwrap_or(u32::MAX),
        has_annotations: recording.annotation_path.is_some(),
        duration_seconds: recording.duration_seconds.unwrap_or(0.0),
        transcription: match recording.transcription_status {
            TranscriptionStatus::Pending => TranscriptionState::Pending,
            TranscriptionStatus::Processing => TranscriptionState::Processing,
            TranscriptionStatus::Complete => TranscriptionState::Complete,
            TranscriptionStatus::Failed => TranscriptionState::Failed,
            TranscriptionStatus::Unknown(_) => TranscriptionState::Unavailable,
        },
    }
}

pub(super) fn resolve_recording_from(
    mut recordings: Vec<RecordingFile>,
    selector: RecordingSelector,
) -> Result<RecordingFile, RuntimeError> {
    match selector {
        RecordingSelector::Latest => {
            sort_recordings_latest_first(&mut recordings);
            recordings.into_iter().next().ok_or_else(|| {
                PortError::new(PortErrorKind::NotFound, "no recordings were found").into()
            })
        }
        RecordingSelector::Id(value) => {
            let recording_id = RecordingId::new(value)
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
            let mut matches = recordings
                .into_iter()
                .filter(|recording| recording.id == recording_id);
            let recording = matches.next().ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("recording {recording_id} was not found"),
                )
            })?;
            if matches.next().is_some() {
                return Err(RuntimeError::DataConflict(format!(
                    "recording ID {recording_id} exists in more than one project"
                )));
            }
            Ok(recording)
        }
    }
}

const CONTEXT_TRANSCRIPT_LIMIT: usize = 1_200;

pub(super) fn transcript_excerpt(transcript: &str) -> String {
    let transcript = transcript.split_whitespace().collect::<Vec<_>>().join(" ");
    if transcript.chars().count() <= CONTEXT_TRANSCRIPT_LIMIT {
        return transcript;
    }
    let mut excerpt = transcript
        .chars()
        .take(CONTEXT_TRANSCRIPT_LIMIT)
        .collect::<String>();
    if let Some(boundary) = excerpt.rfind(char::is_whitespace) {
        excerpt.truncate(boundary);
    }
    excerpt.push_str("…\n\n_(Transcript truncated; open the recording for the full text.)_");
    excerpt
}

pub(super) fn render_recording_context(recording: &RecordingFile, project_name: &str) -> String {
    let mut output = format!(
        "# Dicta recording: {}\n\nProject: {} (`{}`)\n",
        recording.id, project_name, recording.project_id
    );
    if let Some(branch) = recording.git_branch.as_deref() {
        let _ = writeln!(output, "Branch: `{branch}`");
    }
    let _ = writeln!(output, "Scope: {}", recording.recording_scope);
    if !recording.note.trim().is_empty() {
        let _ = writeln!(output, "\n## Note\n\n{}", recording.note.trim());
    }
    if let Some(transcript) = recording
        .transcript
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let excerpt = transcript_excerpt(transcript);
        let _ = writeln!(output, "\n## Transcript excerpt\n\n{excerpt}");
    }
    if !recording.timeline_notes.is_empty() {
        output.push_str("\n## Timeline notes\n");
        for note in &recording.timeline_notes {
            let total_seconds =
                std::time::Duration::try_from_secs_f64(note.timestamp_seconds.max(0.0))
                    .map_or(0, |duration| duration.as_secs());
            let _ = write!(
                output,
                "\n- [{:02}:{:02}] {}",
                total_seconds / 60,
                total_seconds % 60,
                note.text.trim()
            );
        }
        output.push('\n');
    }
    output
}

pub(super) fn validate_general_path(path: Option<String>) -> Result<Option<String>, RuntimeError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    if path.chars().count() > 4096 || path.chars().any(char::is_control) {
        return Err(RuntimeError::InvalidRequest(
            "General storage path is too long or contains control characters".to_owned(),
        ));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(RuntimeError::InvalidRequest(
            "General storage path must be absolute".to_owned(),
        ));
    }
    Ok(Some(path.to_owned()))
}

pub(super) fn validate_timeline_notes(
    recording: &RecordingFile,
    notes: &[TimelineNote],
) -> Result<(), RuntimeError> {
    if notes.len() > 500 {
        return Err(RuntimeError::InvalidRequest(
            "a recording can contain at most 500 timeline notes".to_owned(),
        ));
    }
    let duration = recording.duration_seconds;
    let mut ids = std::collections::HashSet::with_capacity(notes.len());
    for note in notes {
        if !note.is_valid()
            || note.text.chars().count() > 2_000
            || !matches!(note.source.as_str(), "typed" | "voice")
            || !ids.insert(note.id.as_str())
        {
            return Err(RuntimeError::InvalidRequest(
                "one or more timeline notes are invalid".to_owned(),
            ));
        }
        if duration.is_some_and(|duration| note.timestamp_seconds > duration + 0.5) {
            return Err(RuntimeError::InvalidRequest(
                "a timeline note cannot be placed beyond the end of the recording".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(super) fn sort_recordings_latest_first(recordings: &mut [RecordingFile]) {
    recordings.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.id.cmp(&left.id))
            .then_with(|| right.project_id.cmp(&left.project_id))
    });
}
