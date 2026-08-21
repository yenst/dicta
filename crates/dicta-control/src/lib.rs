//! Shared local-control protocol for Dicta.
//!
//! The crate deliberately uses blocking standard-library I/O. Short-lived CLI
//! clients and the native application's control thread do not need an async
//! runtime, and keeping the protocol UI-independent lets every frontend share
//! the same command surface.

#![forbid(unsafe_code)]

pub mod cli;
pub mod codec;
pub mod error;
pub mod protocol;

#[cfg(unix)]
pub mod socket;

pub use error::{ErrorCode, ExitCode, ProtocolError};
pub use protocol::{
    AnnotationTool, CleanupSummary, Command, Event, EventEnvelope, ModelInstallStage, ModelState,
    ModelStatusSummary, ModelTier, ProjectSummary, RecordingSelector, RecordingSummary,
    RequestEnvelope, RequestId, Response, ResponseEnvelope, ResponsePayload, ServerMessage,
    VoiceNoteState, VoiceNoteStatus, PROTOCOL_VERSION,
};
