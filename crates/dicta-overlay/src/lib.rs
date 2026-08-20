//! Resolution-independent recording annotation sessions and persistence.

use dicta_core::{
    storage::{annotation_sidecar_path, write_json_atomic},
    AnnotationCanvas, AnnotationEvent, AnnotationFile, AnnotationId, AnnotationStyle,
    AnnotationTool, NormalizedPoint, RecordingId,
};
use serde_json::{Map, Value};
use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

/// Whether the overlay lets input reach applications underneath it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionMode {
    /// The compositor should route input to the application below the overlay.
    PassThrough,
    /// The overlay captures pointer input to create annotations.
    Annotating,
}

/// Terminal lifecycle state for one recording's annotation session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    /// The session accepts edits.
    Open,
    /// The session has persisted its sidecar.
    Finalized,
    /// The session discarded all in-memory events.
    Aborted,
}

/// The eight output transforms defined by Wayland and exposed by wlroots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTransform {
    /// No transform.
    Normal,
    /// 90-degree rotation.
    Rotated90,
    /// 180-degree rotation.
    Rotated180,
    /// 270-degree rotation.
    Rotated270,
    /// Horizontal flip.
    Flipped,
    /// Horizontal flip combined with a 90-degree rotation.
    Flipped90,
    /// Horizontal flip combined with a 180-degree rotation.
    Flipped180,
    /// Horizontal flip combined with a 270-degree rotation.
    Flipped270,
}

impl OutputTransform {
    /// Returns the stable name stored in the annotation sidecar.
    #[must_use]
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Rotated90 => "rotated_90",
            Self::Rotated180 => "rotated_180",
            Self::Rotated270 => "rotated_270",
            Self::Flipped => "flipped",
            Self::Flipped90 => "flipped_90",
            Self::Flipped180 => "flipped_180",
            Self::Flipped270 => "flipped_270",
        }
    }

    const fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Rotated90 | Self::Rotated270 | Self::Flipped90 | Self::Flipped270
        )
    }

    fn to_recording_space(self, point: NormalizedPoint) -> NormalizedPoint {
        match self {
            Self::Normal => point,
            Self::Rotated90 => NormalizedPoint {
                x: point.y,
                y: 1.0 - point.x,
            },
            Self::Rotated180 => NormalizedPoint {
                x: 1.0 - point.x,
                y: 1.0 - point.y,
            },
            Self::Rotated270 => NormalizedPoint {
                x: 1.0 - point.y,
                y: point.x,
            },
            Self::Flipped => NormalizedPoint {
                x: 1.0 - point.x,
                y: point.y,
            },
            Self::Flipped90 => NormalizedPoint {
                x: 1.0 - point.y,
                y: 1.0 - point.x,
            },
            Self::Flipped180 => NormalizedPoint {
                x: point.x,
                y: 1.0 - point.y,
            },
            Self::Flipped270 => NormalizedPoint {
                x: point.y,
                y: point.x,
            },
        }
    }
}

/// A pointer position in the overlay window's logical coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePoint {
    /// Logical horizontal coordinate.
    pub x: f64,
    /// Logical vertical coordinate.
    pub y: f64,
}

/// Converts fractional-scale compositor coordinates into recording coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceMapping {
    canvas: AnnotationCanvas,
    transform: OutputTransform,
}

impl SurfaceMapping {
    /// Builds a mapping for one capture output.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayError::InvalidCanvas`] if dimensions or scale are invalid.
    pub fn new(canvas: AnnotationCanvas, transform: OutputTransform) -> Result<Self, OverlayError> {
        if !canvas.is_valid() {
            return Err(OverlayError::InvalidCanvas);
        }
        Ok(Self { canvas, transform })
    }

