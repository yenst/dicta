//! Explicit conversions between disk models and control-protocol documents.

use dicta_control::{
    RecordingDocument, SettingsDocument, TimelineNoteDocument, TranscriptSegmentDocument,
};
use dicta_core::{storage::AppSettings, RecordingFile, TimelineNote, TranscriptSegment};

pub(crate) fn settings_document(settings: AppSettings) -> SettingsDocument {
    SettingsDocument {
        shortcut_id: settings.shortcut_id,
        cleanup_merged_videos: settings.cleanup_merged_videos,
        branch_locking: settings.branch_locking,
        transcription_language: settings.transcription_language,
        general_path: settings.general_path,
        extra: settings.extra,
    }
}

pub(crate) fn recording_document(recording: RecordingFile) -> RecordingDocument {
    RecordingDocument {
        id: recording.id.into_string(),
        project_id: recording.project_id.into_string(),
        video_path: recording.video_path,
        metadata_path: recording.metadata_path,
        note: recording.note,
        recording_scope: recording.recording_scope.to_string(),
        git_branch: recording.git_branch,
        started_at: recording.started_at,
        ended_at: recording.ended_at,
        duration_seconds: recording.duration_seconds,
        size_bytes: recording.size_bytes,
        success: recording.success,
        transcript: recording.transcript,
        transcript_path: recording.transcript_path,
        transcript_segments: recording
            .transcript_segments
            .into_iter()
            .map(transcript_segment_document)
            .collect(),
        transcription_status: recording.transcription_status.to_string(),
        transcription_error: recording.transcription_error,
        transcription_language: recording.transcription_language,
        poster_path: recording.poster_path,
        annotation_path: recording.annotation_path,
        timeline_notes: recording
            .timeline_notes
            .into_iter()
            .map(timeline_note_document)
            .collect(),
        extra: recording.extra,
    }
}

pub(super) fn timeline_notes_from_wire(notes: Vec<TimelineNoteDocument>) -> Vec<TimelineNote> {
    notes.into_iter().map(timeline_note_from_document).collect()
}

fn transcript_segment_document(segment: TranscriptSegment) -> TranscriptSegmentDocument {
    TranscriptSegmentDocument {
        start_seconds: segment.start_seconds,
        end_seconds: segment.end_seconds,
        text: segment.text,
    }
}

fn timeline_note_document(note: TimelineNote) -> TimelineNoteDocument {
    TimelineNoteDocument {
        id: note.id,
        timestamp_seconds: note.timestamp_seconds,
        text: note.text,
        created_at: note.created_at,
        source: note.source,
        extra: note.extra,
    }
}

fn timeline_note_from_document(note: TimelineNoteDocument) -> TimelineNote {
    TimelineNote {
        id: note.id,
        timestamp_seconds: note.timestamp_seconds,
        text: note.text,
        created_at: note.created_at,
        source: note.source,
        extra: note.extra,
    }
}
