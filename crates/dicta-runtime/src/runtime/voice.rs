//! Voice-note capture and transcription for [`Runtime`].

use super::{render::validate_timeline_notes, Runtime};
#[cfg(unix)]
use crate::voice_note_directory;
use crate::{
    error::RuntimeError,
    ports::{
        AnnotationPort, CapturePort, Clock, Completion, IdSource, PortError, PortErrorKind,
        StoragePort, TranscriptionPort,
    },
};
use dicta_control::{RecordingSelector, Response, VoiceNoteState, VoiceNoteStatus};
use dicta_core::{RecordingFile, TimelineNote};
use dicta_transcribe::TranscriptionOutput;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct PendingVoiceNote {
    pub(crate) recording: RecordingFile,
    pub(crate) note_id: String,
    pub(crate) timestamp_seconds: f64,
    pub(crate) audio_path: PathBuf,
}

impl Drop for PendingVoiceNote {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.audio_path);
    }
}

impl<C, T, A, S, K, I> Runtime<C, T, A, S, K, I>
where
    C: CapturePort,
    T: TranscriptionPort,
    A: AnnotationPort,
    S: StoragePort,
    K: Clock,
    I: IdSource,
{
    pub(super) fn transcribe_voice_note(
        &mut self,
        selector: RecordingSelector,
        note_id: &str,
        timestamp_seconds: f64,
        audio_path: &str,
    ) -> Result<Response, RuntimeError> {
        self.require_idle("transcribe a voice note")?;
        if self.pending_voice_note.is_some() || self.cancelled_voice_inflight.is_some() {
            return Err(RuntimeError::InvalidRequest(
                "a voice-note job is already active".to_owned(),
            ));
        }
        if !self.transcription.is_available() {
            return Err(PortError::new(
                PortErrorKind::Unavailable,
                "local transcription is unavailable for voice notes",
            )
            .into());
        }
        let note_id = note_id.trim();
        if note_id.is_empty()
            || note_id.len() > 128
            || note_id.chars().any(char::is_control)
            || !timestamp_seconds.is_finite()
            || timestamp_seconds < 0.0
        {
            return Err(RuntimeError::InvalidRequest(
                "voice-note identity or timestamp is invalid".to_owned(),
            ));
        }
        let recording = self.resolve_recording(selector)?;
        if recording
            .duration_seconds
            .is_some_and(|duration| timestamp_seconds > duration + 1.0)
        {
            return Err(RuntimeError::InvalidRequest(
                "voice-note timestamp is outside the recording".to_owned(),
            ));
        }
        let audio_path = validate_voice_note_audio(Path::new(audio_path))?;
        let mut input = recording.clone();
        input.video_path = audio_path.to_string_lossy().into_owned();
        let pending = PendingVoiceNote {
            recording,
            note_id: note_id.to_owned(),
            timestamp_seconds,
            audio_path,
        };
        self.voice_note_status = VoiceNoteStatus {
            state: VoiceNoteState::Processing,
            recording_id: Some(pending.recording.id.to_string()),
            note_id: Some(pending.note_id.clone()),
            message: "Transcribing voice note…".to_owned(),
        };
        match self.transcription.transcribe(&input) {
            Ok(Completion::Ready(output)) => {
                self.pending_voice_note = Some(pending);
                self.complete_voice_note(Ok(output))?;
            }
            Ok(Completion::Pending) => self.pending_voice_note = Some(pending),
            Err(error) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                self.voice_note_status.message.clone_from(&error.message);
                drop(pending);
                return Err(error.into());
            }
        }
        Ok(Response::VoiceNote(self.voice_note_status.clone()))
    }

    pub(super) fn complete_voice_note(
        &mut self,
        result: Result<TranscriptionOutput, PortError>,
    ) -> Result<(), RuntimeError> {
        let Some(pending) = self.pending_voice_note.take() else {
            return Err(RuntimeError::InvalidRequest(
                "voice-note completion is stale".to_owned(),
            ));
        };
        match result {
            Ok(output) if !output.transcript.trim().is_empty() => {
                let mut notes = pending.recording.timeline_notes.clone();
                notes.push(TimelineNote::voice(
                    pending.note_id.clone(),
                    pending.timestamp_seconds,
                    output.transcript.trim(),
                    self.clock.now(),
                ));
                validate_timeline_notes(&pending.recording, &notes)?;
                notes.sort_by(|left, right| {
                    left.timestamp_seconds
                        .total_cmp(&right.timestamp_seconds)
                        .then_with(|| left.id.cmp(&right.id))
                });
                self.storage
                    .save_timeline_notes(&pending.recording, &notes)?;
                self.voice_note_status = VoiceNoteStatus {
                    state: VoiceNoteState::Complete,
                    recording_id: Some(pending.recording.id.to_string()),
                    note_id: Some(pending.note_id.clone()),
                    message: "Voice note added.".to_owned(),
                };
                Ok(())
            }
            Ok(_) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                "Voice-note transcription was empty."
                    .clone_into(&mut self.voice_note_status.message);
                Ok(())
            }
            Err(error) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                self.voice_note_status.message.clone_from(&error.message);
                Ok(())
            }
        }
    }

    pub(super) fn cancel_voice_note(&mut self) -> Response {
        let Some(pending) = self.pending_voice_note.take() else {
            return Response::VoiceNote(self.voice_note_status.clone());
        };
        self.cancelled_voice_inflight = Some(pending.recording.id.clone());
        self.voice_note_status = VoiceNoteStatus {
            state: VoiceNoteState::Cancelling,
            recording_id: Some(pending.recording.id.to_string()),
            note_id: Some(pending.note_id.clone()),
            message: "Cancelling voice note…".to_owned(),
        };
        drop(pending);
        Response::VoiceNote(self.voice_note_status.clone())
    }
}

#[cfg(unix)]
fn validate_voice_note_audio(path: &Path) -> Result<PathBuf, PortError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("wav") {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio must be an absolute WAV path",
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        PortError::new(
            PortErrorKind::NotFound,
            format!("could not inspect voice-note audio: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != dicta_control::socket::effective_user_id()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() <= 44
        || metadata.len() > 64 * 1024 * 1024
    {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio is not a private, bounded regular WAV file",
        ));
    }
    let expected = voice_note_directory()?.canonicalize().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve private voice-note storage: {error}"),
        )
    })?;
    let canonical = path.canonicalize().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve voice-note audio: {error}"),
        )
    })?;
    if canonical.parent() != Some(expected.as_path()) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio escaped private runtime storage",
        ));
    }
    Ok(canonical)
}

#[cfg(not(unix))]
fn validate_voice_note_audio(_path: &Path) -> Result<PathBuf, PortError> {
    Err(PortError::new(
        PortErrorKind::Unavailable,
        "voice notes require the native Linux runtime",
    ))
}