    /// Returns the transformed output size in compositor logical pixels.
    #[must_use]
    pub fn logical_size(&self) -> (f64, f64) {
        let width = f64::from(self.canvas.width_pixels) / f64::from(self.canvas.scale);
        let height = f64::from(self.canvas.height_pixels) / f64::from(self.canvas.scale);
        if self.transform.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Maps a logical surface point into normalized recording coordinates.
    ///
    /// Points outside the surface are clamped to its edge.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayError::InvalidPoint`] for non-finite coordinates.
    pub fn map(&self, point: SurfacePoint) -> Result<NormalizedPoint, OverlayError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(OverlayError::InvalidPoint);
        }
        let (width, height) = self.logical_size();
        #[allow(clippy::cast_possible_truncation)]
        let surface_point = NormalizedPoint {
            x: (point.x / width).clamp(0.0, 1.0) as f32,
            y: (point.y / height).clamp(0.0, 1.0) as f32,
        };
        Ok(self.transform.to_recording_space(surface_point))
    }

    /// Returns the physical recording canvas.
    #[must_use]
    pub fn canvas(&self) -> &AnnotationCanvas {
        &self.canvas
    }

    /// Returns the transform applied by this mapping.
    #[must_use]
    pub const fn transform(&self) -> OutputTransform {
        self.transform
    }
}

#[derive(Clone, Debug)]
struct ActiveAnnotation {
    event: AnnotationEvent,
}

#[derive(Clone, Debug)]
struct FinalizedSession {
    path: PathBuf,
    file: AnnotationFile,
}

/// Owns all annotation state for exactly one recording.
#[derive(Clone, Debug)]
pub struct AnnotationSession {
    file: AnnotationFile,
    mapping: SurfaceMapping,
    mode: InteractionMode,
    status: SessionStatus,
    active: Option<ActiveAnnotation>,
    last_time: Duration,
    next_id: u64,
    finalized: Option<FinalizedSession>,
}

impl AnnotationSession {
    /// Creates a pass-through annotation session.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayError::InvalidCanvas`] if the output geometry is invalid.
    pub fn new(
        recording_id: RecordingId,
        mut canvas: AnnotationCanvas,
        transform: OutputTransform,
    ) -> Result<Self, OverlayError> {
        canvas.extra.insert(
            "output_transform".to_string(),
            Value::String(transform.persisted_name().to_string()),
        );
        let mapping = SurfaceMapping::new(canvas.clone(), transform)?;
        Ok(Self {
            file: AnnotationFile::new(recording_id, canvas),
            mapping,
            mode: InteractionMode::PassThrough,
            status: SessionStatus::Open,
            active: None,
            last_time: Duration::ZERO,
            next_id: 1,
            finalized: None,
        })
    }

