use dicta_core::storage::AppSettings;
use dicta_runtime::{PortError, PortErrorKind};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OmarchyShortcutIntegration {
    config_home: Option<PathBuf>,
}

impl OmarchyShortcutIntegration {
    pub(crate) const fn disabled() -> Self {
        Self { config_home: None }
    }

    pub(crate) fn discover() -> Self {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".config"))
            });
        Self { config_home }
    }

    #[cfg(test)]
    pub(crate) fn new(config_home: impl Into<PathBuf>) -> Self {
        Self {
            config_home: Some(config_home.into()),
        }
    }

    /// Replaces only the managed Dicta binding module. The integration is
    /// opt-in: without a module installed by `dicta-install-omarchy-shortcut`,
    /// settings remain portable data and no desktop configuration is created.
    pub(crate) fn sync_if_installed(&self, settings: &AppSettings) -> Result<bool, PortError> {
        let Some(config_home) = self.config_home.as_deref() else {
            return Ok(false);
        };
        let hypr = config_home.join("hypr");
        let destination = hypr.join("dicta-bindings.lua");
        let metadata = match fs::symlink_metadata(&destination) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error("inspect managed Omarchy shortcut", &error)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "managed Omarchy shortcut must be a regular, non-symlinked file",
            ));
        }
        validate_real_directory(&hypr)?;
        let content = managed_binding(&settings.shortcut_id)?;
        if fs::read(&destination)
            .map_err(|error| io_error("read managed Omarchy shortcut", &error))?
            == content.as_bytes()
        {
            return Ok(true);
        }
        write_atomic(&destination, content.as_bytes())?;
        Ok(true)
    }
}

fn managed_binding(shortcut_id: &str) -> Result<String, PortError> {
    let sequence = match shortcut_id {
        "command_shift_r" | "alt_shift_r" => "ALT + SHIFT + R",
        "command_shift_d" => "SUPER + SHIFT + D",
        "option_space" => "ALT + SPACE",
        "control_space" => "CTRL + SPACE",
        _ => {
            return Err(PortError::new(
                PortErrorKind::InvalidRequest,
                format!("unknown Omarchy shortcut preset `{shortcut_id}`"),
            ));
        }
    };
    Ok(format!(
        "-- Managed by Dicta. Re-run dicta-install-omarchy-shortcut to repair or remove it.\n\
         -- The selected recording key and annotation key are released before Dicta claims them.\n\
         hl.unbind(\"{sequence}\")\n\
         o.bind(\"{sequence}\", \"Toggle Dicta recording\", \"dicta record toggle\")\n\
         hl.unbind(\"SUPER + ALT + A\")\n\
         hl.unbind(\"F8\")\n\
         hl.unbind(\"CTRL + SHIFT + D\")\n\
         o.bind(\"CTRL + SHIFT + D\", \"Draw Dicta annotation (hold)\", \"dicta annotate enable\")\n\
         o.bind(\"CTRL + SHIFT + D\", nil, \"dicta annotate disable\", {{ release = true }})\n\
         o.window({{ class = \"^dicta-native$\", title = \"^Dicta Annotation Overlay$\" }}, {{\n\
           tag = \"-default-opacity\",\n\
           float = true,\n\
           pin = true,\n\
           border_size = 0,\n\
           rounding = 0,\n\
           opacity = \"1 1\",\n\
           size = {{ \"monitor_w\", \"monitor_h\" }},\n\
           move = {{ 0, 0 }},\n\
         }})\n\
         o.window({{ class = \"^dicta-native$\", title = \"^Dicta status$\" }}, {{\n\
           tag = \"-default-opacity\",\n\
           float = true,\n\
           pin = true,\n\
           no_initial_focus = true,\n\
           border_size = 0,\n\
           rounding = 12,\n\
           opacity = \"1 1\",\n\
           move = {{ \"monitor_w-window_w-24\", \"monitor_h-window_h-34\" }},\n\
         }})\n"
    ))
}

