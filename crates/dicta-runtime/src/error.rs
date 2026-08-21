//! Runtime configuration, snapshots, and typed errors.

use crate::ports::PortError;
use dicta_control::{
    protocol::StatusSnapshot, ErrorCode, EventEnvelope, ProtocolError, ResponseEnvelope,
};
use dicta_engine::{AppSnapshot, ControllerError, StateKind};
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    pub transcribe_after_recording: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            transcribe_after_recording: true,
        }
    }
}

/// Read-only state safe to hand to QML, a CLI server, or an MCP adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSnapshot {
    pub app: AppSnapshot,
    pub status: StatusSnapshot,
    pub last_event_sequence: u64,
}

/// The correlated response and event frames produced by one request.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlOutput {
    pub response: ResponseEnvelope,
    pub events: Vec<EventEnvelope>,
}

#[derive(Debug)]
pub enum RuntimeError {
    InvalidRequest(String),
    Conflict(ControllerError),
    CommandConflict {
        command: &'static str,
        state: StateKind,
    },
    DataConflict(String),
    Port(PortError),
    EventSequenceExhausted,
}

impl RuntimeError {
    #[must_use]
    pub fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::InvalidRequest(message) => {
                ProtocolError::new(ErrorCode::InvalidRequest, message.clone())
            }
            Self::Conflict(error) => ProtocolError::new(ErrorCode::Conflict, error.to_string()),
            Self::CommandConflict { command, state } => ProtocolError::new(
                ErrorCode::Conflict,
                format!("cannot {command} while application is {state}"),
            ),
            Self::DataConflict(message) => ProtocolError::new(ErrorCode::Conflict, message.clone()),
            Self::Port(error) => error.protocol_error(),
            Self::EventSequenceExhausted => {
                ProtocolError::new(ErrorCode::Internal, "runtime event sequence exhausted")
            }
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::DataConflict(message) => {
                formatter.write_str(message)
            }
            Self::Conflict(error) => error.fmt(formatter),
            Self::CommandConflict { command, state } => {
                write!(formatter, "cannot {command} while application is {state}")
            }
            Self::Port(error) => error.fmt(formatter),
            Self::EventSequenceExhausted => formatter.write_str("runtime event sequence exhausted"),
        }
    }
}

impl Error for RuntimeError {}

impl From<ControllerError> for RuntimeError {
    fn from(error: ControllerError) -> Self {
        Self::Conflict(error)
    }
}

impl From<PortError> for RuntimeError {
    fn from(error: PortError) -> Self {
        Self::Port(error)
    }
}