    /// Returns the current input mode.
    #[must_use]
    pub const fn mode(&self) -> InteractionMode {
        self.mode
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn status(&self) -> SessionStatus {
        self.status
    }

    /// Returns committed events in display order.
    #[must_use]
    pub fn events(&self) -> &[AnnotationEvent] {
        &self.file.events
    }

    /// Returns the coordinate mapping for this recording.
    #[must_use]
    pub fn mapping(&self) -> &SurfaceMapping {
        &self.mapping
    }

    /// Changes whether the overlay captures input.
    ///
    /// Switching to pass-through cancels an in-progress annotation.
    ///
    /// # Errors
    ///
    /// Returns an error after the session is finalized or aborted.
    pub fn set_mode(&mut self, mode: InteractionMode) -> Result<(), OverlayError> {
        self.ensure_open()?;
        if mode == InteractionMode::PassThrough {
            self.active = None;
        }
        self.mode = mode;
        Ok(())
    }

    /// Starts a new annotation at a recording-relative time.
    ///
    /// # Errors
    ///
    /// Returns an error when input is passing through, another annotation is active,
    /// inputs are invalid, time moves backwards, or the session is terminal.
    pub fn begin(
        &mut self,
        tool: AnnotationTool,
        point: SurfacePoint,
        style: AnnotationStyle,
        at: Duration,
    ) -> Result<AnnotationId, OverlayError> {
        self.ensure_can_draw()?;
        if self.active.is_some() {
            return Err(OverlayError::AnnotationAlreadyActive);
        }
        if tool == AnnotationTool::Unknown {
            return Err(OverlayError::InvalidTool);
        }
        if !style.is_valid() {
            return Err(OverlayError::InvalidStyle);
        }
        let mapped = self.mapping.map(point)?;
        self.observe_time(at)?;
        let id = AnnotationId::new(format!("annotation-{:06}", self.next_id))
            .map_err(|_| OverlayError::InvalidGeneratedId)?;
        self.next_id += 1;
        self.active = Some(ActiveAnnotation {
            event: AnnotationEvent {
                id: id.clone(),
                tool,
                started_at_seconds: at.as_secs_f64(),
                ended_at_seconds: None,
                style,
                points: vec![mapped],
                extra: Map::default(),
            },
        });
        Ok(id)
    }

    /// Extends the in-progress annotation.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active annotation, input is passing through,
    /// the point is invalid, time moves backwards, or the session is terminal.
    pub fn update(&mut self, point: SurfacePoint, at: Duration) -> Result<(), OverlayError> {
        self.ensure_can_draw()?;
        if self.active.is_none() {
            return Err(OverlayError::NoActiveAnnotation);
        }
        let mapped = self.mapping.map(point)?;
        self.observe_time(at)?;
        let active = self
            .active
            .as_mut()
            .ok_or(OverlayError::NoActiveAnnotation)?;
        if active.event.tool == AnnotationTool::Pen || active.event.points.len() == 1 {
            active.event.points.push(mapped);
        } else {
            active.event.points[1] = mapped;
        }
        Ok(())
    }

    /// Completes and commits the active annotation.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::update`], or an invalid-annotation error.
    pub fn finish(
        &mut self,
        point: SurfacePoint,
        at: Duration,
    ) -> Result<AnnotationId, OverlayError> {
        self.update(point, at)?;
        let mut event = self
            .active
            .take()
            .ok_or(OverlayError::NoActiveAnnotation)?
            .event;
        event.ended_at_seconds = Some(at.as_secs_f64());
        if !event.is_valid() {
            return Err(OverlayError::InvalidAnnotation);
        }
        let id = event.id.clone();
        self.file.events.push(event);
        Ok(id)
    }

    /// Cancels the active annotation without affecting committed events.
    ///
    /// # Errors
    ///
    /// Returns an error after the session is finalized or aborted.
    pub fn cancel_active(&mut self) -> Result<bool, OverlayError> {
        self.ensure_open()?;
        Ok(self.active.take().is_some())
    }

    /// Removes and returns the most recently committed event.
    ///
    /// # Errors
    ///
    /// Returns an error while drawing or after the session is terminal.
    pub fn undo(&mut self) -> Result<Option<AnnotationEvent>, OverlayError> {
        self.ensure_open()?;
        if self.active.is_some() {
            return Err(OverlayError::AnnotationAlreadyActive);
        }
        Ok(self.file.events.pop())
    }

    /// Cancels active input and clears every committed event.
    ///
    /// # Errors
    ///
    /// Returns an error after the session is finalized or aborted.
    pub fn clear(&mut self) -> Result<usize, OverlayError> {
        self.ensure_open()?;
        self.active = None;
        let count = self.file.events.len();
        self.file.events.clear();
        Ok(count)
    }

    /// Atomically writes the conventional sidecar beside recording metadata.
    ///
    /// # Errors
    ///
    /// Returns an error while drawing, after abort, for an invalid document, or if
    /// persistence fails.
    pub fn finalize_for_metadata(
        &mut self,
        metadata_path: &Path,
    ) -> Result<AnnotationFile, OverlayError> {
        self.finalize_to(&annotation_sidecar_path(metadata_path))
    }

    /// Atomically writes the sidecar to an explicit path.
    ///
    /// Repeating the same finalization is idempotent; using another path is rejected.
    ///
    /// # Errors
    ///
    /// Returns an error while drawing, after abort, for an invalid document, or if
    /// persistence fails.
    pub fn finalize_to(&mut self, path: &Path) -> Result<AnnotationFile, OverlayError> {
        match self.status {
            SessionStatus::Finalized => {
                let finalized = self
                    .finalized
                    .as_ref()
                    .ok_or(OverlayError::MissingFinalizedResult)?;
                if finalized.path == path {
                    return Ok(finalized.file.clone());
                }
                return Err(OverlayError::AlreadyFinalized(finalized.path.clone()));
            }
            SessionStatus::Aborted => return Err(OverlayError::SessionAborted),
            SessionStatus::Open => {}
        }
        if self.active.is_some() {
            return Err(OverlayError::AnnotationAlreadyActive);
        }
        if !self.file.is_valid() {
            return Err(OverlayError::InvalidAnnotationFile);
        }
        write_json_atomic(path, &self.file).map_err(OverlayError::Persistence)?;
        self.status = SessionStatus::Finalized;
        self.mode = InteractionMode::PassThrough;
        self.finalized = Some(FinalizedSession {
            path: path.to_path_buf(),
            file: self.file.clone(),
        });
        Ok(self.file.clone())
    }

    /// Discards in-memory annotations and makes the session terminal.
    ///
    /// Repeated aborts are idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`OverlayError::SessionFinalized`] if already persisted.
    pub fn abort(&mut self) -> Result<(), OverlayError> {
        match self.status {
            SessionStatus::Aborted => return Ok(()),
            SessionStatus::Finalized => return Err(OverlayError::SessionFinalized),
            SessionStatus::Open => {}
        }
        self.active = None;
        self.file.events.clear();
        self.mode = InteractionMode::PassThrough;
        self.status = SessionStatus::Aborted;
        Ok(())
    }

    fn ensure_open(&self) -> Result<(), OverlayError> {
        match self.status {
            SessionStatus::Open => Ok(()),
            SessionStatus::Finalized => Err(OverlayError::SessionFinalized),
            SessionStatus::Aborted => Err(OverlayError::SessionAborted),
        }
    }

    fn ensure_can_draw(&self) -> Result<(), OverlayError> {
        self.ensure_open()?;
        if self.mode != InteractionMode::Annotating {
            return Err(OverlayError::PassThrough);
        }
        Ok(())
    }

    fn observe_time(&mut self, at: Duration) -> Result<(), OverlayError> {
        if at < self.last_time {
            return Err(OverlayError::NonMonotonicTime {
                previous: self.last_time,
                received: at,
            });
        }
        self.last_time = at;
        Ok(())
    }
}

/// A rejected overlay operation.
#[derive(Debug)]
pub enum OverlayError {
    /// Canvas dimensions or scale are invalid.
    InvalidCanvas,
    /// A pointer coordinate is non-finite.
    InvalidPoint,
    /// Stroke color, opacity, or width is invalid.
    InvalidStyle,
    /// The requested tool is not supported.
    InvalidTool,
    /// A deterministic identifier could not be represented by the core model.
    InvalidGeneratedId,
    /// A completed event failed core validation.
    InvalidAnnotation,
    /// The full sidecar failed core validation.
    InvalidAnnotationFile,
    /// Drawing was attempted while input should pass through.
    PassThrough,
    /// An operation requires no active annotation.
    AnnotationAlreadyActive,
    /// An operation requires an active annotation.
    NoActiveAnnotation,
    /// A timestamp was older than the most recently observed timestamp.
    NonMonotonicTime {
        /// Most recently observed recording time.
        previous: Duration,
        /// Rejected recording time.
        received: Duration,
    },
    /// The operation is invalid after finalization.
    SessionFinalized,
    /// The operation is invalid after abort.
    SessionAborted,
    /// The session was already finalized to another path.
    AlreadyFinalized(PathBuf),
    /// An internal finalized result is missing.
    MissingFinalizedResult,
    /// Atomic persistence failed.
    Persistence(String),
}

impl fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCanvas => formatter.write_str("annotation canvas is invalid"),
            Self::InvalidPoint => formatter.write_str("annotation point is not finite"),
            Self::InvalidStyle => formatter.write_str("annotation style is invalid"),
            Self::InvalidTool => formatter.write_str("annotation tool is unsupported"),
            Self::InvalidGeneratedId => formatter.write_str("could not generate annotation id"),
            Self::InvalidAnnotation => formatter.write_str("completed annotation is invalid"),
            Self::InvalidAnnotationFile => formatter.write_str("annotation file is invalid"),
            Self::PassThrough => formatter.write_str("overlay is in pass-through mode"),
            Self::AnnotationAlreadyActive => formatter.write_str("an annotation is already active"),
            Self::NoActiveAnnotation => formatter.write_str("there is no active annotation"),
            Self::NonMonotonicTime { previous, received } => write!(
                formatter,
                "recording time moved backwards from {previous:?} to {received:?}"
            ),
            Self::SessionFinalized => formatter.write_str("annotation session is finalized"),
            Self::SessionAborted => formatter.write_str("annotation session is aborted"),
            Self::AlreadyFinalized(path) => {
                write!(
                    formatter,
                    "annotation session was finalized to {}",
                    path.display()
                )
            }
            Self::MissingFinalizedResult => {
                formatter.write_str("finalized annotation result is unavailable")
            }
            Self::Persistence(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for OverlayError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use std::{fs, time::SystemTime};

    fn canvas(scale: f32) -> AnnotationCanvas {
        AnnotationCanvas {
            output_name: Some("DP-1".to_string()),
            width_pixels: 3000,
            height_pixels: 2000,
            scale,
            extra: Map::new(),
        }
    }

    fn style() -> AnnotationStyle {
        AnnotationStyle {
            color: "#ffffff".to_string(),
            width: 3.0,
            opacity: 1.0,
            extra: Map::new(),
        }
    }

    fn session(transform: OutputTransform) -> AnnotationSession {
        AnnotationSession::new(
            RecordingId::new("20260820-12-00-00").unwrap(),
            canvas(1.25),
            transform,
        )
        .unwrap()
    }

    #[test]
    fn fractional_scale_and_rotation_map_to_recording_space() {
        let mapping = SurfaceMapping::new(canvas(1.25), OutputTransform::Rotated90).unwrap();
        assert_eq!(mapping.logical_size(), (1600.0, 2400.0));
        assert_eq!(
            mapping.map(SurfacePoint { x: 0.0, y: 0.0 }).unwrap(),
            NormalizedPoint { x: 0.0, y: 1.0 }
        );
        assert_eq!(
            mapping
                .map(SurfacePoint {
                    x: 1600.0,
                    y: 2400.0,
                })
                .unwrap(),
            NormalizedPoint { x: 1.0, y: 0.0 }
        );
    }

    #[test]
    fn drawing_requires_annotation_mode_and_uses_deterministic_ids() {
        let mut session = session(OutputTransform::Normal);
        let point = SurfacePoint { x: 120.0, y: 80.0 };
        assert!(matches!(
            session.begin(AnnotationTool::Pen, point, style(), Duration::ZERO),
            Err(OverlayError::PassThrough)
        ));
        session.set_mode(InteractionMode::Annotating).unwrap();
        let first = session
            .begin(AnnotationTool::Pen, point, style(), Duration::from_secs(1))
            .unwrap();
        session.finish(point, Duration::from_millis(1250)).unwrap();
        let second = session
            .begin(
                AnnotationTool::Rectangle,
                point,
                style(),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(first.as_str(), "annotation-000001");
        assert_eq!(second.as_str(), "annotation-000002");
        session.cancel_active().unwrap();
        assert_eq!(session.undo().unwrap().unwrap().id, first);
        assert!(session.events().is_empty());
    }

    #[test]
    fn time_must_be_monotonic() {
        let mut session = session(OutputTransform::Normal);
        session.set_mode(InteractionMode::Annotating).unwrap();
        session
            .begin(
                AnnotationTool::Arrow,
                SurfacePoint { x: 0.0, y: 0.0 },
                style(),
                Duration::from_secs(4),
            )
            .unwrap();
        assert!(matches!(
            session.update(SurfacePoint { x: 1.0, y: 1.0 }, Duration::from_secs(3)),
            Err(OverlayError::NonMonotonicTime { .. })
        ));
    }

    #[test]
    fn clear_cancels_active_input_and_removes_committed_events() {
        let mut session = session(OutputTransform::Normal);
        session.set_mode(InteractionMode::Annotating).unwrap();
        let point = SurfacePoint { x: 50.0, y: 50.0 };
        session
            .begin(
                AnnotationTool::Pen,
                point,
                style(),
                Duration::from_millis(1),
            )
            .unwrap();
        session.finish(point, Duration::from_millis(2)).unwrap();
        session
            .begin(
                AnnotationTool::Arrow,
                point,
                style(),
                Duration::from_millis(3),
            )
            .unwrap();

        assert_eq!(session.clear().unwrap(), 1);
        assert!(session.events().is_empty());
        assert!(matches!(session.undo(), Ok(None)));
    }

    #[test]
    fn finalize_is_atomic_idempotent_and_uses_the_versioned_sidecar() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "dicta-overlay-test-{}-{unique}",
            std::process::id()
        ));
        let metadata = directory.join("recording.json");
        let sidecar = directory.join("recording.annotations.json");
        let mut session = session(OutputTransform::Rotated270);
        session.set_mode(InteractionMode::Annotating).unwrap();
        session
            .begin(
                AnnotationTool::Spotlight,
                SurfacePoint { x: 20.0, y: 20.0 },
                style(),
                Duration::from_millis(50),
            )
            .unwrap();
        session
            .finish(
                SurfacePoint { x: 100.0, y: 80.0 },
                Duration::from_millis(75),
            )
            .unwrap();