fn validate_real_directory(path: &Path) -> Result<(), PortError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect Hyprland configuration directory", &error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "Hyprland configuration directory must be a real directory",
        ));
    }
    Ok(())
}

fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<(), PortError> {
    let parent = destination.parent().ok_or_else(|| {
        PortError::new(
            PortErrorKind::Internal,
            "managed Omarchy shortcut has no parent directory",
        )
    })?;
    let temporary = parent.join(format!(
        ".dicta-bindings.lua.{}.{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| io_error("create managed Omarchy shortcut staging file", &error))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| io_error("write managed Omarchy shortcut staging file", &error))?;
        fs::rename(&temporary, destination)
            .map_err(|error| io_error("replace managed Omarchy shortcut", &error))?;
        let directory = fs::File::open(parent)
            .map_err(|error| io_error("open Hyprland configuration directory", &error))?;
        directory
            .sync_all()
            .map_err(|error| io_error("sync Hyprland configuration directory", &error))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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
    use std::{os::unix::fs::symlink, time::SystemTime};

    fn fixture() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dicta-omarchy-shortcut-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("hypr")).unwrap();
        root
    }

    #[test]
    fn absent_managed_module_never_creates_desktop_configuration() {
        let root = fixture();
        let integration = OmarchyShortcutIntegration::new(&root);
        assert!(!integration
            .sync_if_installed(&AppSettings::default())
            .unwrap());
        assert_eq!(fs::read_dir(root.join("hypr")).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_module_tracks_the_typed_shortcut_without_shell_expansion() {
        let root = fixture();
        let destination = root.join("hypr/dicta-bindings.lua");
        fs::write(&destination, "placeholder\n").unwrap();
        let integration = OmarchyShortcutIntegration::new(&root);
        let mut settings = AppSettings {
            shortcut_id: "control_space".to_owned(),
            ..AppSettings::default()
        };
        assert!(integration.sync_if_installed(&settings).unwrap());
        let control_space = fs::read_to_string(&destination).unwrap();
        assert!(control_space.contains("hl.unbind(\"CTRL + SPACE\")"));
        assert!(control_space.contains("\"dicta record toggle\""));
        assert!(control_space.contains("hl.unbind(\"SUPER + ALT + A\")"));
        assert!(control_space.contains("hl.unbind(\"F8\")"));
        assert!(control_space.contains("hl.unbind(\"CTRL + SHIFT + D\")"));
        assert!(control_space.contains("\"CTRL + SHIFT + D\""));
        assert!(control_space.contains("\"dicta annotate enable\""));
        assert!(control_space.contains("\"dicta annotate disable\""));
        assert!(control_space.contains("release = true"));
        assert!(control_space.contains("Dicta Annotation Overlay"));
        assert!(!control_space.contains("Dicta Annotation Helper"));
        assert!(control_space.contains("Dicta status"));
        assert!(control_space.contains("monitor_h-window_h-34"));
        assert!(control_space.contains("float = true"));
        assert!(!control_space.contains("--toggle-recording"));

        settings.shortcut_id = "command_shift_d".to_owned();
        assert!(integration.sync_if_installed(&settings).unwrap());
        let super_shift_d = fs::read_to_string(&destination).unwrap();
        assert!(super_shift_d.contains("SUPER + SHIFT + D"));
        assert!(!super_shift_d.contains("CTRL + SPACE"));
        assert!(!integration
            .sync_if_installed(&AppSettings {
                shortcut_id: "$(touch /tmp/nope)".to_owned(),
                ..AppSettings::default()
            })
            .unwrap_err()
            .message
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn symlinked_managed_module_is_rejected_without_touching_its_target() {
        let root = fixture();
        let target = root.join("outside.lua");
        fs::write(&target, "sentinel\n").unwrap();
        symlink(&target, root.join("hypr/dicta-bindings.lua")).unwrap();
        let error = OmarchyShortcutIntegration::new(&root)
            .sync_if_installed(&AppSettings::default())
            .unwrap_err();
        assert_eq!(error.kind, PortErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(target).unwrap(), "sentinel\n");
        fs::remove_dir_all(root).unwrap();
    }
}
