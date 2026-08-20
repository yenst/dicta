use std::{fmt, io};

#[derive(Debug)]
pub enum CaptureError {
    MissingTool(&'static str),
    CommandIo {
        program: String,
        source: io::Error,
    },
    CommandFailed {
        program: String,
        code: Option<i32>,
        stderr: String,
    },
    InvalidResponse {
        program: &'static str,
        detail: String,
    },
    InvalidConfiguration(String),
    OutputNotFound(String),
    AudioSourceNotFound(String),
    MissingMixedSource(String),
    AlreadyRecording,
    NotRecording,
    RecorderExited {
        code: Option<i32>,
    },
    StopTimedOut,
    OutputMissing(String),
    DestinationExists(String),
    StagingExists(String),
    FinalizeIo {
        action: &'static str,
        path: String,
        source: io::Error,
    },
}

impl CaptureError {
    pub(crate) fn command_io(program: impl Into<String>, source: io::Error) -> Self {
        Self::CommandIo {
            program: program.into(),
            source,
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTool(tool) => {
                write!(formatter, "required capture tool `{tool}` is missing")
            }
            Self::CommandIo { program, source } => {
                write!(formatter, "could not execute `{program}`: {source}")
            }
            Self::CommandFailed {
                program,
                code,
                stderr,
            } => write!(
                formatter,
                "`{program}` failed with status {code:?}: {}",
                stderr.trim()
            ),
            Self::InvalidResponse { program, detail } => {
                write!(formatter, "could not parse `{program}` output: {detail}")
            }
            Self::InvalidConfiguration(detail) => {
                write!(formatter, "invalid capture config: {detail}")
            }
            Self::OutputNotFound(name) => {
                write!(formatter, "capture output `{name}` was not found")
            }
            Self::AudioSourceNotFound(name) => {
                write!(formatter, "audio source `{name}` was not found")
            }
            Self::MissingMixedSource(name) => write!(
                formatter,
                "combined microphone/system PipeWire source `{name}` is unavailable"
            ),
            Self::AlreadyRecording => formatter.write_str("a capture is already running"),
            Self::NotRecording => formatter.write_str("no capture is running"),
            Self::RecorderExited { code } => {
                write!(
                    formatter,
                    "capture process exited unexpectedly with status {code:?}"
                )
            }
            Self::StopTimedOut => formatter.write_str("capture process did not stop cleanly"),
            Self::OutputMissing(path) => {
                write!(formatter, "capture process did not produce `{path}`")
            }
            Self::DestinationExists(path) => {
                write!(formatter, "refusing to replace existing recording `{path}`")
            }
            Self::StagingExists(path) => {
                write!(formatter, "capture staging file already exists at `{path}`")
            }
            Self::FinalizeIo {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} `{path}`: {source}"),
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CommandIo { source, .. } | Self::FinalizeIo { source, .. } => Some(source),
            _ => None,
        }
    }
}
