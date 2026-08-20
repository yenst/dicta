use crate::{
    command::CommandPlan,
    discovery::{
        AudioSourceKind, CaptureCapabilities, CaptureOutput, Geometry, OutputTransform, SessionKind,
    },
    error::CaptureError,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureTarget {
    pub output_name: String,
    pub geometry: Geometry,
    pub scale: f64,
    pub pixel_size: (u32, u32),
    pub transform: OutputTransform,
}

/// The compositor-space area supplied to the recorder backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureArea {
    /// Capture the complete selected output by its native connector name.
    Monitor,
    /// Capture a logical Hyprland region contained by the selected output.
    LogicalRegion(Geometry),
    /// Ask xdg-desktop-portal to select a monitor or window interactively.
    Portal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureBackend {
    GpuScreenRecorder,
    WfRecorder,
}

impl From<&CaptureOutput> for CaptureTarget {
    fn from(output: &CaptureOutput) -> Self {
        Self {
            output_name: output.name.clone(),
            geometry: output.geometry,
            scale: output.scale,
            pixel_size: output.pixel_size,
            transform: output.transform,
        }
    }
}

impl CaptureTarget {
    #[must_use]
    pub const fn encoded_pixel_size(&self) -> (u32, u32) {
        self.pixel_size
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioSelection {
    None,
    Default,
    Microphone {
        source_name: String,
    },
    System {
        source_name: String,
    },
    /// A pre-provisioned `PipeWire` source that combines microphone and system audio.
    Mixed {
        source_name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureConfig {
    pub target: CaptureTarget,
    pub area: CaptureArea,
    pub audio: AudioSelection,
    pub destination: PathBuf,
    pub staging_destination: PathBuf,
    pub frame_rate: u16,
}

impl CaptureConfig {
    #[must_use]
    pub fn new(
        output: &CaptureOutput,
        audio: AudioSelection,
        destination: impl Into<PathBuf>,
    ) -> Self {
        let destination = destination.into();
        Self {
            target: CaptureTarget::from(output),
            area: CaptureArea::Monitor,
            audio,
            staging_destination: staging_path(
                &destination,
                NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed),
            ),
            destination,
            frame_rate: 60,
        }
    }

    #[must_use]
    pub fn with_logical_region(mut self, region: Geometry) -> Self {
        self.area = CaptureArea::LogicalRegion(region);
        self
    }

    #[must_use]
    pub fn with_portal(mut self) -> Self {
        self.area = CaptureArea::Portal;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuScreenRecorderPlan {
    pub command: CommandPlan,
    pub output_name: String,
    pub geometry: Geometry,
    /// Hyprland output scale retained for overlay-to-video coordinate mapping.
    pub scale_milli: u32,
    pub encoded_pixel_size: (u32, u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WfRecorderPlan {
    pub command: CommandPlan,
    pub output_name: String,
    pub geometry: Geometry,
    /// Hyprland output scale retained for overlay-to-video coordinate mapping.
    pub scale_milli: u32,
    pub encoded_pixel_size: (u32, u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapturePlan {
    GpuScreenRecorder(GpuScreenRecorderPlan),
    WfRecorder(WfRecorderPlan),
}

impl CapturePlan {
    #[must_use]
    pub const fn backend(&self) -> CaptureBackend {
        match self {
            Self::GpuScreenRecorder(_) => CaptureBackend::GpuScreenRecorder,
            Self::WfRecorder(_) => CaptureBackend::WfRecorder,
        }
    }

    #[must_use]
    pub const fn command(&self) -> &CommandPlan {
        match self {
            Self::GpuScreenRecorder(plan) => &plan.command,
            Self::WfRecorder(plan) => &plan.command,
        }
    }

    #[must_use]
    pub fn output_name(&self) -> &str {
        match self {
            Self::GpuScreenRecorder(plan) => &plan.output_name,
            Self::WfRecorder(plan) => &plan.output_name,
        }
    }

    #[must_use]
    pub const fn geometry(&self) -> Geometry {
        match self {
            Self::GpuScreenRecorder(plan) => plan.geometry,
            Self::WfRecorder(plan) => plan.geometry,
        }
    }

    #[must_use]
    pub const fn scale_milli(&self) -> u32 {
        match self {
            Self::GpuScreenRecorder(plan) => plan.scale_milli,
            Self::WfRecorder(plan) => plan.scale_milli,
        }
    }

    #[must_use]
    pub const fn encoded_pixel_size(&self) -> (u32, u32) {
        match self {
            Self::GpuScreenRecorder(plan) => plan.encoded_pixel_size,
            Self::WfRecorder(plan) => plan.encoded_pixel_size,
        }
    }
}

/// Selects and builds the best available recorder plan deterministically.
///
/// `gpu-screen-recorder` is always preferred when present. `wf-recorder` is
/// the compatibility fallback for monitor and logical-region capture. Portal
/// capture is intentionally opt-in and requires `gpu-screen-recorder`.
///
/// # Errors
///
/// Returns a typed capability, selection, or configuration error when no
/// backend can safely satisfy the requested capture.
pub fn capture_plan(
    capabilities: &CaptureCapabilities,
    config: &CaptureConfig,
) -> Result<CapturePlan, CaptureError> {
    validate_common_capabilities(capabilities)?;
    if capabilities.tools.gpu_screen_recorder {
        return gpu_screen_recorder_plan(capabilities, config).map(CapturePlan::GpuScreenRecorder);
    }
    if matches!(config.area, CaptureArea::Portal) {
        return Err(CaptureError::MissingTool("gpu-screen-recorder"));
    }
    wf_recorder_plan(capabilities, config).map(CapturePlan::WfRecorder)
}

/// Builds a shell-free `gpu-screen-recorder` 6.x invocation using Omarchy's
/// production capture defaults.
///
/// # Errors
///
/// Returns a typed capability, target, or configuration error when capture
/// cannot safely start with the requested settings.
pub fn gpu_screen_recorder_plan(
    capabilities: &CaptureCapabilities,
    config: &CaptureConfig,
) -> Result<GpuScreenRecorderPlan, CaptureError> {
    validate_common_capabilities(capabilities)?;
    if !capabilities.tools.gpu_screen_recorder {
        return Err(CaptureError::MissingTool("gpu-screen-recorder"));
    }
    validate_config(capabilities, config)?;
    let metadata = capture_metadata(config)?;

    let mut command = CommandPlan::new("gpu-screen-recorder");
    match config.area {
        CaptureArea::Monitor => {
            command.push_arg("-w");
            command.push_arg(config.target.output_name.clone());
            command.push_arg("-s");
            command.push_arg("0x0");
        }
        CaptureArea::LogicalRegion(region) => {
            command.push_arg("-w");
            command.push_arg("region");
            command.push_arg("-region");
            command.push_arg(region.gpu_screen_recorder_argument());
        }
        CaptureArea::Portal => {
            command.push_arg("-w");
            command.push_arg("portal");
            command.push_arg("-s");
            command.push_arg("0x0");
        }
    }
    command.push_arg("-k");
    command.push_arg("auto");
    command.push_arg("-f");
    command.push_arg(config.frame_rate.to_string());
    command.push_arg("-fm");
    command.push_arg("cfr");
    command.push_arg("-encoder");
    command.push_arg("gpu");
    command.push_arg("-fallback-cpu-encoding");
    command.push_arg("yes");
    append_gpu_audio_arguments(&mut command, &config.audio);
    command.push_arg("-o");
    command.push_arg(config.staging_destination.as_os_str().to_owned());

    Ok(GpuScreenRecorderPlan {
        command,
        output_name: metadata.output_name,
        geometry: metadata.geometry,
        scale_milli: metadata.scale_milli,
        encoded_pixel_size: metadata.encoded_pixel_size,
    })
}

/// Builds a shell-free `wf-recorder` invocation after validating discovery data.
///
/// # Errors
///
/// Returns a typed capability, selection, or configuration error when capture
/// cannot safely start with the requested settings.
pub fn wf_recorder_plan(
    capabilities: &CaptureCapabilities,
    config: &CaptureConfig,
) -> Result<WfRecorderPlan, CaptureError> {
    validate_common_capabilities(capabilities)?;
    if !capabilities.tools.wf_recorder {
        return Err(CaptureError::MissingTool("wf-recorder"));
    }
    if matches!(config.area, CaptureArea::Portal) {
        return Err(CaptureError::InvalidConfiguration(
            "portal capture requires gpu-screen-recorder".to_string(),
        ));
    }
    validate_config(capabilities, config)?;
    validate_audio(capabilities, &config.audio)?;
    let metadata = capture_metadata(config)?;

    let mut command = CommandPlan::new("wf-recorder")
        .arg("--output")
        .arg(config.target.output_name.clone())
        .arg("--geometry")
        .arg(metadata.geometry.wf_recorder_argument())
        .arg("--codec")
        .arg("libx264")
        .arg("--codec-param")
        .arg("preset=ultrafast")
        .arg("--codec-param")
        .arg("crf=23")
        .arg("--pixel-format")
        .arg("yuv420p")
        .arg("--framerate")
        .arg(config.frame_rate.to_string());
    append_audio_arguments(&mut command, &config.audio);
    command.push_arg("--file");
    command.push_arg(config.staging_destination.as_os_str().to_owned());

    Ok(WfRecorderPlan {
        command,
        output_name: metadata.output_name,
        geometry: metadata.geometry,
        scale_milli: metadata.scale_milli,
        encoded_pixel_size: metadata.encoded_pixel_size,
    })
}

fn staging_path(destination: &Path, id: u64) -> PathBuf {
    let parent = parent_or_dot(destination);
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("recording");
    let process = std::process::id();
    let staged_name = match destination
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if !extension.is_empty() => {
            let stem = destination
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(file_name);
            format!(".{stem}.dicta-{process}-{id}.part.{extension}")
        }
        _ => format!(".{file_name}.dicta-{process}-{id}.part"),
    };
    parent.join(staged_name)
}

fn validate_staging_path(destination: &Path, staging: &Path) -> Result<(), CaptureError> {
    if destination == staging || staging.as_os_str().is_empty() {
        return Err(CaptureError::InvalidConfiguration(
            "staging path must differ from the final destination".to_string(),
        ));
    }
    let destination_parent = parent_or_dot(destination);
    let staging_parent = parent_or_dot(staging);
    if destination_parent != staging_parent {
        return Err(CaptureError::InvalidConfiguration(
            "staging file must be in the destination directory".to_string(),
        ));
    }
    Ok(())
}

fn parent_or_dot(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn validate_common_capabilities(capabilities: &CaptureCapabilities) -> Result<(), CaptureError> {
    if capabilities.session != SessionKind::HyprlandWayland {
        return Err(CaptureError::InvalidConfiguration(
            "capture requires a Hyprland Wayland session".to_string(),
        ));
    }
    for (available, name) in [
        (capabilities.tools.hyprctl, "hyprctl"),
        (capabilities.tools.kill, "kill"),
    ] {
        if !available {
            return Err(CaptureError::MissingTool(name));
        }
    }
    Ok(())
}

fn validate_config(
    capabilities: &CaptureCapabilities,
    config: &CaptureConfig,
) -> Result<(), CaptureError> {
    validate_target(capabilities, &config.target)?;
    if let CaptureArea::LogicalRegion(region) = config.area {
        validate_region(&config.target.geometry, &region)?;
    }
    if config.frame_rate == 0 || config.frame_rate > 240 {
        return Err(CaptureError::InvalidConfiguration(
            "frame rate must be between 1 and 240".to_string(),
        ));
    }
    if config.destination.as_os_str().is_empty() {
        return Err(CaptureError::InvalidConfiguration(
            "destination path is empty".to_string(),
        ));
    }
    validate_staging_path(&config.destination, &config.staging_destination)
}

fn validate_region(output: &Geometry, region: &Geometry) -> Result<(), CaptureError> {
    if region.width == 0 || region.height == 0 {
        return Err(CaptureError::InvalidConfiguration(
            "logical capture region must have positive dimensions".to_string(),
        ));
    }
    let output_left = i64::from(output.x);
    let output_top = i64::from(output.y);
    let output_right = output_left + i64::from(output.width);
    let output_bottom = output_top + i64::from(output.height);
    let region_left = i64::from(region.x);
    let region_top = i64::from(region.y);
    let region_right = region_left + i64::from(region.width);
    let region_bottom = region_top + i64::from(region.height);
    if region_left < output_left
        || region_top < output_top
        || region_right > output_right
        || region_bottom > output_bottom
    {
        return Err(CaptureError::InvalidConfiguration(
            "logical capture region must be contained by the selected output".to_string(),
        ));
    }
    Ok(())
}

struct CaptureMetadata {
    output_name: String,
    geometry: Geometry,
    scale_milli: u32,
    encoded_pixel_size: (u32, u32),
}

fn capture_metadata(config: &CaptureConfig) -> Result<CaptureMetadata, CaptureError> {
    let scale_milli = scale_milli(config.target.scale)?;
    let (geometry, encoded_pixel_size) = match config.area {
        CaptureArea::LogicalRegion(region) => (
            region,
            (
                scaled_dimension(region.width, config.target.scale)?,
                scaled_dimension(region.height, config.target.scale)?,
            ),
        ),
        CaptureArea::Monitor | CaptureArea::Portal => {
            (config.target.geometry, config.target.encoded_pixel_size())
        }
    };
    Ok(CaptureMetadata {
        output_name: config.target.output_name.clone(),
        geometry,
        scale_milli,
        encoded_pixel_size,
    })
}

fn validate_target(
    capabilities: &CaptureCapabilities,
    target: &CaptureTarget,
) -> Result<(), CaptureError> {
    let output = capabilities
        .output(&target.output_name)
        .ok_or_else(|| CaptureError::OutputNotFound(target.output_name.clone()))?;
    if target.geometry.width == 0
        || target.geometry.height == 0
        || !target.scale.is_finite()
        || target.scale <= 0.0
    {
        return Err(CaptureError::InvalidConfiguration(
            "target geometry and scale must be positive".to_string(),
        ));
    }
    if target.geometry != output.geometry || (target.scale - output.scale).abs() > f64::EPSILON {
        return Err(CaptureError::InvalidConfiguration(
            "selected output geometry or scale is stale; run discovery again".to_string(),
        ));
    }
    if target.pixel_size != output.pixel_size || target.transform != output.transform {
        return Err(CaptureError::InvalidConfiguration(
            "selected output pixel size or transform is stale; run discovery again".to_string(),
        ));
    }
    Ok(())
}

fn validate_audio(
    capabilities: &CaptureCapabilities,
    selection: &AudioSelection,
) -> Result<(), CaptureError> {
    if matches!(selection, AudioSelection::None) {
        return Ok(());
    }
    if !capabilities.tools.pactl {
        return Err(CaptureError::MissingTool("pactl"));
    }
    let (name, expected_kind) = match selection {
        AudioSelection::None | AudioSelection::Default => return Ok(()),
        AudioSelection::Microphone { source_name } => (source_name, AudioSourceKind::Microphone),
        AudioSelection::System { source_name } => (source_name, AudioSourceKind::SystemMonitor),
        AudioSelection::Mixed { source_name } => {
            let valid = capabilities
                .audio_source(source_name)
                .is_some_and(|source| source.kind == AudioSourceKind::Mixed);
            return if valid {
                Ok(())
            } else {
                Err(CaptureError::MissingMixedSource(source_name.clone()))
            };
        }
    };
    let source = capabilities
        .audio_source(name)
        .ok_or_else(|| CaptureError::AudioSourceNotFound(name.clone()))?;
    if source.kind != expected_kind {
        return Err(CaptureError::InvalidConfiguration(format!(
            "audio source `{name}` has the wrong capture role"
        )));
    }
    Ok(())
}

fn append_audio_arguments(command: &mut CommandPlan, selection: &AudioSelection) {
    match selection {
        AudioSelection::None => {}
        AudioSelection::Default => {
            command.push_arg("--audio");
            command.push_arg("--audio-backend=pipewire");
            command.push_arg("--audio-codec");
            command.push_arg("aac");
        }
        AudioSelection::Microphone { source_name }
        | AudioSelection::System { source_name }
        | AudioSelection::Mixed { source_name } => {
            command.push_arg(OsString::from(format!("--audio={source_name}")));
            command.push_arg("--audio-backend=pipewire");
            command.push_arg("--audio-codec");
            command.push_arg("aac");
        }
    }
}

fn append_gpu_audio_arguments(command: &mut CommandPlan, selection: &AudioSelection) {
    let source = match selection {
        AudioSelection::None => return,
        AudioSelection::Default | AudioSelection::Microphone { .. } => "default_input",
        AudioSelection::System { .. } => "default_output",
        AudioSelection::Mixed { .. } => "default_output|default_input",
    };
    command.push_arg("-a");
    command.push_arg(source);
    command.push_arg("-ac");
    command.push_arg("aac");
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scale_milli(scale: f64) -> Result<u32, CaptureError> {
    let scaled = (scale * 1000.0).round();
    if !scaled.is_finite() || scaled <= 0.0 || scaled > f64::from(u32::MAX) {
        return Err(CaptureError::InvalidConfiguration(
            "output scale cannot be represented".to_string(),
        ));
    }
    Ok(scaled as u32)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scaled_dimension(logical: u32, scale: f64) -> Result<u32, CaptureError> {
    let scaled = (f64::from(logical) * scale).round();
    if !scaled.is_finite() || scaled <= 0.0 || scaled > f64::from(u32::MAX) {
        return Err(CaptureError::InvalidConfiguration(
            "capture region pixel size cannot be represented".to_string(),
        ));
    }
    Ok(scaled as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{AudioSource, ToolCapabilities};

    fn capabilities() -> CaptureCapabilities {
        CaptureCapabilities {
            session: SessionKind::HyprlandWayland,
            tools: ToolCapabilities {
                gpu_screen_recorder: true,
                wf_recorder: true,
                hyprctl: true,
                pactl: true,
                pw_dump: true,
                kill: true,
            },
            outputs: vec![CaptureOutput {
                name: "DP-1".into(),
                description: "Main".into(),
                geometry: Geometry {
                    x: -100,
                    y: 20,
                    width: 1920,
                    height: 1080,
                },
                scale: 1.25,
                pixel_size: (1920, 1080),
                transform: OutputTransform::Normal,
                refresh_hz: 144.0,
                focused: true,
            }],
            audio_sources: vec![
                AudioSource {
                    name: "mic".into(),
                    description: "Mic".into(),
                    kind: AudioSourceKind::Microphone,
                    is_default: true,
                    state: None,
                },
                AudioSource {
                    name: "system.monitor".into(),
                    description: "System".into(),
                    kind: AudioSourceKind::SystemMonitor,
                    is_default: false,
                    state: None,
                },
                AudioSource {
                    name: "dicta.mix".into(),
                    description: "Combined".into(),
                    kind: AudioSourceKind::Mixed,
                    is_default: false,
                    state: None,
                },
            ],
        }
    }

    fn args(plan: &WfRecorderPlan) -> Vec<String> {
        command_args(&plan.command)
    }

    fn gpu_args(plan: &GpuScreenRecorderPlan) -> Vec<String> {
        command_args(&plan.command)
    }

    fn command_args(command: &CommandPlan) -> Vec<String> {
        command
            .arguments()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn command_plan_is_shell_free_and_carries_output_geometry_scale_and_mic() {
        let capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::Microphone {
                source_name: "mic".into(),
            },
            "/tmp/demo;touch nope.mp4",
        );
        let plan = wf_recorder_plan(&capabilities, &config).unwrap();
        let arguments = args(&plan);
        assert_eq!(plan.command.program(), "wf-recorder");
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--output", "DP-1"]));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--geometry", "-100,20 1920x1080"]));
        assert!(arguments.contains(&"--audio=mic".to_string()));
        assert_eq!(
            arguments.last().unwrap(),
            &config.staging_destination.to_string_lossy()
        );
        assert!(!arguments.contains(&"--overwrite".to_string()));
        assert_eq!(
            config.staging_destination.parent(),
            config.destination.parent()
        );
        assert_eq!(plan.scale_milli, 1250);
        assert_eq!(plan.encoded_pixel_size, (1920, 1080));
    }

    #[test]
    fn system_and_mixed_sources_are_explicit() {
        let capabilities = capabilities();
        for (selection, argument) in [
            (
                AudioSelection::System {
                    source_name: "system.monitor".into(),
                },
                "--audio=system.monitor",
            ),
            (
                AudioSelection::Mixed {
                    source_name: "dicta.mix".into(),
                },
                "--audio=dicta.mix",
            ),
        ] {
            let config =
                CaptureConfig::new(&capabilities.outputs[0], selection, "/tmp/capture.mp4");
            assert!(args(&wf_recorder_plan(&capabilities, &config).unwrap())
                .contains(&argument.to_string()));
        }
    }

    #[test]
    fn absent_combined_source_has_a_typed_error() {
        let capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::Mixed {
                source_name: "missing.mix".into(),
            },
            "/tmp/capture.mp4",
        );
        assert!(matches!(
            wf_recorder_plan(&capabilities, &config),
            Err(CaptureError::MissingMixedSource(name)) if name == "missing.mix"
        ));
    }

    #[test]
    fn gpu_screen_recorder_is_preferred_and_wf_recorder_is_deterministic_fallback() {
        let mut capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::None,
            "/tmp/capture.mp4",
        );
        assert!(matches!(
            capture_plan(&capabilities, &config).unwrap(),
            CapturePlan::GpuScreenRecorder(_)
        ));

        capabilities.tools.gpu_screen_recorder = false;
        assert!(matches!(
            capture_plan(&capabilities, &config).unwrap(),
            CapturePlan::WfRecorder(_)
        ));

        capabilities.tools.wf_recorder = false;
        assert!(matches!(
            capture_plan(&capabilities, &config),
            Err(CaptureError::MissingTool("wf-recorder"))
        ));
    }

    #[test]
    fn gpu_monitor_plan_matches_omarchy_performance_defaults() {
        let capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::System {
                source_name: "system.monitor".into(),
            },
            "/tmp/capture with spaces.mp4",
        );
        let plan = gpu_screen_recorder_plan(&capabilities, &config).unwrap();
        let arguments = gpu_args(&plan);
        assert_eq!(plan.command.program(), "gpu-screen-recorder");
        for pair in [
            ["-w", "DP-1"],
            ["-s", "0x0"],
            ["-k", "auto"],
            ["-f", "60"],
            ["-fm", "cfr"],
            ["-encoder", "gpu"],
            ["-fallback-cpu-encoding", "yes"],
            ["-a", "default_output"],
            ["-ac", "aac"],
        ] {
            assert!(arguments.windows(2).any(|actual| actual == pair));
        }
        assert_eq!(
            arguments.last().unwrap(),
            &config.staging_destination.to_string_lossy()
        );
        assert_eq!(plan.geometry, config.target.geometry);
        assert_eq!(plan.scale_milli, 1250);
        assert_eq!(plan.encoded_pixel_size, (1920, 1080));
    }

    #[test]
    fn logical_region_uses_v6_region_form_and_preserves_region_geometry() {
        let capabilities = capabilities();
        let region = Geometry {
            x: 100,
            y: 100,
            width: 800,
            height: 400,
        };
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::None,
            "/tmp/region.mp4",
        )
        .with_logical_region(region);
        let plan = gpu_screen_recorder_plan(&capabilities, &config).unwrap();
        let arguments = gpu_args(&plan);
        assert!(arguments.windows(2).any(|pair| pair == ["-w", "region"]));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["-region", "800x400+100+100"]));
        assert_eq!(plan.geometry, region);
        assert_eq!(plan.encoded_pixel_size, (1000, 500));

        let wf_plan = wf_recorder_plan(&capabilities, &config).unwrap();
        assert!(args(&wf_plan)
            .windows(2)
            .any(|pair| pair == ["--geometry", "100,100 800x400"]));
        assert_eq!(wf_plan.geometry, region);
        assert_eq!(wf_plan.encoded_pixel_size, (1000, 500));
    }

    #[test]
    fn portal_is_explicit_and_never_silently_falls_back() {
        let mut capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::None,
            "/tmp/portal.mp4",
        )
        .with_portal();
        let plan = gpu_screen_recorder_plan(&capabilities, &config).unwrap();
        assert!(gpu_args(&plan)
            .windows(2)
            .any(|pair| pair == ["-w", "portal"]));

        capabilities.tools.gpu_screen_recorder = false;
        assert!(matches!(
            capture_plan(&capabilities, &config),
            Err(CaptureError::MissingTool("gpu-screen-recorder"))
        ));
        assert!(matches!(
            wf_recorder_plan(&capabilities, &config),
            Err(CaptureError::InvalidConfiguration(detail)) if detail.contains("portal")
        ));
    }

    #[test]
    fn gpu_audio_modes_use_default_pipewire_endpoints_and_merge_tracks() {
        let capabilities = capabilities();
        for (selection, expected) in [
            (AudioSelection::Default, Some("default_input")),
            (
                AudioSelection::Microphone {
                    source_name: "mic".into(),
                },
                Some("default_input"),
            ),
            (
                AudioSelection::System {
                    source_name: "system.monitor".into(),
                },
                Some("default_output"),
            ),
            (
                AudioSelection::Mixed {
                    source_name: "dicta.mix".into(),
                },
                Some("default_output|default_input"),
            ),
            (AudioSelection::None, None),
        ] {
            let config = CaptureConfig::new(&capabilities.outputs[0], selection, "/tmp/audio.mp4");
            let arguments = gpu_args(&gpu_screen_recorder_plan(&capabilities, &config).unwrap());
            match expected {
                Some(source) => assert!(arguments.windows(2).any(|pair| pair == ["-a", source])),
                None => assert!(!arguments.iter().any(|argument| argument == "-a")),
            }
        }
    }

    #[test]
    fn region_must_be_inside_selected_output() {
        let capabilities = capabilities();
        let config = CaptureConfig::new(
            &capabilities.outputs[0],
            AudioSelection::None,
            "/tmp/region.mp4",
        )
        .with_logical_region(Geometry {
            x: -101,
            y: 20,
            width: 10,
            height: 10,
        });
        assert!(matches!(
            gpu_screen_recorder_plan(&capabilities, &config),
            Err(CaptureError::InvalidConfiguration(detail)) if detail.contains("contained")
        ));
    }
}
