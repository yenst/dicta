pub mod annotations;
pub mod branch;
pub mod git;
mod ids;
mod models;
pub mod storage;
pub mod transcript;

pub use annotations::{
    AnnotationCanvas, AnnotationEvent, AnnotationFile, AnnotationStyle, AnnotationTool,
    NormalizedPoint, ANNOTATION_FORMAT_VERSION,
};
pub use ids::{AnnotationId, InvalidId, ProjectId, RecordingId};
pub use models::{
    BranchMetadata, ProjectFile, RecordingFile, RecordingScope, TimelineNote, TranscriptSegment,
    TranscriptionStatus,
};

pub const GENERAL_PROJECT_ID: &str = "__unprojected__";
