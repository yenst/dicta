use crate::{
    codec::{read_frame, write_frame, CodecError},
    Command, EventEnvelope, ProtocolError, RequestEnvelope, RequestId, Response, ResponseEnvelope,
    ResponsePayload, ServerMessage, PROTOCOL_VERSION,
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    fmt, fs,
    io::{self, BufReader, Cursor, Read},
    num::NonZeroU64,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
};

pub const SOCKET_FILE_NAME: &str = "control-v1.sock";

#[derive(Debug)]
pub enum ControlError {
    Io(io::Error),
    Codec(CodecError),
    Remote(ProtocolError),
    Security(String),
    Protocol(String),
    Disconnected,
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "local control I/O failed: {error}"),
            Self::Codec(error) => error.fmt(formatter),
            Self::Remote(error) => error.fmt(formatter),
            Self::Security(message) => write!(formatter, "unsafe control socket: {message}"),
            Self::Protocol(message) => write!(formatter, "control protocol violation: {message}"),
            Self::Disconnected => formatter.write_str("Dicta control socket disconnected"),
        }
    }
}

impl Error for ControlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::Remote(error) => Some(error),
            Self::Security(_) | Self::Protocol(_) | Self::Disconnected => None,
        }
    }
}

impl From<io::Error> for ControlError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CodecError> for ControlError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

pub fn default_socket_path() -> Result<PathBuf, ControlError> {
    let runtime_root = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", effective_user_id())));
    if !runtime_root.is_absolute() {
        return Err(ControlError::Security(
            "XDG_RUNTIME_DIR must be an absolute path".to_string(),
        ));
    }
    Ok(runtime_root.join("dicta").join(SOCKET_FILE_NAME))
}

pub fn effective_user_id() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Create or tighten the directory used to contain the socket. Symlinks and
/// directories owned by another user are rejected.
pub fn ensure_private_runtime_dir(path: &Path) -> Result<(), ControlError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ControlError::Security(format!(
                    "{} is not a real directory",
                    path.display()
                )));
            }
            if metadata.uid() != effective_user_id() {
                return Err(ControlError::Security(format!(
                    "{} is owned by uid {}",
                    path.display(),
                    metadata.uid()
                )));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn validate_private_socket(path: &Path) -> Result<(), ControlError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(ControlError::Security(format!(
            "{} is not a Unix socket",
            path.display()
        )));
    }
    if metadata.uid() != effective_user_id() {
        return Err(ControlError::Security(format!(
            "{} is owned by uid {}",
            path.display(),
            metadata.uid()
        )));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(ControlError::Security(format!(
            "{} is accessible by another user",
            path.display()
        )));
    }
    Ok(())
}

pub struct LocalServer {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
}

