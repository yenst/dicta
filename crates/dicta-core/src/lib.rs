pub mod branch;
pub mod git;
mod ids;
mod models;
pub mod storage;
pub mod transcript;

pub use ids::{InvalidId, ProjectId, RecordingId};
pub use models::{
    BranchMetadata, ProjectFile, RecordingScope, TranscriptSegment, TranscriptionStatus,
};

pub const GENERAL_PROJECT_ID: &str = "__unprojected__";
