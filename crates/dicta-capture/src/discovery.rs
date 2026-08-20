use crate::{
    command::{CommandOutput, CommandPlan, Platform},
    error::CaptureError,
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, env, ffi::OsStr};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionEnvironment {
    pub session_type: Option<String>,
    pub wayland_display: Option<String>,
    pub hyprland_instance_signature: Option<String>,
}

impl SessionEnvironment {
    #[must_use]
    pub fn current() -> Self {
        Self {
            session_type: env::var("XDG_SESSION_TYPE").ok(),
            wayland_display: env::var("WAYLAND_DISPLAY").ok(),
            hyprland_instance_signature: env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(),
        }
    }

    #[must_use]
    pub fn is_wayland(&self) -> bool {
        self.session_type
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
            || self.wayland_display.is_some()
    }

    #[must_use]
    pub fn is_hyprland(&self) -> bool {
        self.hyprland_instance_signature.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionKind {
    HyprlandWayland,
    OtherWayland,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolCapabilities {
    pub gpu_screen_recorder: bool,
    pub wf_recorder: bool,
    pub hyprctl: bool,
    pub pactl: bool,
    pub pw_dump: bool,
    pub kill: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Geometry {
    #[must_use]
    pub fn wf_recorder_argument(self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }

    #[must_use]
    pub fn gpu_screen_recorder_argument(self) -> String {
        format!("{}x{}+{}+{}", self.width, self.height, self.x, self.y)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTransform {
    Normal,
    Rotated90,
    Rotated180,
    Rotated270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl OutputTransform {
    fn from_hyprland(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Normal,
            1 => Self::Rotated90,
            2 => Self::Rotated180,
            3 => Self::Rotated270,
            4 => Self::Flipped,
            5 => Self::Flipped90,
            6 => Self::Flipped180,
            7 => Self::Flipped270,
            _ => return None,
        })
    }

    const fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Rotated90 | Self::Rotated270 | Self::Flipped90 | Self::Flipped270
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureOutput {
    pub name: String,
    pub description: String,
    pub geometry: Geometry,
    pub scale: f64,
    pub pixel_size: (u32, u32),
    pub transform: OutputTransform,
    pub refresh_hz: f64,
    pub focused: bool,
}

impl CaptureOutput {
    #[must_use]
    pub const fn encoded_pixel_size(&self) -> (u32, u32) {
        self.pixel_size
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn float_to_dimension(value: f64) -> u32 {
    if value.is_finite() && value > 0.0 && value <= f64::from(u32::MAX) {
        value as u32
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSourceKind {
    Microphone,
    SystemMonitor,
    Mixed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioSource {
    pub name: String,
    pub description: String,
    pub kind: AudioSourceKind,
    pub is_default: bool,
    pub state: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureCapabilities {
    pub session: SessionKind,
    pub tools: ToolCapabilities,
    pub outputs: Vec<CaptureOutput>,
    pub audio_sources: Vec<AudioSource>,
}

impl CaptureCapabilities {
    #[must_use]
    pub fn output(&self, name: &str) -> Option<&CaptureOutput> {
        self.outputs.iter().find(|output| output.name == name)
    }

    #[must_use]
    pub fn audio_source(&self, name: &str) -> Option<&AudioSource> {
        self.audio_sources.iter().find(|source| source.name == name)
    }
}

/// Discovers capture tools, Hyprland outputs, and audio sources.
///
/// Discovery only executes finite query commands; it never opens a stream or
/// spawns `wf-recorder`.
///
/// # Errors
///
/// Returns a typed error if an available discovery command fails or emits an
/// invalid response.
pub fn discover(
    platform: &mut impl Platform,
    environment: &SessionEnvironment,
) -> Result<CaptureCapabilities, CaptureError> {
    let tools = ToolCapabilities {
        gpu_screen_recorder: platform.executable_exists(OsStr::new("gpu-screen-recorder")),
        wf_recorder: platform.executable_exists(OsStr::new("wf-recorder")),
        hyprctl: platform.executable_exists(OsStr::new("hyprctl")),
        pactl: platform.executable_exists(OsStr::new("pactl")),
        pw_dump: platform.executable_exists(OsStr::new("pw-dump")),
        kill: platform.executable_exists(OsStr::new("kill")),
    };
    let session = if environment.is_wayland() && environment.is_hyprland() {
        SessionKind::HyprlandWayland
    } else if environment.is_wayland() {
        SessionKind::OtherWayland
    } else {
        SessionKind::Unsupported
    };

    let outputs = if tools.hyprctl && session == SessionKind::HyprlandWayland {
        discover_outputs(platform)?
    } else {
        Vec::new()
    };
    let audio_sources = if tools.pactl {
        discover_audio_sources(platform)?
    } else {
        Vec::new()
    };

    Ok(CaptureCapabilities {
        session,
        tools,
        outputs,
        audio_sources,
    })
}

fn checked_output(
    platform: &mut impl Platform,
    plan: &CommandPlan,
) -> Result<CommandOutput, CaptureError> {
    let program = plan.program().to_string_lossy().into_owned();
    let output = platform
        .output(plan)
        .map_err(|source| CaptureError::command_io(program.clone(), source))?;
    if output.success {
        Ok(output)
    } else {
        Err(CaptureError::CommandFailed {
            program,
            code: output.code,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HyprlandOutput {
    name: String,
    #[serde(default)]
    description: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    #[serde(default = "unit_scale")]
    scale: f64,
    #[serde(default)]
    refresh_rate: f64,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    transform: u8,
}

const fn unit_scale() -> f64 {
    1.0
}

fn discover_outputs(platform: &mut impl Platform) -> Result<Vec<CaptureOutput>, CaptureError> {
    let plan = CommandPlan::new("hyprctl")
        .arg("-j")
        .arg("monitors")
        .arg("all");
    let output = checked_output(platform, &plan)?;
    let parsed: Vec<HyprlandOutput> =
        serde_json::from_slice(&output.stdout).map_err(|error| CaptureError::InvalidResponse {
            program: "hyprctl",
            detail: error.to_string(),
        })?;
    parsed
        .into_iter()
        .filter(|output| !output.disabled)
        .map(|output| {
            if output.name.trim().is_empty()
                || output.width == 0
                || output.height == 0
                || !output.scale.is_finite()
                || output.scale <= 0.0
            {
                return Err(CaptureError::InvalidResponse {
                    program: "hyprctl",
                    detail: "monitor has an invalid name, geometry, or scale".to_string(),
                });
            }
            let transform = OutputTransform::from_hyprland(output.transform).ok_or_else(|| {
                CaptureError::InvalidResponse {
                    program: "hyprctl",
                    detail: format!("monitor has unknown transform {}", output.transform),
                }
            })?;
            let (pixel_width, pixel_height) = if transform.swaps_axes() {
                (output.height, output.width)
            } else {
                (output.width, output.height)
            };
            let logical_width = float_to_dimension(f64::from(pixel_width) / output.scale);
            let logical_height = float_to_dimension(f64::from(pixel_height) / output.scale);
            if logical_width == 0 || logical_height == 0 {
                return Err(CaptureError::InvalidResponse {
                    program: "hyprctl",
                    detail: "monitor scale produces an invalid logical geometry".to_string(),
                });
            }
            Ok(CaptureOutput {
                name: output.name,
                description: output.description,
                geometry: Geometry {
                    x: output.x,
                    y: output.y,
                    width: logical_width,
                    height: logical_height,
                },
                scale: output.scale,
                pixel_size: (pixel_width, pixel_height),
                transform,
                refresh_hz: output.refresh_rate,
                focused: output.focused,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct PactlSource {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    monitor_of_sink: Value,
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

fn discover_audio_sources(platform: &mut impl Platform) -> Result<Vec<AudioSource>, CaptureError> {
    let plan = CommandPlan::new("pactl")
        .arg("--format=json")
        .arg("list")
        .arg("sources");
    let output = checked_output(platform, &plan)?;
    let sources: Vec<PactlSource> =
        serde_json::from_slice(&output.stdout).map_err(|error| CaptureError::InvalidResponse {
            program: "pactl",
            detail: error.to_string(),
        })?;
    let default_name = default_audio_source(platform);
    Ok(sources
        .into_iter()
        .map(|source| {
            let kind = classify_audio_source(&source);
            AudioSource {
                is_default: default_name.as_deref() == Some(source.name.as_str()),
                name: source.name,
                description: source.description,
                kind,
                state: source.state,
            }
        })
        .collect())
}

fn default_audio_source(platform: &mut impl Platform) -> Option<String> {
    let plan = CommandPlan::new("pactl").arg("get-default-source");
    let output = platform.output(&plan).ok()?;
    if !output.success {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn classify_audio_source(source: &PactlSource) -> AudioSourceKind {
    let declared_mixed = source
        .properties
        .get("dicta.capture.role")
        .is_some_and(|role| role.eq_ignore_ascii_case("mixed"));
    let normalized_name = source.name.to_ascii_lowercase();
    if declared_mixed || (normalized_name.contains("dicta") && normalized_name.contains("mix")) {
        AudioSourceKind::Mixed
    } else if normalized_name.ends_with(".monitor") || is_sink_monitor(&source.monitor_of_sink) {
        AudioSourceKind::SystemMonitor
    } else {
        AudioSourceKind::Microphone
    }
}

fn is_sink_monitor(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_u64().is_some_and(|id| id != u64::from(u32::MAX)),
        Value::String(id) => id != "4294967295" && !id.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CaptureChild, ProcessExit};
    use std::{collections::VecDeque, ffi::OsString, io, time::Duration};

    struct QueryPlatform {
        available: Vec<OsString>,
        outputs: VecDeque<CommandOutput>,
        seen: Vec<CommandPlan>,
    }

    impl Platform for QueryPlatform {
        fn executable_exists(&self, name: &OsStr) -> bool {
            self.available.iter().any(|candidate| candidate == name)
        }

        fn output(&mut self, plan: &CommandPlan) -> io::Result<CommandOutput> {
            self.seen.push(plan.clone());
            self.outputs
                .pop_front()
                .ok_or_else(|| io::Error::other("unexpected command"))
        }

        fn spawn(&mut self, _: &CommandPlan) -> io::Result<Box<dyn CaptureChild>> {
            panic!("discovery must not initialize capture")
        }

        fn sleep(&mut self, _: Duration) {}
    }

    #[test]
    fn discovers_hyprland_outputs_and_audio_without_spawning() {
        let monitors = br#"[{"name":"DP-1","description":"Main","x":-100,"y":0,"width":2560,"height":1440,"refreshRate":143.99,"scale":1.25,"focused":true,"disabled":false}]"#;
        let sources = br#"[{"name":"alsa_input.usb-mic","description":"Desk mic","state":"RUNNING","monitor_of_sink":4294967295},{"name":"alsa_output.main.monitor","description":"Desktop audio","monitor_of_sink":2},{"name":"dicta.capture.mix","description":"Dicta combined","properties":{"dicta.capture.role":"mixed"}}]"#;
        let mut platform = QueryPlatform {
            available: [
                "gpu-screen-recorder",
                "wf-recorder",
                "hyprctl",
                "pactl",
                "pw-dump",
                "kill",
            ]
            .map(OsString::from)
            .to_vec(),
            outputs: VecDeque::from([
                CommandOutput::success(monitors.to_vec()),
                CommandOutput::success(sources.to_vec()),
                CommandOutput::success(b"alsa_input.usb-mic\n".to_vec()),
            ]),
            seen: Vec::new(),
        };
        let capabilities = discover(
            &mut platform,
            &SessionEnvironment {
                session_type: Some("wayland".into()),
                wayland_display: Some("wayland-1".into()),
                hyprland_instance_signature: Some("instance".into()),
            },
        )
        .unwrap();

        assert_eq!(capabilities.session, SessionKind::HyprlandWayland);
        assert!(capabilities.tools.gpu_screen_recorder);
        assert_eq!(capabilities.outputs[0].geometry.x, -100);
        assert_eq!(
            capabilities.outputs[0].geometry,
            Geometry {
                x: -100,
                y: 0,
                width: 2048,
                height: 1152
            }
        );
        assert_eq!(capabilities.outputs[0].encoded_pixel_size(), (2560, 1440));
        assert!(capabilities.audio_sources[0].is_default);
        assert_eq!(
            capabilities.audio_sources[1].kind,
            AudioSourceKind::SystemMonitor
        );
        assert_eq!(capabilities.audio_sources[2].kind, AudioSourceKind::Mixed);
        assert_eq!(platform.seen.len(), 3);
    }

    #[test]
    fn missing_tools_are_capabilities_not_discovery_side_effects() {
        let mut platform = QueryPlatform {
            available: Vec::new(),
            outputs: VecDeque::new(),
            seen: Vec::new(),
        };
        let capabilities = discover(
            &mut platform,
            &SessionEnvironment {
                session_type: Some("tty".into()),
                wayland_display: None,
                hyprland_instance_signature: None,
            },
        )
        .unwrap();
        assert_eq!(capabilities.session, SessionKind::Unsupported);
        assert!(capabilities.outputs.is_empty());
        assert!(capabilities.audio_sources.is_empty());
        assert!(platform.seen.is_empty());
    }

    #[test]
    fn rotated_fractional_output_uses_logical_geometry_and_physical_frame_size() {
        let monitors = br#"[{"name":"eDP-1","x":20,"y":40,"width":3840,"height":2160,"scale":2.0,"transform":1}]"#;
        let mut platform = QueryPlatform {
            available: vec![OsString::from("hyprctl")],
            outputs: VecDeque::from([CommandOutput::success(monitors.to_vec())]),
            seen: Vec::new(),
        };
        let capabilities = discover(
            &mut platform,
            &SessionEnvironment {
                session_type: Some("wayland".into()),
                wayland_display: Some("wayland-1".into()),
                hyprland_instance_signature: Some("instance".into()),
            },
        )
        .unwrap();
        let output = &capabilities.outputs[0];
        assert_eq!(output.transform, OutputTransform::Rotated90);
        assert_eq!(output.pixel_size, (2160, 3840));
        assert_eq!(
            output.geometry,
            Geometry {
                x: 20,
                y: 40,
                width: 1080,
                height: 1920,
            }
        );
    }

    #[allow(dead_code)]
    struct NeverChild;

    impl CaptureChild for NeverChild {
        fn id(&self) -> u32 {
            0
        }

        fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
            Ok(None)
        }

        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn wait(&mut self) -> io::Result<ProcessExit> {
            Ok(ProcessExit {
                success: true,
                code: Some(0),
            })
        }
    }
}
