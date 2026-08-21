//! Sole mutable adapter around the Dicta engine controller.

mod dispatch;
mod render;
mod voice;
pub(crate) mod wire;

pub(crate) use voice::PendingVoiceNote;

use crate::{
    error::{ControlOutput, RuntimeConfig, RuntimeError, RuntimeSnapshot},
    ports::{
        AnnotationPort, CapturePoll, CapturePort, Clock, Completion, IdSource, PortError,
        PortErrorKind, StoragePort, TranscriptionPort,
    },
};
use dicta_capture::CaptureArtifact;
use dicta_control::{
    protocol::StatusSnapshot, AnnotationTool, Command as ControlCommand, Event as ControlEvent,
    EventEnvelope, RequestEnvelope, ResponseEnvelope, VoiceNoteStatus,
};
use dicta_core::RecordingId;
use dicta_engine::{
    AppSnapshot, AppState, Command as EngineCommand, CommandKind, Controller, ControllerError,
    Operation, RecordingSession, StateKind,
};
use dicta_transcribe::TranscriptionOutput;

use self::render::status_from_snapshot;

/// Oldest events are dropped once this many have been retained.
pub const MAX_RETAINED_EVENTS: usize = 256;

/// Sole mutable adapter around [`Controller`].
pub struct Runtime<C, T, A, S, K, I> {
    pub(crate) controller: Controller,
    pub(crate) capture: C,
    pub(crate) transcription: T,
    pub(crate) annotations: A,
    pub(crate) storage: S,
    pub(crate) clock: K,
    pub(crate) ids: I,
    pub(crate) config: RuntimeConfig,
    pub(crate) selected_tool: AnnotationTool,
    pub(crate) next_event_sequence: u64,
    pub(crate) events: Vec<ControlEvent>,
    pub(crate) pending_voice_note: Option<PendingVoiceNote>,
    pub(crate) cancelled_voice_inflight: Option<RecordingId>,
    pub(crate) voice_note_status: VoiceNoteStatus,
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
    #[must_use]
    pub fn new(
        capture: C,
        transcription: T,
        annotations: A,
        storage: S,
        clock: K,
        ids: I,
        config: RuntimeConfig,
    ) -> Self {
        Self {
            controller: Controller::new(),
            capture,
            transcription,
            annotations,
            storage,
            clock,
            ids,
            config,
            selected_tool: AnnotationTool::Pen,
            next_event_sequence: 1,
            events: Vec::new(),
            pending_voice_note: None,
            cancelled_voice_inflight: None,
            voice_note_status: VoiceNoteStatus::default(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            app: self.controller.snapshot(),
            status: self.status(),
            last_event_sequence: self.next_event_sequence.saturating_sub(1),
        }
    }

    #[must_use]
    pub fn events(&self) -> &[ControlEvent] {
        &self.events
    }

    #[must_use]
    pub fn events_since(&self, sequence: Option<u64>) -> Vec<EventEnvelope> {
        let sequence = sequence.unwrap_or(0);
        self.events
            .iter()
            .filter(|event| event_sequence(event) > sequence)
            .cloned()
            .map(EventEnvelope::new)
            .collect()
    }

    /// Polls injected background ports once without blocking the runtime thread.
    ///
    /// A completed worker failure is converted into the normal typed failed
    /// state and is therefore considered consumed. Conflicts and internal event
    /// sequence failures remain errors so stale completions never mutate state.
    ///
    /// # Errors
    /// Returns a conflict for a stale completion or an internal event-sequence
    /// failure.
    pub fn poll_background(&mut self) -> Result<bool, RuntimeError> {
        let mut consumed = false;
        match self.capture.poll() {
            Ok(CapturePoll::Idle | CapturePoll::Running) => {}
            Ok(CapturePoll::Stopped(artifact)) => {
                consumed = true;
                match self.complete_polled_capture_stop(&artifact) {
                    Ok(()) | Err(RuntimeError::Port(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) => {
                consumed = true;
                match self.fail_polled_capture(error) {
                    Ok(()) | Err(RuntimeError::Port(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        if let Some(completion) = self.transcription.poll_completion() {
            if self
                .pending_voice_note
                .as_ref()
                .is_some_and(|pending| pending.recording.id == completion.recording_id)
            {
                self.complete_voice_note(completion.result)?;
                consumed = true;
            } else if self.cancelled_voice_inflight.as_ref() == Some(&completion.recording_id) {
                self.cancelled_voice_inflight = None;
                self.voice_note_status = VoiceNoteStatus::default();
                consumed = true;
            } else {
                match self.complete_transcription(completion.recording_id, completion.result) {
                    Ok(()) | Err(RuntimeError::Port(_)) => consumed = true,
                    Err(error) => return Err(error),
                }
            }
        }
        if self.transcription.poll_model_install().is_some() {
            consumed = true;
        }
        if self.config.transcribe_after_recording
            && self.transcription.is_available()
            && self.controller.snapshot().state.kind() == StateKind::Idle
            && self.pending_voice_note.is_none()
            && self.cancelled_voice_inflight.is_none()
        {
            if let Some(candidate) = self.storage.poll_transcription_retry() {
                consumed = true;
                if let Ok(recording) = candidate {
                    self.start_retry_transcription(&recording)?;
                }
            }
        }
        Ok(consumed)
    }

    /// Translates one validated wire request into domain work and a stable response.
    pub fn handle(&mut self, request: RequestEnvelope) -> ControlOutput {
        let id = request.id;
        if let Err(error) = request.validate_version() {
            return ControlOutput {
                response: ResponseEnvelope::failure(id, error),
                events: Vec::new(),
            };
        }

        let event_start = self.events.len();
        let queried_events = match &request.command {
            ControlCommand::Events { since_sequence, .. } => Some(*since_sequence),
            _ => None,
        };
        let result = self.apply_control(request.command);
        let events = queried_events.map_or_else(
            || {
                self.events[event_start..]
                    .iter()
                    .cloned()
                    .map(EventEnvelope::new)
                    .collect()
            },
            |since| self.events_since(since),
        );
        let response = match result {
            Ok(response) => ResponseEnvelope::success(id, response),
            Err(error) => ResponseEnvelope::failure(id, error.protocol_error()),
        };
        ControlOutput { response, events }
    }

    /// Completes a pending recorder startup. Stale IDs are rejected atomically.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, the supplied port failure, or an
    /// internal sequence error.
    pub fn complete_capture_start(
        &mut self,
        recording_id: RecordingId,
        result: Result<(), PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(4)?;
        self.require_recording(
            StateKind::Preparing,
            &recording_id,
            CommandKind::RecordingPrepared,
        )?;
        match result {
            Ok(()) => self.complete_capture_start_inner(recording_id),
            Err(error) => {
                self.raise_failure(Operation::PrepareRecording, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Completes a pending recorder stop. Stale IDs cause no port or storage calls.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, or a typed capture, annotation,
    /// storage, transcription, or sequence failure.
    pub fn complete_capture_stop(
        &mut self,
        recording_id: RecordingId,
        result: Result<CaptureArtifact, PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(8)?;
        let session = self.require_session(
            StateKind::Stopping,
            &recording_id,
            CommandKind::RecordingStopped,
        )?;
        match result {
            Ok(artifact) => self.complete_capture_stop_inner(&session, &artifact),
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Completes a pending transcription. Stale IDs never reach storage.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, or a typed transcription,
    /// storage, or sequence failure.
    pub fn complete_transcription(
        &mut self,
        recording_id: RecordingId,
        result: Result<TranscriptionOutput, PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(4)?;
        self.require_recording(
            StateKind::Transcribing,
            &recording_id,
            CommandKind::TranscriptionCompleted,
        )?;
        match result {
            Ok(output) => {
                if let Err(error) = self.storage.save_transcription(&recording_id, &output) {
                    self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                    return Err(error.into());
                }
                let outcome = self
                    .controller
                    .dispatch(EngineCommand::TranscriptionCompleted {
                        recording_id: recording_id.clone(),
                    })?;
                self.publish(ControlEvent::TranscriptionCompleted {
                    sequence: 0,
                    recording_id: recording_id.into_string(),
                })?;
                self.publish_state(&outcome.snapshot)
            }
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Reports a recorder failure after startup. Only the active recording matches.
    ///
    /// # Errors
    /// Returns a conflict for stale or invalid failure reports, or a sequence error.
    pub fn capture_failed(
        &mut self,
        recording_id: RecordingId,
        error: PortError,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(3)?;
        let kind = self.controller.snapshot().state.kind();
        if !matches!(kind, StateKind::Recording | StateKind::Annotating) {
            return Err(ControllerError::UnexpectedOperation {
                operation: Operation::Capture,
                state: kind,
            }
            .into());
        }
        self.require_recording(kind, &recording_id, CommandKind::OperationFailed)?;
        self.raise_failure(Operation::Capture, recording_id, &error)?;
        Err(error.into())
    }

    fn complete_polled_capture_stop(
        &mut self,
        artifact: &CaptureArtifact,
    ) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        let session = match snapshot.state.kind() {
            StateKind::Recording | StateKind::Annotating => {
                let session = session_from_state(&snapshot.state).clone();
                let outcome = self.controller.dispatch(EngineCommand::StopRecording)?;
                self.publish_state(&outcome.snapshot)?;
                session
            }
            StateKind::Stopping => session_from_state(&snapshot.state).clone(),
            _ => return Ok(()),
        };
        self.complete_capture_stop_inner(&session, artifact)
    }

    fn fail_polled_capture(&mut self, error: PortError) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        let Some(recording_id) = recording_id_from_state(&snapshot.state).cloned() else {
            return Ok(());
        };
        match snapshot.state.kind() {
            StateKind::Preparing => self.complete_capture_start(recording_id, Err(error)),
            StateKind::Recording | StateKind::Annotating => {
                self.capture_failed(recording_id, error)
            }
            StateKind::Stopping => self.complete_capture_stop(recording_id, Err(error)),
            _ => Ok(()),
        }
    }

    pub(super) fn complete_capture_start_inner(
        &mut self,
        recording_id: RecordingId,
    ) -> Result<(), RuntimeError> {
        let outcome = self.controller.dispatch(EngineCommand::RecordingPrepared {
            recording_id: recording_id.clone(),
        })?;
        self.publish(ControlEvent::RecordingStarted {
            sequence: 0,
            recording_id: recording_id.into_string(),
        })?;
        self.publish_state(&outcome.snapshot)
    }

    pub(super) fn complete_capture_stop_inner(
        &mut self,
        session: &RecordingSession,
        artifact: &CaptureArtifact,
    ) -> Result<(), RuntimeError> {
        let recording_id = session.recording_id.clone();
        if artifact.path.as_os_str().is_empty()
            || artifact.output_name.trim().is_empty()
            || artifact.geometry.width == 0
            || artifact.geometry.height == 0
            || artifact.scale_milli == 0
            || artifact.encoded_pixel_size.0 == 0
            || artifact.encoded_pixel_size.1 == 0
        {
            let error = PortError::new(
                PortErrorKind::Internal,
                "capture port returned an invalid artifact",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let annotations = match self.annotations.finish(&recording_id) {
            Ok(annotations) => annotations,
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        };
        if annotations
            .as_ref()
            .is_some_and(|document| !document.is_valid() || document.recording_id != recording_id)
        {
            let error = PortError::new(
                PortErrorKind::Internal,
                "annotation port returned an invalid document",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let saved = match self
            .storage
            .save_recording(session, artifact, annotations.as_ref())
        {
            Ok(recording) => recording,
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        };
        if !saved.is_valid() || saved.id != recording_id {
            let error = PortError::new(
                PortErrorKind::Internal,
                "storage returned invalid recording metadata",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let duration_seconds = artifact.duration.as_secs_f64();
        let should_transcribe =
            self.config.transcribe_after_recording && self.transcription.is_available();
        let outcome = self.controller.dispatch(EngineCommand::RecordingStopped {
            recording_id: recording_id.clone(),
            transcribe: should_transcribe,
        })?;
        self.publish(ControlEvent::RecordingStopped {
            sequence: 0,
            recording_id: recording_id.clone().into_string(),
            duration_seconds,
        })?;
        self.publish_state(&outcome.snapshot)?;
        if should_transcribe {
            self.storage.mark_transcription_pending(&recording_id)?;
            match self.transcription.transcribe(&saved) {
                Ok(Completion::Ready(output)) => {
                    self.complete_transcription(recording_id, Ok(output))?;
                }
                Ok(Completion::Pending) => {}
                Err(error) => {
                    self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    pub(super) fn require_annotating_session(&self) -> Result<RecordingSession, RuntimeError> {
        let snapshot = self.controller.snapshot();
        if snapshot.state.kind() != StateKind::Annotating {
            return Err(RuntimeError::CommandConflict {
                command: "edit annotations",
                state: snapshot.state.kind(),
            });
        }
        Ok(session_from_state(&snapshot.state).clone())
    }

    pub(super) fn require_session(
        &self,
        state: StateKind,
        recording_id: &RecordingId,
        command: CommandKind,
    ) -> Result<RecordingSession, RuntimeError> {
        self.require_recording(state, recording_id, command)?;
        Ok(session_from_state(&self.controller.snapshot().state).clone())
    }

    pub(super) fn require_recording(
        &self,
        state: StateKind,
        recording_id: &RecordingId,
        command: CommandKind,
    ) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        if snapshot.state.kind() != state {
            return Err(ControllerError::InvalidTransition {
                command,
                state: snapshot.state.kind(),
            }
            .into());
        }
        let current_id = recording_id_from_state(&snapshot.state);
        if let Some(expected) = current_id {
            if expected != recording_id {
                return Err(ControllerError::WrongRecording {
                    command,
                    expected: expected.clone(),
                    received: recording_id.clone(),
                }
                .into());
            }
        }
        Ok(())
    }

    pub(super) fn raise_failure(
        &mut self,
        operation: Operation,
        recording_id: RecordingId,
        error: &PortError,
    ) -> Result<(), RuntimeError> {
        if matches!(operation, Operation::Transcribe) {
            let _ = self
                .storage
                .mark_transcription_failed(&recording_id, &error.message);
        }
        let message = if error.message.trim().is_empty() {
            "operation failed".to_owned()
        } else {
            error.message.clone()
        };
        let outcome = self.controller.dispatch(EngineCommand::OperationFailed {
            operation,
            recording_id,
            message,
        })?;
        self.publish(ControlEvent::Failed {
            sequence: 0,
            error: error.protocol_error(),
        })?;
        self.publish_state(&outcome.snapshot)
    }

    pub(super) fn status(&self) -> StatusSnapshot {
        status_from_snapshot(&self.controller.snapshot(), self.selected_tool)
    }

    pub(super) fn publish_current_state(&mut self) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        self.publish_state(&snapshot)
    }

    pub(super) fn publish_state(&mut self, snapshot: &AppSnapshot) -> Result<(), RuntimeError> {
        self.publish(ControlEvent::StateChanged {
            sequence: 0,
            status: status_from_snapshot(snapshot, self.selected_tool),
        })
    }

    pub(super) fn publish(&mut self, event: ControlEvent) -> Result<(), RuntimeError> {
        let sequence = self.next_event_sequence;
        self.next_event_sequence = sequence
            .checked_add(1)
            .ok_or(RuntimeError::EventSequenceExhausted)?;
        self.events.push(with_sequence(event, sequence));
        let overflow = self.events.len().saturating_sub(MAX_RETAINED_EVENTS);
        if overflow > 0 {
            self.events.drain(..overflow);
        }
        Ok(())
    }

    pub(super) fn ensure_event_capacity(&self, count: u64) -> Result<(), RuntimeError> {
        self.next_event_sequence
            .checked_add(count)
            .ok_or(RuntimeError::EventSequenceExhausted)
            .map(|_| ())
    }
}

#[derive(Clone, Copy)]
pub(super) enum AnnotationEdit {
    Undo,
    Clear,
}

pub(super) fn session_from_state(state: &AppState) -> &RecordingSession {
    match state {
        AppState::Preparing(session)
        | AppState::Recording(session)
        | AppState::Annotating(session)
        | AppState::Stopping(session) => session,
        AppState::Idle | AppState::Transcribing { .. } | AppState::Failed(_) => {
            unreachable!("caller validated a recording session state")
        }
    }
}

pub(super) fn recording_id_from_state(state: &AppState) -> Option<&RecordingId> {
    match state {
        AppState::Preparing(session)
        | AppState::Recording(session)
        | AppState::Annotating(session)
        | AppState::Stopping(session) => Some(&session.recording_id),
        AppState::Transcribing { recording_id } => Some(recording_id),
        AppState::Failed(failure) => Some(&failure.recording_id),
        AppState::Idle => None,
    }
}

pub(crate) fn event_sequence(event: &ControlEvent) -> u64 {
    match event {
        ControlEvent::UiShowRequested { sequence }
        | ControlEvent::UiRecordingRequested { sequence, .. }
        | ControlEvent::StateChanged { sequence, .. }
        | ControlEvent::RecordingStarted { sequence, .. }
        | ControlEvent::RecordingStopped { sequence, .. }
        | ControlEvent::AnnotationCreated { sequence, .. }
        | ControlEvent::TranscriptionCompleted { sequence, .. }
        | ControlEvent::Failed { sequence, .. } => *sequence,
    }
}

pub(super) fn with_sequence(event: ControlEvent, value: u64) -> ControlEvent {
    match event {
        ControlEvent::UiShowRequested { .. } => ControlEvent::UiShowRequested { sequence: value },
        ControlEvent::UiRecordingRequested { recording_id, .. } => {
            ControlEvent::UiRecordingRequested {
                sequence: value,
                recording_id,
            }
        }
        ControlEvent::StateChanged { status, .. } => ControlEvent::StateChanged {
            sequence: value,
            status,
        },
        ControlEvent::RecordingStarted { recording_id, .. } => ControlEvent::RecordingStarted {
            sequence: value,
            recording_id,
        },
        ControlEvent::RecordingStopped {
            recording_id,
            duration_seconds,
            ..
        } => ControlEvent::RecordingStopped {
            sequence: value,
            recording_id,
            duration_seconds,
        },
        ControlEvent::AnnotationCreated {
            tool,
            timestamp_seconds,
            ..
        } => ControlEvent::AnnotationCreated {
            sequence: value,
            tool,
            timestamp_seconds,
        },
        ControlEvent::TranscriptionCompleted { recording_id, .. } => {
            ControlEvent::TranscriptionCompleted {
                sequence: value,
                recording_id,
            }
        }
        ControlEvent::Failed { error, .. } => ControlEvent::Failed {
            sequence: value,
            error,
        },
    }
}