impl LocalServer {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, ControlError> {
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            ControlError::Security("socket path has no parent directory".to_string())
        })?;
        ensure_private_runtime_dir(parent)?;
        if fs::symlink_metadata(path).is_ok() {
            return Err(ControlError::Io(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{} already exists", path.display()),
            )));
        }
        let listener = UnixListener::bind(path)?;
        if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(path);
            return Err(error.into());
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = fs::remove_file(path);
                return Err(error.into());
            }
        };
        let identity = (metadata.dev(), metadata.ino());
        Ok(Self {
            listener,
            path: path.to_path_buf(),
            identity,
        })
    }

    pub fn accept(&self) -> Result<ServerConnection, ControlError> {
        let (stream, _) = self.listener.accept()?;
        ServerConnection::new(stream)
    }

    /// Changes whether accepts wait for a client. This only affects the listener;
    /// accepted connections are explicitly returned to blocking mode.
    ///
    /// # Errors
    /// Returns an I/O error when the listener flags cannot be changed.
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), ControlError> {
        self.listener.set_nonblocking(nonblocking)?;
        Ok(())
    }

    /// Enables nonblocking mode and attempts one accept. `WouldBlock` is
    /// represented as `Ok(None)`, avoiding both accidental blocking and
    /// error-driven polling. Accepted connections are returned to blocking mode.
    ///
    /// # Errors
    /// Returns an I/O error for failures other than the absence of a pending client.
    pub fn try_accept(&self) -> Result<Option<ServerConnection>, ControlError> {
        self.listener.set_nonblocking(true)?;
        match self.listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                Ok(Some(ServerConnection::new(stream)?))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let still_ours = fs::symlink_metadata(&self.path)
            .map(|metadata| (metadata.dev(), metadata.ino()) == self.identity)
            .unwrap_or(false);
        if still_ours {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub struct ServerConnection {
    reader: UnixStream,
    writer: UnixStream,
    pending: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequestPoll {
    Request(RequestEnvelope),
    Pending,
    Closed,
}

impl ServerConnection {
    fn new(stream: UnixStream) -> Result<Self, ControlError> {
        Ok(Self {
            writer: stream.try_clone()?,
            reader: stream,
            pending: Vec::new(),
        })
    }

    pub fn read_request(&mut self) -> Result<Option<RequestEnvelope>, ControlError> {
        self.reader.set_nonblocking(false)?;
        loop {
            if let Some(request) = self.take_request(false)? {
                return Ok(Some(request));
            }
            let mut bytes = [0_u8; 8192];
            match self.reader.read(&mut bytes) {
                Ok(0) => return self.take_request(true),
                Ok(read) => self.append(&bytes[..read])?,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Polls one request without blocking and retains partial frames for the next
    /// call. EOF with a final non-empty frame remains valid, matching `read_frame`.
    ///
    /// # Errors
    /// Returns the shared codec errors for empty, oversized, or malformed frames,
    /// and I/O errors other than `WouldBlock`.
    pub fn poll_request(&mut self) -> Result<RequestPoll, ControlError> {
        self.reader.set_nonblocking(true)?;
        let result = (|| loop {
            if let Some(request) = self.take_request(false)? {
                return Ok(RequestPoll::Request(request));
            }
            let mut bytes = [0_u8; 8192];
            match self.reader.read(&mut bytes) {
                Ok(0) => {
                    return Ok(self
                        .take_request(true)?
                        .map_or(RequestPoll::Closed, RequestPoll::Request));
                }
                Ok(read) => self.append(&bytes[..read])?,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(RequestPoll::Pending);
                }
                Err(error) => return Err(error.into()),
            }
        })();
        let restore = self.reader.set_nonblocking(false).map_err(ControlError::Io);
        match result {
            Ok(poll) => {
                restore?;
                Ok(poll)
            }
            Err(error) => {
                let _ = restore;
                Err(error)
            }
        }
    }

    pub fn send_response(&mut self, response: &ResponseEnvelope) -> Result<(), ControlError> {
        write_frame(&mut self.writer, response)?;
        Ok(())
    }

    pub fn send_event(&mut self, event: &EventEnvelope) -> Result<(), ControlError> {
        write_frame(&mut self.writer, event)?;
        Ok(())
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), ControlError> {
        self.pending.extend_from_slice(bytes);
        if self
            .pending
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.pending.len(), |newline| newline)
            > crate::codec::DEFAULT_MAX_FRAME_BYTES
        {
            return Err(CodecError::FrameTooLarge {
                limit: crate::codec::DEFAULT_MAX_FRAME_BYTES,
            }
            .into());
        }
        Ok(())
    }

    fn take_request(&mut self, eof: bool) -> Result<Option<RequestEnvelope>, ControlError> {
        let frame_end = self
            .pending
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|newline| newline + 1)
            .or_else(|| (eof && !self.pending.is_empty()).then_some(self.pending.len()));
        let Some(frame_end) = frame_end else {
            return Ok(None);
        };
        let frame: Vec<_> = self.pending.drain(..frame_end).collect();
        let mut reader = Cursor::new(frame);
        read_frame(&mut reader).map_err(Into::into)
    }
}

pub struct LocalClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: NonZeroU64,
    events: VecDeque<EventEnvelope>,
    responses: VecDeque<ResponseEnvelope>,
}

impl LocalClient {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self, ControlError> {
        let path = path.as_ref();
        validate_private_socket(path)?;
        let stream = UnixStream::connect(path)?;
        Self::from_stream(stream)
    }

    pub fn from_stream(stream: UnixStream) -> Result<Self, ControlError> {
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next_id: NonZeroU64::MIN,
            events: VecDeque::new(),
            responses: VecDeque::new(),
        })
    }

    pub fn send(&mut self, command: Command) -> Result<RequestId, ControlError> {
        let id = RequestId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).unwrap_or(NonZeroU64::MIN);
        write_frame(&mut self.writer, &RequestEnvelope::new(id, command))?;
        Ok(id)
    }

    pub fn request(&mut self, command: Command) -> Result<Response, ControlError> {
        let id = self.send(command)?;
        self.wait(id)
    }

    /// Wait for a previously sent request. Other responses and asynchronous
    /// events are retained, so clients may pipeline commands without losing
    /// correlation.
    pub fn wait(&mut self, id: RequestId) -> Result<Response, ControlError> {
        if let Some(position) = self.responses.iter().position(|response| response.id == id) {
            let response = self
                .responses
                .remove(position)
                .expect("position came from queue");
            return finish_response(response);
        }
        loop {
            match self.read_message()? {
                ServerMessage::Event(event) => self.events.push_back(event),
                ServerMessage::Response(response) if response.id == id => {
                    return finish_response(response);
                }
                ServerMessage::Response(response) => self.responses.push_back(response),
            }
        }
    }

    pub fn read_message(&mut self) -> Result<ServerMessage, ControlError> {
        read_frame(&mut self.reader)?.ok_or(ControlError::Disconnected)
    }

    pub fn pop_event(&mut self) -> Option<EventEnvelope> {
        self.events.pop_front()
    }
}

fn finish_response(response: ResponseEnvelope) -> Result<Response, ControlError> {
    if response.version != PROTOCOL_VERSION {
        return Err(ControlError::Protocol(format!(
            "server returned version {}",
            response.version
        )));
    }
    match response.payload {
        ResponsePayload::Success { result } => Ok(result),
        ResponsePayload::Failure { error } => Err(ControlError::Remote(error)),
    }
}
