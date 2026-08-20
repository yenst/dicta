use serde::{de::DeserializeOwned, Serialize};
use std::{
    error::Error,
    fmt,
    io::{self, BufRead, Read, Write},
};

pub const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub enum CodecError {
    Io(io::Error),
    Json(serde_json::Error),
    EmptyFrame,
    FrameTooLarge { limit: usize },
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "control transport I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "invalid control frame: {error}"),
            Self::EmptyFrame => formatter.write_str("control frame is empty"),
            Self::FrameTooLarge { limit } => {
                write!(formatter, "control frame exceeds the {limit}-byte limit")
            }
        }
    }
}

impl Error for CodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::EmptyFrame | Self::FrameTooLarge { .. } => None,
        }
    }
}

impl From<io::Error> for CodecError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for CodecError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Read one newline-delimited JSON frame. EOF before any bytes is clean end of
/// stream; a final non-empty frame without a trailing newline is accepted.
pub fn read_frame<R, T>(reader: &mut R) -> Result<Option<T>, CodecError>
where
    R: BufRead,
    T: DeserializeOwned,
{
    read_frame_with_limit(reader, DEFAULT_MAX_FRAME_BYTES)
}

pub fn read_frame_with_limit<R, T>(reader: &mut R, limit: usize) -> Result<Option<T>, CodecError>
where
    R: BufRead,
    T: DeserializeOwned,
{
    let mut bytes = Vec::new();
    let read = (&mut *reader)
        .take((limit + 2) as u64)
        .read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > limit + 1 || (bytes.len() == limit + 1 && bytes.last() != Some(&b'\n')) {
        return Err(CodecError::FrameTooLarge { limit });
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.is_empty() {
        return Err(CodecError::EmptyFrame);
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

/// Write one compact JSON value followed by exactly one newline.
pub fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), CodecError>
where
    W: Write,
    T: Serialize,
{
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
