use dicta_capture::{AudioSelection, CaptureArea};
use dicta_transcribe::{
    Language, ModelCatalog, ModelInstallerConfig, ModelKind, ModelSelection, VoxtypeBackendConfig,
    COMPACT_FILENAME,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct LinuxConfig {
    pub storage_root: PathBuf,
    pub output_name: String,
    pub audio: AudioSelection,
    pub area: CaptureArea,
    pub frame_rate: u16,
    pub transcription: LinuxTranscriptionConfig,
}

impl LinuxConfig {
    #[must_use]
    pub fn new(storage_root: impl Into<PathBuf>, output_name: impl Into<String>) -> Self {
        let storage_root = storage_root.into();
        Self {
            transcription: LinuxTranscriptionConfig::discover(&storage_root),
            storage_root,
            output_name: output_name.into(),
            // The native GPU backend maps this stable logical source to
            // `default_output|default_input`, preserving desktop sound and
            // narration in one AAC track for post-recording transcription.
            audio: AudioSelection::Mixed {
                source_name: "dicta-default-mixed".to_owned(),
            },
            area: CaptureArea::Monitor,
            frame_rate: 60,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.storage_root.as_os_str().is_empty() || !self.storage_root.is_absolute() {
            return Err("storage root must be an explicit absolute path".to_owned());
        }
        if self.output_name.trim().is_empty() {
            return Err("capture output name must not be empty".to_owned());
        }
        if self.frame_rate == 0 || self.frame_rate > 240 {
            return Err("frame rate must be between 1 and 240".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxTranscriptionConfig {
    pub enabled: bool,
    pub language: Language,
    pub model: ModelSelection,
    pub catalog: ModelCatalog,
    pub backend: VoxtypeBackendConfig,
    pub installer: ModelInstallerConfig,
}

impl LinuxTranscriptionConfig {
    #[must_use]
    pub fn discover(storage_root: &Path) -> Self {
        let enabled = std::env::var("DICTA_TRANSCRIPTION").map_or(true, |value| {
            !matches!(value.trim(), "0" | "false" | "off" | "disabled")
        });
        let language = std::env::var("DICTA_TRANSCRIPTION_LANGUAGE")
            .ok()
            .and_then(|value| Language::from_code(value.trim()))
            .or_else(|| settings_language(storage_root))
            .unwrap_or_default();
        let models_dir = local_data_root()
            .unwrap_or_else(|| storage_root.to_path_buf())
            .join("Dicta")
            .join("models");
        let mut catalog = ModelCatalog::new(models_dir);
        if let Some(path) = std::env::var_os("DICTA_WHISPER_MODEL")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            catalog = catalog.with_override_path(path);
        }
        if let Some(path) = bundled_model_candidate() {
            catalog = catalog.with_bundled_compact(path);
        }
        let model = match std::env::var("DICTA_TRANSCRIPTION_MODEL")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("compact") => ModelSelection::Kind(ModelKind::Compact),
            Some("large-v3-turbo") => ModelSelection::Kind(ModelKind::LargeV3Turbo),
            Some(path) if !path.is_empty() && path != "auto" => {
                ModelSelection::Path(PathBuf::from(path))
            }
            _ => ModelSelection::Auto,
        };
        let mut backend = VoxtypeBackendConfig::default();
        if let Some(program) =
            std::env::var_os("DICTA_FFMPEG_BIN").filter(|value| !value.is_empty())
        {
            backend.ffmpeg_program = PathBuf::from(program);
        }
        if let Some(program) =
            std::env::var_os("DICTA_VOXTYPE_BIN").filter(|value| !value.is_empty())
        {
            backend.voxtype_program = PathBuf::from(program);
        } else {
            let vulkan_backend = PathBuf::from("/usr/lib/voxtype/voxtype-vulkan");
            if vulkan_backend.is_file() && Path::new("/dev/dri/renderD128").exists() {
                backend.voxtype_program = vulkan_backend;
            }
        }
        let mut installer = ModelInstallerConfig::default();
        if let Some(program) = std::env::var_os("DICTA_CURL_BIN").filter(|value| !value.is_empty())
        {
            installer.curl_program = PathBuf::from(program);
        }
        if let Some(program) =
            std::env::var_os("DICTA_SHA1SUM_BIN").filter(|value| !value.is_empty())
        {
            installer.sha1sum_program = PathBuf::from(program);
        }
        Self {
            enabled,
            language,
            model,
            catalog,
            backend,
            installer,
        }
    }

    #[must_use]
    pub fn disabled(storage_root: &Path) -> Self {
        let mut config = Self::discover(storage_root);
        config.enabled = false;
        config
    }
}

fn settings_language(storage_root: &Path) -> Option<Language> {
    let settings = dicta_core::storage::read_json::<dicta_core::storage::AppSettings>(
        &storage_root.join("settings.json"),
    )
    .ok()?
    .normalized();
    Language::from_code(&settings.transcription_language)
}

fn local_data_root() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
}

fn bundled_model_candidate() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DICTA_BUNDLED_WHISPER_MODEL")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(path);
    }
    let executable = std::env::current_exe().ok()?;
    let binary_directory = executable.parent()?;
    [
        binary_directory.join(COMPACT_FILENAME),
        binary_directory
            .join("../share/dicta")
            .join(COMPACT_FILENAME),
        binary_directory
            .join("../share/Dicta/resources")
            .join(COMPACT_FILENAME),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageLayout {
    root: PathBuf,
}

impl StorageLayout {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn recording_directory(
        &self,
        project_id: Option<&dicta_core::ProjectId>,
        recording_id: &dicta_core::RecordingId,
    ) -> PathBuf {
        let project = project_id.map_or(dicta_core::GENERAL_PROJECT_ID, |id| id.as_str());
        let project_directory = if project == dicta_core::GENERAL_PROJECT_ID {
            self.root.join("General")
        } else {
            self.root.join(project)
        };
        project_directory
            .join("recordings")
            .join(day_from_recording_id(recording_id.as_str()))
    }

    #[must_use]
    pub fn video_path(
        &self,
        project_id: Option<&dicta_core::ProjectId>,
        recording_id: &dicta_core::RecordingId,
    ) -> PathBuf {
        self.recording_directory(project_id, recording_id)
            .join(format!("{recording_id}.mp4"))
    }

    #[must_use]
    pub fn metadata_path(
        &self,
        project_id: Option<&dicta_core::ProjectId>,
        recording_id: &dicta_core::RecordingId,
    ) -> PathBuf {
        self.recording_directory(project_id, recording_id)
            .join(format!("{recording_id}.json"))
    }

    pub(crate) fn reservation_directory(&self) -> PathBuf {
        self.root.join(".ids")
    }
}

pub(crate) fn day_from_recording_id(recording_id: &str) -> String {
    let prefix = recording_id.as_bytes().get(..8);
    if prefix.is_some_and(|value| value.iter().all(u8::is_ascii_digit)) {
        format!(
            "{}-{}-{}",
            &recording_id[0..4],
            &recording_id[4..6],
            &recording_id[6..8]
        )
    } else {
        "undated".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicta_core::{ProjectId, RecordingId};

    #[test]
    fn layout_matches_existing_offline_scanner_shape() {
        let layout = StorageLayout::new("/data/dicta");
        let recording = RecordingId::new("20260820-18-00-00").unwrap();
        assert_eq!(
            layout.video_path(None, &recording),
            Path::new("/data/dicta/General/recordings/2026-08-20/20260820-18-00-00.mp4")
        );
        let project = ProjectId::new("dicta").unwrap();
        assert_eq!(
            layout.metadata_path(Some(&project), &recording),
            Path::new("/data/dicta/dicta/recordings/2026-08-20/20260820-18-00-00.json")
        );
    }

    #[test]
    fn legacy_settings_language_is_reused() {
        let root =
            std::env::temp_dir().join(format!("dicta-linux-language-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("settings.json"),
            br#"{"transcription_language":"nl"}"#,
        )
        .unwrap();
        assert_eq!(settings_language(&root), Some(Language::Dutch));
        std::fs::remove_file(root.join("settings.json")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn native_config_captures_desktop_and_narration_audio() {
        let config = LinuxConfig::new("/data/dicta", "DP-1");
        assert_eq!(
            config.audio,
            AudioSelection::Mixed {
                source_name: "dicta-default-mixed".to_owned()
            }
        );
    }
}