        let first = session.finalize_for_metadata(&metadata).unwrap();
        let second = session.finalize_for_metadata(&metadata).unwrap();
        assert_eq!(first, second);
        assert_eq!(session.status(), SessionStatus::Finalized);
        assert_eq!(
            first.canvas.extra["output_transform"],
            Value::String("rotated_270".to_string())
        );
        assert_eq!(
            dicta_core::storage::read_json::<AnnotationFile>(&sidecar).unwrap(),
            first
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn abort_is_idempotent_and_never_persists() {
        let mut session = session(OutputTransform::Normal);
        session.abort().unwrap();
        session.abort().unwrap();
        assert_eq!(session.status(), SessionStatus::Aborted);
        assert!(matches!(
            session.finalize_to(Path::new("annotations.json")),
            Err(OverlayError::SessionAborted)
        ));
    }

    #[test]
    fn every_wlroots_flip_transform_has_an_explicit_mapping() {
        let point = SurfacePoint {
            x: 400.0,
            y: 1200.0,
        };
        let cases = [
            (OutputTransform::Flipped, (0.833_333_3, 0.75)),
            (OutputTransform::Flipped90, (0.5, 0.75)),
            (OutputTransform::Flipped180, (0.166_666_67, 0.25)),
            (OutputTransform::Flipped270, (0.5, 0.25)),
        ];
        for (transform, expected) in cases {
            let mapped = SurfaceMapping::new(canvas(1.25), transform)
                .unwrap()
                .map(point)
                .unwrap();
            assert!((mapped.x - expected.0).abs() < 0.000_01, "{transform:?}");
            assert!((mapped.y - expected.1).abs() < 0.000_01, "{transform:?}");
        }
    }
}
