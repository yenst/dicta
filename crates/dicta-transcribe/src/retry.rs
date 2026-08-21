use dicta_core::{RecordingFile, TranscriptionStatus};

/// Selects legacy pending/failed recordings that can be retried after startup.
/// The order is deterministic: pending before failed, then oldest first.
#[must_use]
pub fn retry_candidates(recordings: impl IntoIterator<Item = RecordingFile>) -> Vec<RecordingFile> {
    let mut candidates = recordings
        .into_iter()
        .filter(|recording| {
            recording.success
                && !recording.video_path.trim().is_empty()
                && matches!(
                    recording.transcription_status,
                    TranscriptionStatus::Pending | TranscriptionStatus::Failed
                )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        retry_rank(&left.transcription_status)
            .cmp(&retry_rank(&right.transcription_status))
            .then_with(|| left.started_at.cmp(&right.started_at))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    candidates
}

fn retry_rank(status: &TranscriptionStatus) -> u8 {
    match status {
        TranscriptionStatus::Pending => 0,
        TranscriptionStatus::Failed => 1,
        TranscriptionStatus::Processing
        | TranscriptionStatus::Complete
        | TranscriptionStatus::Unknown(_) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicta_core::{ProjectId, RecordingId, RecordingScope};

    fn recording(id: &str, status: TranscriptionStatus) -> RecordingFile {
        RecordingFile {
            id: RecordingId::new(id).unwrap(),
            project_id: ProjectId::new("__unprojected__").unwrap(),
            video_path: format!("/recordings/{id}.mp4"),
            metadata_path: String::new(),
            note: String::new(),
            recording_scope: RecordingScope::Unprojected,
            git_branch: None,
            started_at: None,
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: status,
            transcription_error: None,
            transcription_language: None,
            poster_path: None,
            annotation_path: None,
            timeline_notes: Vec::new(),
            extra: Default::default(),
        }
    }

    #[test]
    fn retry_discovery_is_filtered_and_pending_first() {
        let mut complete = recording("complete", TranscriptionStatus::Complete);
        complete.transcript = Some("done".to_owned());
        let mut missing_video = recording("missing-video", TranscriptionStatus::Pending);
        missing_video.video_path.clear();
        let candidates = retry_candidates([
            recording("failed", TranscriptionStatus::Failed),
            complete,
            recording("pending", TranscriptionStatus::Pending),
            missing_video,
        ]);
        assert_eq!(
            candidates
                .iter()
                .map(|recording| recording.id.as_str())
                .collect::<Vec<_>>(),
            ["pending", "failed"]
        );
    }
}
