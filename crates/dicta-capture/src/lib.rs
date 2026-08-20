#![forbid(unsafe_code)]

//! Native Omarchy/Hyprland screen and audio capture primitives.

mod command;
mod discovery;
mod error;
mod plan;
mod recorder;

pub use command::{
    CaptureChild, CommandOutput, CommandPlan, Platform, ProcessExit, SystemPlatform,
};
pub use discovery::{
    discover, AudioSource, AudioSourceKind, CaptureCapabilities, CaptureOutput, Geometry,
    OutputTransform, SessionEnvironment, SessionKind, ToolCapabilities,
};
pub use error::CaptureError;
pub use plan::{
    capture_plan, gpu_screen_recorder_plan, wf_recorder_plan, AudioSelection, CaptureArea,
    CaptureBackend, CaptureConfig, CapturePlan, CaptureTarget, GpuScreenRecorderPlan,
    WfRecorderPlan,
};
pub use recorder::{CaptureArtifact, PollOutcome, Recorder, StopReason, MAX_RECORDING_DURATION};
