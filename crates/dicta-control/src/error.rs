use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// Stable error identifiers exposed on the wire.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    NotFound,
    Conflict,
    Unavailable,
    PermissionDenied,
    Internal,
}

/// A serializable failure. `details` is intended for structured diagnostics,
/// while clients may safely branch on `code`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub const fn exit_code(&self) -> ExitCode {
        self.code.exit_code()
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl Error for ProtocolError {}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnsupportedVersion => "unsupported_version",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::Internal => "internal",
        }
    }

    /// Stable process exit-code mapping for the command-line client.
    pub const fn exit_code(self) -> ExitCode {
        match self {
            Self::InvalidRequest | Self::UnsupportedVersion => ExitCode::Usage,
            Self::NotFound => ExitCode::NotFound,
            Self::Conflict => ExitCode::Conflict,
            Self::Unavailable => ExitCode::Unavailable,
            Self::PermissionDenied => ExitCode::PermissionDenied,
            Self::Internal => ExitCode::Software,
        }
    }
}

/// Stable CLI exit codes. Values follow `sysexits.h` where it has an
/// appropriate category; conflict has a dedicated application code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    Usage = 64,
    NotFound = 66,
    Unavailable = 69,
    Software = 70,
    PermissionDenied = 77,
    Conflict = 78,
}

impl ExitCode {
    pub const fn get(self) -> u8 {
        self as u8
    }
}
