//! UI-independent application control for Dicta.
//!
//! [`Controller`] is the sole owner of mutable application state. Frontends send
//! typed [`Command`]s, execute work requested by returned [`Event`]s, and feed
//! completions back as commands. This crate deliberately owns no threads, event
//! loops, global handles, or platform resources.

use dicta_core::{ProjectId, RecordingId};
use std::{error::Error, fmt};

/// Immutable details shared by every phase of a recording session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingSession {
    pub recording_id: RecordingId,
    pub project_id: Option<ProjectId>,
    pub note: Option<String>,
}

impl RecordingSession {
    fn new(recording_id: RecordingId, project_id: Option<ProjectId>, note: Option<String>) -> Self {
        Self {
            recording_id,
            project_id,
            note: note.and_then(|note| {
                let note = note.trim().to_owned();
                (!note.is_empty()).then_some(note)
            }),
        }
    }
}

/// The operation that was running when the controller entered [`AppState::Failed`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    PrepareRecording,
    Capture,
    StopRecording,
    Transcribe,
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PrepareRecording => "prepare recording",
            Self::Capture => "capture",
            Self::StopRecording => "stop recording",
            Self::Transcribe => "transcribe",
        })
    }
}

/// Failure information retained until the user dismisses it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppFailure {
    pub operation: Operation,
    pub recording_id: RecordingId,
    pub message: String,
}

/// All possible controller states. Payloads are immutable outside [`Controller`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppState {
    Idle,
    Preparing(RecordingSession),
    Recording(RecordingSession),
    Annotating(RecordingSession),
    Stopping(RecordingSession),
    Transcribing { recording_id: RecordingId },
    Failed(AppFailure),
}

impl AppState {
    #[must_use]
    pub const fn kind(&self) -> StateKind {
        match self {
            Self::Idle => StateKind::Idle,
            Self::Preparing(_) => StateKind::Preparing,
            Self::Recording(_) => StateKind::Recording,
            Self::Annotating(_) => StateKind::Annotating,
            Self::Stopping(_) => StateKind::Stopping,
            Self::Transcribing { .. } => StateKind::Transcribing,
            Self::Failed(_) => StateKind::Failed,
        }
    }
}

/// Payload-free state name, useful for logs, protocol adapters, and errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateKind {
    Idle,
    Preparing,
    Recording,
    Annotating,
    Stopping,
    Transcribing,
    Failed,
}

impl fmt::Display for StateKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Preparing => "preparing",
            Self::Recording => "recording",
            Self::Annotating => "annotating",
            Self::Stopping => "stopping",
            Self::Transcribing => "transcribing",
            Self::Failed => "failed",
        })
    }
}

/// Inputs accepted by the controller.
///
/// Requests from a UI/CLI and completions from workers intentionally use the same
/// serialized dispatch point, preserving a single state owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    SelectProject(Option<ProjectId>),
    StartRecording {
        recording_id: RecordingId,
        note: Option<String>,
    },
    RecordingPrepared {
        recording_id: RecordingId,
    },
    StartAnnotating,
    StopAnnotating,
    StopRecording,
    RecordingStopped {
        recording_id: RecordingId,
        transcribe: bool,
    },
    TranscribeRecording {
        recording_id: RecordingId,
    },
    TranscriptionCompleted {
        recording_id: RecordingId,
    },
    OperationFailed {
        operation: Operation,
        recording_id: RecordingId,
        message: String,
    },
    DismissFailure,
}

impl Command {
    #[must_use]
    pub const fn kind(&self) -> CommandKind {
        match self {
            Self::SelectProject(_) => CommandKind::SelectProject,
            Self::StartRecording { .. } => CommandKind::StartRecording,
            Self::RecordingPrepared { .. } => CommandKind::RecordingPrepared,
            Self::StartAnnotating => CommandKind::StartAnnotating,
            Self::StopAnnotating => CommandKind::StopAnnotating,
            Self::StopRecording => CommandKind::StopRecording,
            Self::RecordingStopped { .. } => CommandKind::RecordingStopped,
            Self::TranscribeRecording { .. } => CommandKind::TranscribeRecording,
            Self::TranscriptionCompleted { .. } => CommandKind::TranscriptionCompleted,
            Self::OperationFailed { .. } => CommandKind::OperationFailed,
            Self::DismissFailure => CommandKind::DismissFailure,
        }
    }
}

