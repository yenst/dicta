//! Deterministic integration boundary for Dicta's native frontends.

#![forbid(unsafe_code)]

#[cfg(unix)]
pub mod service;

mod error;
mod ports;
mod runtime;

pub use error::{ControlOutput, RuntimeConfig, RuntimeError, RuntimeSnapshot};
pub use ports::{
    AnnotationPort, CapturePoll, CapturePort, Clock, Completion, IdSource, ModelInstallPoll,
    PortError, PortErrorKind, StoragePort, TranscriptionCompletion, TranscriptionPort,
};
pub(crate) use runtime::event_sequence;
pub use runtime::{Runtime, MAX_RETAINED_EVENTS};

use std::path::PathBuf;

/// Returns the private per-user directory used only for short-lived voice-note
/// capture files.
///
/// # Errors
/// Returns a permission/security error when the runtime directory is not a
/// private real directory owned by the current user.
#[cfg(unix)]
pub fn voice_note_directory() -> Result<PathBuf, PortError> {
    let socket = dicta_control::socket::default_socket_path().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve the private Dicta runtime directory: {error}"),
        )
    })?;
    let directory = socket
        .parent()
        .ok_or_else(|| {
            PortError::new(
                PortErrorKind::PermissionDenied,
                "Dicta control socket has no private runtime directory",
            )
        })?
        .join("voice-notes");
    dicta_control::socket::ensure_private_runtime_dir(&directory).map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not prepare private voice-note storage: {error}"),
        )
    })?;
    Ok(directory)
}

#[cfg(test)]
mod tests;
