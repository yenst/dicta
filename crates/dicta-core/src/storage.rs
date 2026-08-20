use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub const DEFAULT_SHORTCUT_ID: &str = "alt_shift_r";
pub const DEFAULT_TRANSCRIPTION_LANGUAGE: &str = "auto";
pub const SHORTCUT_IDS: [&str; 5] = [
    "command_shift_r",
    DEFAULT_SHORTCUT_ID,
    "command_shift_d",
    "option_space",
    "control_space",
];
pub const TRANSCRIPTION_LANGUAGES: [&str; 6] = ["auto", "nl", "en", "fr", "de", "es"];

/// Persistent application preferences shared by the legacy and native apps.
///
/// The wire names deliberately match the existing `settings.json` contract so
/// moving between releases never requires a migration tool.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppSettings {
    #[serde(default = "default_shortcut_id")]
    pub shortcut_id: String,
    #[serde(default = "enabled_by_default")]
    pub cleanup_merged_videos: bool,
    #[serde(default = "enabled_by_default")]
    pub branch_locking: bool,
    #[serde(default = "default_transcription_language")]
    pub transcription_language: String,
    #[serde(default)]
    pub general_path: Option<String>,
}

impl AppSettings {
    #[must_use]
    pub fn normalized(mut self) -> Self {
        if self.shortcut_id == "command_shift_r" || !is_shortcut_id(&self.shortcut_id) {
            self.shortcut_id = DEFAULT_SHORTCUT_ID.to_owned();
        }
        if !is_transcription_language(&self.transcription_language) {
            self.transcription_language = DEFAULT_TRANSCRIPTION_LANGUAGE.to_owned();
        }
        self.general_path = self
            .general_path
            .take()
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty());
        self
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut_id: default_shortcut_id(),
            cleanup_merged_videos: true,
            branch_locking: true,
            transcription_language: default_transcription_language(),
            general_path: None,
        }
    }
}

#[must_use]
pub fn is_shortcut_id(value: &str) -> bool {
    SHORTCUT_IDS.contains(&value)
}

#[must_use]
pub fn is_transcription_language(value: &str) -> bool {
    TRANSCRIPTION_LANGUAGES.contains(&value)
}

fn default_shortcut_id() -> String {
    DEFAULT_SHORTCUT_ID.to_owned()
}

fn default_transcription_language() -> String {
    DEFAULT_TRANSCRIPTION_LANGUAGE.to_owned()
}

const fn enabled_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GeneralSettings {
    #[serde(default)]
    pub general_path: Option<String>,
}

pub fn configured_general_path(storage_root: &Path, configured: Option<&str>) -> Option<PathBuf> {
    let path = configured.map(str::trim).filter(|path| !path.is_empty())?;
    let path = PathBuf::from(path);
    Some(if path.is_absolute() {
        path
    } else {
        storage_root.join(path)
    })
}

pub fn general_storage_path(storage_root: &Path, configured: Option<&str>) -> PathBuf {
    configured_general_path(storage_root, configured)
        .unwrap_or_else(|| storage_root.join("General"))
}

pub fn general_storage_candidates(storage_root: &Path, configured: Option<&str>) -> Vec<PathBuf> {
    let mut paths = configured_general_path(storage_root, configured)
        .into_iter()
        .collect::<Vec<_>>();
    for path in [
        storage_root.join("General"),
        storage_root.join("unprojected"),
    ] {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

pub fn preferred_storage_root(documents: &Path) -> PathBuf {
    let current = documents.join("Dicta");
    let legacy = documents.join("PromptReel");
    if current.exists() || !legacy.exists() {
        current
    } else {
        legacy
    }
}

pub fn annotation_sidecar_path(metadata_path: &Path) -> PathBuf {
    let stem = metadata_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("recording");
    metadata_path.with_file_name(format!("{stem}.annotations.json"))
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Could not serialize {}: {error}", path.display()))?;

    let mut last_error = None;
    for _ in 0..16 {
        let counter = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("dicta.json");
        let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "Could not create temporary file for {}: {error}",
                    path.display()
                ));
            }
        };

        let result = (|| {
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "Could not atomically write {}: {error}",
                path.display()
            ));
        }
        return Ok(());
    }

    Err(format!(
        "Could not reserve a temporary file for {}: {}",
        path.display(),
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "temporary name collision".to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn configured_general_storage_precedes_current_and_legacy_defaults() {
        let root = Path::new("/documents/Dicta");
        assert_eq!(general_storage_path(root, None), root.join("General"));
        assert_eq!(
            general_storage_candidates(root, Some("custom")),
            vec![
                root.join("custom"),
                root.join("General"),
                root.join("unprojected")
            ]
        );
    }

    #[test]
    fn legacy_settings_defaults_and_normalization_are_stable() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults, AppSettings::default());

        let normalized = AppSettings {
            shortcut_id: "command_shift_r".to_owned(),
            transcription_language: "xx".to_owned(),
            general_path: Some("  ".to_owned()),
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(normalized.shortcut_id, "alt_shift_r");
        assert_eq!(normalized.transcription_language, "auto");
        assert_eq!(normalized.general_path, None);
    }

    #[test]
    fn annotation_sidecar_is_a_sibling_of_recording_metadata() {
        let metadata = Path::new("/repo/.dicta/recordings/2026-08-20/12-00-00.json");
        assert_eq!(
            annotation_sidecar_path(metadata),
            Path::new("/repo/.dicta/recordings/2026-08-20/12-00-00.annotations.json")
        );
    }

    #[test]
    fn atomic_json_writes_round_trip_and_replace_existing_content() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "dicta-core-storage-test-{}-{unique}",
            std::process::id()
        ));
        let path = directory.join("recording.json");

        write_json_atomic(&path, &json!({"version": 1})).unwrap();
        assert_eq!(read_json::<serde_json::Value>(&path).unwrap()["version"], 1);
        write_json_atomic(&path, &json!({"version": 2, "ready": true})).unwrap();
        let value = read_json::<serde_json::Value>(&path).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["ready"], true);
        assert_eq!(
            fs::read_dir(&directory).unwrap().count(),
            1,
            "temporary files were left behind"
        );

        fs::remove_dir_all(directory).unwrap();
    }
}