/// Payload-free command name used in transition errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    SelectProject,
    StartRecording,
    RecordingPrepared,
    StartAnnotating,
    StopAnnotating,
    StopRecording,
    RecordingStopped,
    TranscribeRecording,
    TranscriptionCompleted,
    OperationFailed,
    DismissFailure,
}

impl fmt::Display for CommandKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SelectProject => "select project",
            Self::StartRecording => "start recording",
            Self::RecordingPrepared => "recording prepared",
            Self::StartAnnotating => "start annotating",
            Self::StopAnnotating => "stop annotating",
            Self::StopRecording => "stop recording",
            Self::RecordingStopped => "recording stopped",
            Self::TranscribeRecording => "transcribe recording",
            Self::TranscriptionCompleted => "transcription completed",
            Self::OperationFailed => "operation failed",
            Self::DismissFailure => "dismiss failure",
        })
    }
}

/// Domain notifications and work requests emitted after a successful transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    ProjectSelected(Option<ProjectId>),
    PrepareRecording(RecordingSession),
    RecordingStarted(RecordingSession),
    AnnotationModeChanged {
        recording_id: RecordingId,
        enabled: bool,
    },
    StopCapture(RecordingSession),
    RecordingSaved(RecordingSession),
    TranscriptionRequested {
        recording_id: RecordingId,
    },
    TranscriptionFinished {
        recording_id: RecordingId,
    },
    FailureRaised(AppFailure),
    FailureDismissed,
}

/// A revisioned, immutable view that can be handed directly to any frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub revision: u64,
    pub selected_project: Option<ProjectId>,
    pub state: AppState,
}

/// Result of one successful command dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchOutcome {
    pub snapshot: AppSnapshot,
    pub events: Vec<Event>,
}

/// A rejected command. Rejection never mutates controller state or revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerError {
    InvalidTransition {
        command: CommandKind,
        state: StateKind,
    },
    WrongRecording {
        command: CommandKind,
        expected: RecordingId,
        received: RecordingId,
    },
    UnexpectedOperation {
        operation: Operation,
        state: StateKind,
    },
    EmptyFailureMessage,
    RevisionOverflow,
}

impl fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { command, state } => {
                write!(formatter, "cannot {command} while application is {state}")
            }
            Self::WrongRecording {
                command,
                expected,
                received,
            } => write!(
                formatter,
                "cannot apply {command} for recording {received}; current recording is {expected}"
            ),
            Self::UnexpectedOperation { operation, state } => {
                write!(
                    formatter,
                    "{operation} cannot fail while application is {state}"
                )
            }
            Self::EmptyFailureMessage => formatter.write_str("failure message cannot be empty"),
            Self::RevisionOverflow => formatter.write_str("controller revision overflow"),
        }
    }
}

impl Error for ControllerError {}

