use dicta_core::storage::{read_json, write_json_atomic, AppSettings};
use dicta_runtime::{PortError, PortErrorKind};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsStore {
    root: PathBuf,
}

impl SettingsStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Reads the legacy-compatible settings document without creating it.
    ///
    /// # Errors
    /// Returns a typed error for malformed JSON or unsafe filesystem objects.
    pub fn load(&self) -> Result<AppSettings, PortError> {
        self.validate_root()?;
        let path = self.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(PortError::new(
                    PortErrorKind::PermissionDenied,
                    "settings.json must be a regular, non-symlinked file",
                ))
            }
            Ok(_) => read_json::<AppSettings>(&path)
                .map(AppSettings::normalized)
                .map_err(|error| PortError::new(PortErrorKind::InvalidRequest, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
            Err(error) => Err(io_error("inspect settings", &error)),
        }
    }

    /// Atomically replaces the validated settings document.
    ///
    /// # Errors
    /// Returns a typed error when the root or destination is unsafe or cannot
    /// be written.
    pub fn save(&self, settings: &AppSettings) -> Result<(), PortError> {
        self.validate_root()?;
        let path = self.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(PortError::new(
                    PortErrorKind::PermissionDenied,
                    "settings.json must be a regular, non-symlinked file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error("inspect settings", &error)),
        }
        write_json_atomic(&path, &settings.clone().normalized())
            .map_err(|error| PortError::new(PortErrorKind::Internal, error))
    }

    fn validate_root(&self) -> Result<(), PortError> {
        let metadata = fs::symlink_metadata(&self.root)
            .map_err(|error| io_error("inspect storage root", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "settings storage root must be a real directory",
            ));
        }
        Ok(())
    }
}

fn io_error(action: &str, error: &io::Error) -> PortError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        _ => PortErrorKind::Internal,
    };
    PortError::new(kind, format!("could not {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dicta-linux-settings-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn missing_settings_use_legacy_defaults_and_updates_are_atomic() {
        let root = fixture();
        let store = SettingsStore::new(&root);
        assert_eq!(store.load().unwrap(), AppSettings::default());

        let settings = AppSettings {
            shortcut_id: "control_space".to_owned(),
            cleanup_merged_videos: false,
            branch_locking: false,
            transcription_language: "nl".to_owned(),
            general_path: Some("Archive".to_owned()),
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_settings_are_rejected_without_touching_the_target() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let target = root.join("outside.json");
        fs::write(&target, b"sentinel").unwrap();
        symlink(&target, root.join("settings.json")).unwrap();
        let store = SettingsStore::new(&root);
        assert_eq!(
            store.load().unwrap_err().kind,
            PortErrorKind::PermissionDenied
        );
        assert_eq!(
            store.save(&AppSettings::default()).unwrap_err().kind,
            PortErrorKind::PermissionDenied
        );
        assert_eq!(fs::read(&target).unwrap(), b"sentinel");
        fs::remove_dir_all(root).unwrap();
    }
}