/// The application's only mutable state owner.
#[derive(Debug)]
pub struct Controller {
    revision: u64,
    selected_project: Option<ProjectId>,
    state: AppState,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            revision: 0,
            selected_project: None,
            state: AppState::Idle,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            revision: self.revision,
            selected_project: self.selected_project.clone(),
            state: self.state.clone(),
        }
    }

    /// Applies exactly one command atomically.
    ///
    /// The next revision is checked before state is changed, and all validation
    /// happens before assignment. An error therefore leaves the controller intact.
    ///
    /// # Errors
    ///
    /// Returns [`ControllerError`] when the command is invalid for the current
    /// state, references a stale recording, reports the wrong operation, contains
    /// an empty failure message, or the revision counter is exhausted.
    #[allow(clippy::too_many_lines)]
    pub fn dispatch(&mut self, command: Command) -> Result<DispatchOutcome, ControllerError> {
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(ControllerError::RevisionOverflow)?;
        let command_kind = command.kind();
        let current_kind = self.state.kind();
        let mut selected_project = self.selected_project.clone();

        let (next_state, events) = match command {
            Command::SelectProject(project_id) if matches!(self.state, AppState::Idle) => {
                selected_project.clone_from(&project_id);
                (AppState::Idle, vec![Event::ProjectSelected(project_id)])
            }
            Command::StartRecording { recording_id, note }
                if matches!(self.state, AppState::Idle) =>
            {
                let session = RecordingSession::new(recording_id, selected_project.clone(), note);
                (
                    AppState::Preparing(session.clone()),
                    vec![Event::PrepareRecording(session)],
                )
            }
            Command::RecordingPrepared { recording_id } => {
                let session =
                    self.session_for(command_kind, &recording_id, StateKind::Preparing)?;
                (
                    AppState::Recording(session.clone()),
                    vec![Event::RecordingStarted(session)],
                )
            }
            Command::StartAnnotating if matches!(self.state, AppState::Recording(_)) => {
                let session = self.session_in(StateKind::Recording);
                (
                    AppState::Annotating(session.clone()),
                    vec![Event::AnnotationModeChanged {
                        recording_id: session.recording_id,
                        enabled: true,
                    }],
                )
            }
            Command::StopAnnotating if matches!(self.state, AppState::Annotating(_)) => {
                let session = self.session_in(StateKind::Annotating);
                (
                    AppState::Recording(session.clone()),
                    vec![Event::AnnotationModeChanged {
                        recording_id: session.recording_id,
                        enabled: false,
                    }],
                )
            }
            Command::StopRecording
                if matches!(self.state, AppState::Recording(_) | AppState::Annotating(_)) =>
            {
                let session = self.session_in(self.state.kind());
                (
                    AppState::Stopping(session.clone()),
                    vec![Event::StopCapture(session)],
                )
            }
            Command::RecordingStopped {
                recording_id,
                transcribe,
            } => {
                let session = self.session_for(command_kind, &recording_id, StateKind::Stopping)?;
                let mut events = vec![Event::RecordingSaved(session.clone())];
                if transcribe {
                    events.push(Event::TranscriptionRequested {
                        recording_id: recording_id.clone(),
                    });
                    (AppState::Transcribing { recording_id }, events)
                } else {
                    (AppState::Idle, events)
                }
            }
            Command::TranscribeRecording { recording_id }
                if matches!(self.state, AppState::Idle) =>
            {
                (
                    AppState::Transcribing {
                        recording_id: recording_id.clone(),
                    },
                    vec![Event::TranscriptionRequested { recording_id }],
                )
            }
            Command::TranscriptionCompleted { recording_id } => {
                self.require_recording(command_kind, &recording_id, StateKind::Transcribing)?;
                (
                    AppState::Idle,
                    vec![Event::TranscriptionFinished { recording_id }],
                )
            }
            Command::OperationFailed {
                operation,
                recording_id,
                message,
            } => {
                self.validate_failed_operation(operation, &recording_id)?;
                let message = message.trim().to_owned();
                if message.is_empty() {
                    return Err(ControllerError::EmptyFailureMessage);
                }
                let failure = AppFailure {
                    operation,
                    recording_id,
                    message,
                };
                (
                    AppState::Failed(failure.clone()),
                    vec![Event::FailureRaised(failure)],
                )
            }
            Command::DismissFailure if matches!(self.state, AppState::Failed(_)) => {
                (AppState::Idle, vec![Event::FailureDismissed])
            }
            _ => {
                return Err(ControllerError::InvalidTransition {
                    command: command_kind,
                    state: current_kind,
                });
            }
        };

        self.selected_project = selected_project;
        self.state = next_state;
        self.revision = next_revision;
        Ok(DispatchOutcome {
            snapshot: self.snapshot(),
            events,
        })
    }

    fn current_session(&self) -> Option<&RecordingSession> {
        match &self.state {
            AppState::Preparing(session)
            | AppState::Recording(session)
            | AppState::Annotating(session)
            | AppState::Stopping(session) => Some(session),
            AppState::Idle | AppState::Transcribing { .. } | AppState::Failed(_) => None,
        }
    }

    fn session_in(&self, state: StateKind) -> RecordingSession {
        debug_assert_eq!(self.state.kind(), state);
        match &self.state {
            AppState::Preparing(session)
            | AppState::Recording(session)
            | AppState::Annotating(session)
            | AppState::Stopping(session) => session.clone(),
            AppState::Idle | AppState::Transcribing { .. } | AppState::Failed(_) => {
                unreachable!("{state} is a recording session state")
            }
        }
    }

    fn session_for(
        &self,
        command: CommandKind,
        recording_id: &RecordingId,
        required_state: StateKind,
    ) -> Result<RecordingSession, ControllerError> {
        if self.state.kind() != required_state {
            return Err(ControllerError::InvalidTransition {
                command,
                state: self.state.kind(),
            });
        }
        let session = self.session_in(required_state);
        if &session.recording_id != recording_id {
            return Err(ControllerError::WrongRecording {
                command,
                expected: session.recording_id,
                received: recording_id.clone(),
            });
        }
        Ok(session)
    }

    fn require_recording(
        &self,
        command: CommandKind,
        recording_id: &RecordingId,
        required_state: StateKind,
    ) -> Result<(), ControllerError> {
        if self.state.kind() != required_state {
            return Err(ControllerError::InvalidTransition {
                command,
                state: self.state.kind(),
            });
        }
        let AppState::Transcribing {
            recording_id: expected,
        } = &self.state
        else {
            unreachable!("required state is transcribing");
        };
        if expected != recording_id {
            return Err(ControllerError::WrongRecording {
                command,
                expected: expected.clone(),
                received: recording_id.clone(),
            });
        }
        Ok(())
    }

    fn validate_failed_operation(
        &self,
        operation: Operation,
        recording_id: &RecordingId,
    ) -> Result<(), ControllerError> {
        let valid_state = match operation {
            Operation::PrepareRecording => StateKind::Preparing,
            Operation::Capture => match self.state {
                AppState::Recording(_) | AppState::Annotating(_) => self.state.kind(),
                _ => {
                    return Err(ControllerError::UnexpectedOperation {
                        operation,
                        state: self.state.kind(),
                    });
                }
            },
            Operation::StopRecording => StateKind::Stopping,
            Operation::Transcribe => StateKind::Transcribing,
        };
        if self.state.kind() != valid_state {
            return Err(ControllerError::UnexpectedOperation {
                operation,
                state: self.state.kind(),
            });
        }

        if let Some(session) = self.current_session() {
            if &session.recording_id != recording_id {
                return Err(ControllerError::WrongRecording {
                    command: CommandKind::OperationFailed,
                    expected: session.recording_id.clone(),
                    received: recording_id.clone(),
                });
            }
        } else if let AppState::Transcribing {
            recording_id: expected,
        } = &self.state
        {
            if expected != recording_id {
                return Err(ControllerError::WrongRecording {
                    command: CommandKind::OperationFailed,
                    expected: expected.clone(),
                    received: recording_id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(value: &str) -> ProjectId {
        ProjectId::new(value).unwrap()
    }

    fn recording(value: &str) -> RecordingId {
        RecordingId::new(value).unwrap()
    }

    fn dispatch(controller: &mut Controller, command: Command) -> DispatchOutcome {
        controller.dispatch(command).unwrap()
    }

    fn begin_recording(controller: &mut Controller, id: &str) -> RecordingSession {
        let recording_id = recording(id);
        dispatch(
            controller,
            Command::StartRecording {
                recording_id: recording_id.clone(),
                note: Some("  reproduce the issue  ".to_owned()),
            },
        );
        let outcome = dispatch(controller, Command::RecordingPrepared { recording_id });
        match outcome.snapshot.state {
            AppState::Recording(session) => session,
            state => panic!("expected recording, got {state:?}"),
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn complete_recording_flow_emits_worker_requests_and_snapshots() {
        let mut controller = Controller::new();
        assert_eq!(controller.snapshot().revision, 0);

        let selected = dispatch(
            &mut controller,
            Command::SelectProject(Some(project("dicta"))),
        );
        assert_eq!(selected.snapshot.revision, 1);
        assert_eq!(
            selected.events,
            vec![Event::ProjectSelected(Some(project("dicta")))]
        );

        let started = dispatch(
            &mut controller,
            Command::StartRecording {
                recording_id: recording("r1"),
                note: Some("  explain failure  ".to_owned()),
            },
        );
        let expected_session = RecordingSession {
            recording_id: recording("r1"),
            project_id: Some(project("dicta")),
            note: Some("explain failure".to_owned()),
        };
        assert_eq!(
            started.snapshot.state,
            AppState::Preparing(expected_session.clone())
        );
        assert_eq!(
            started.events,
            vec![Event::PrepareRecording(expected_session.clone())]
        );

        let prepared = dispatch(
            &mut controller,
            Command::RecordingPrepared {
                recording_id: recording("r1"),
            },
        );
        assert_eq!(
            prepared.snapshot.state,
            AppState::Recording(expected_session.clone())
        );
        assert_eq!(
            prepared.events,
            vec![Event::RecordingStarted(expected_session.clone())]
        );

        let annotating = dispatch(&mut controller, Command::StartAnnotating);
        assert_eq!(
            annotating.snapshot.state,
            AppState::Annotating(expected_session.clone())
        );
        assert_eq!(
            annotating.events,
            vec![Event::AnnotationModeChanged {
                recording_id: recording("r1"),
                enabled: true,
            }]
        );

        let stopping = dispatch(&mut controller, Command::StopRecording);
        assert_eq!(
            stopping.snapshot.state,
            AppState::Stopping(expected_session.clone())
        );
        assert_eq!(
            stopping.events,
            vec![Event::StopCapture(expected_session.clone())]
        );

        let stopped = dispatch(
            &mut controller,
            Command::RecordingStopped {
                recording_id: recording("r1"),
                transcribe: true,
            },
        );
        assert_eq!(
            stopped.snapshot.state,
            AppState::Transcribing {
                recording_id: recording("r1")
            }
        );
        assert_eq!(
            stopped.events,
            vec![
                Event::RecordingSaved(expected_session),
                Event::TranscriptionRequested {
                    recording_id: recording("r1")
                }
            ]
        );

        let completed = dispatch(
            &mut controller,
            Command::TranscriptionCompleted {
                recording_id: recording("r1"),
            },
        );
        assert_eq!(completed.snapshot.state, AppState::Idle);
        assert_eq!(completed.snapshot.revision, 7);
        assert_eq!(
            completed.events,
            vec![Event::TranscriptionFinished {
                recording_id: recording("r1")
            }]
        );
    }

    #[test]
    fn annotation_mode_can_return_to_recording() {
        let mut controller = Controller::new();
        let session = begin_recording(&mut controller, "r1");

        dispatch(&mut controller, Command::StartAnnotating);
        let outcome = dispatch(&mut controller, Command::StopAnnotating);

        assert_eq!(outcome.snapshot.state, AppState::Recording(session));
        assert_eq!(
            outcome.events,
            vec![Event::AnnotationModeChanged {
                recording_id: recording("r1"),
                enabled: false,
            }]
        );
    }

    #[test]
    fn stopping_without_transcription_returns_to_idle() {
        let mut controller = Controller::new();
        let session = begin_recording(&mut controller, "r1");
        dispatch(&mut controller, Command::StopRecording);

        let outcome = dispatch(
            &mut controller,
            Command::RecordingStopped {
                recording_id: recording("r1"),
                transcribe: false,
            },
        );

        assert_eq!(outcome.snapshot.state, AppState::Idle);
        assert_eq!(outcome.events, vec![Event::RecordingSaved(session)]);
    }

    #[test]
    fn an_existing_recording_can_be_transcribed_from_idle() {
        let mut controller = Controller::new();
        let requested = dispatch(
            &mut controller,
            Command::TranscribeRecording {
                recording_id: recording("existing"),
            },
        );
        assert_eq!(
            requested.snapshot.state,
            AppState::Transcribing {
                recording_id: recording("existing")
            }
        );

        let completed = dispatch(
            &mut controller,
            Command::TranscriptionCompleted {
                recording_id: recording("existing"),
            },
        );
        assert_eq!(completed.snapshot.state, AppState::Idle);
    }

    #[test]
    fn failures_are_typed_trimmed_and_dismissible() {
        let mut controller = Controller::new();
        dispatch(
            &mut controller,
            Command::StartRecording {
                recording_id: recording("r1"),
                note: None,
            },
        );

        let outcome = dispatch(
            &mut controller,
            Command::OperationFailed {
                operation: Operation::PrepareRecording,
                recording_id: recording("r1"),
                message: "  capture permission denied  ".to_owned(),
            },
        );
        let failure = AppFailure {
            operation: Operation::PrepareRecording,
            recording_id: recording("r1"),
            message: "capture permission denied".to_owned(),
        };
        assert_eq!(outcome.snapshot.state, AppState::Failed(failure.clone()));
        assert_eq!(outcome.events, vec![Event::FailureRaised(failure)]);

        let dismissed = dispatch(&mut controller, Command::DismissFailure);
        assert_eq!(dismissed.snapshot.state, AppState::Idle);
        assert_eq!(dismissed.events, vec![Event::FailureDismissed]);
    }

    #[test]
    fn every_active_operation_accepts_its_matching_failure() {
        for (operation, setup) in [
            (Operation::PrepareRecording, 0_u8),
            (Operation::Capture, 1),
            (Operation::Capture, 2),
            (Operation::StopRecording, 3),
            (Operation::Transcribe, 4),
        ] {
            let mut controller = Controller::new();
            dispatch(
                &mut controller,
                Command::StartRecording {
                    recording_id: recording("r1"),
                    note: None,
                },
            );
            if setup >= 1 {
                dispatch(
                    &mut controller,
                    Command::RecordingPrepared {
                        recording_id: recording("r1"),
                    },
                );
            }
            if setup == 2 {
                dispatch(&mut controller, Command::StartAnnotating);
            }
            if setup >= 3 {
                dispatch(&mut controller, Command::StopRecording);
            }
            if setup == 4 {
                dispatch(
                    &mut controller,
                    Command::RecordingStopped {
                        recording_id: recording("r1"),
                        transcribe: true,
                    },
                );
            }

            let outcome = dispatch(
                &mut controller,
                Command::OperationFailed {
                    operation,
                    recording_id: recording("r1"),
                    message: "failed".to_owned(),
                },
            );
            assert!(matches!(outcome.snapshot.state, AppState::Failed(_)));
        }
    }

    #[test]
    fn invalid_transition_does_not_mutate_state_or_revision() {
        let mut controller = Controller::new();
        let before = controller.snapshot();

        let error = controller.dispatch(Command::StopRecording).unwrap_err();

        assert_eq!(
            error,
            ControllerError::InvalidTransition {
                command: CommandKind::StopRecording,
                state: StateKind::Idle,
            }
        );
        assert_eq!(controller.snapshot(), before);
    }

    #[test]
    fn stale_worker_completion_is_rejected_without_mutation() {
        let mut controller = Controller::new();
        dispatch(
            &mut controller,
            Command::StartRecording {
                recording_id: recording("current"),
                note: None,
            },
        );
        let before = controller.snapshot();

        let error = controller
            .dispatch(Command::RecordingPrepared {
                recording_id: recording("stale"),
            })
            .unwrap_err();

        assert_eq!(
            error,
            ControllerError::WrongRecording {
                command: CommandKind::RecordingPrepared,
                expected: recording("current"),
                received: recording("stale"),
            }
        );
        assert_eq!(controller.snapshot(), before);
    }

    #[test]
    fn project_cannot_change_during_an_active_operation() {
        let mut controller = Controller::new();
        begin_recording(&mut controller, "r1");
        let before = controller.snapshot();

        let error = controller
            .dispatch(Command::SelectProject(Some(project("other"))))
            .unwrap_err();

        assert_eq!(
            error,
            ControllerError::InvalidTransition {
                command: CommandKind::SelectProject,
                state: StateKind::Recording,
            }
        );
        assert_eq!(controller.snapshot(), before);
    }

    #[test]
    fn empty_notes_are_not_retained() {
        let mut controller = Controller::new();
        let outcome = dispatch(
            &mut controller,
            Command::StartRecording {
                recording_id: recording("r1"),
                note: Some(" \n\t ".to_owned()),
            },
        );
        let AppState::Preparing(session) = outcome.snapshot.state else {
            panic!("expected preparing state");
        };
        assert_eq!(session.note, None);
    }

    #[test]
    fn wrong_failure_operation_and_empty_message_are_rejected_atomically() {
        let mut controller = Controller::new();
        dispatch(
            &mut controller,
            Command::StartRecording {
                recording_id: recording("r1"),
                note: None,
            },
        );
        let before = controller.snapshot();

        let operation_error = controller
            .dispatch(Command::OperationFailed {
                operation: Operation::Transcribe,
                recording_id: recording("r1"),
                message: "failure".to_owned(),
            })
            .unwrap_err();
        assert_eq!(
            operation_error,
            ControllerError::UnexpectedOperation {
                operation: Operation::Transcribe,
                state: StateKind::Preparing,
            }
        );
        assert_eq!(controller.snapshot(), before);

        let message_error = controller
            .dispatch(Command::OperationFailed {
                operation: Operation::PrepareRecording,
                recording_id: recording("r1"),
                message: "   ".to_owned(),
            })
            .unwrap_err();
        assert_eq!(message_error, ControllerError::EmptyFailureMessage);
        assert_eq!(controller.snapshot(), before);
    }
}
