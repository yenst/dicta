use crate::{ALLOWED_LANGUAGES, DEFAULT_LANGUAGE, DEFAULT_SHORTCUT_ID};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    #[serde(default = "default_shortcut_id")]
    pub(crate) shortcut_id: String,
    #[serde(default = "enabled_by_default")]
    pub(crate) cleanup_merged_videos: bool,
    #[serde(default = "enabled_by_default")]
    pub(crate) branch_locking: bool,
    #[serde(default = "default_language")]
    pub(crate) transcription_language: String,
    #[serde(default)]
    pub(crate) general_path: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut_id: default_shortcut_id(),
            cleanup_merged_videos: true,
            branch_locking: true,
            transcription_language: default_language(),
            general_path: None,
        }
    }
}

fn default_shortcut_id() -> String {
    DEFAULT_SHORTCUT_ID.to_string()
}

fn enabled_by_default() -> bool {
    true
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}

pub(crate) fn is_allowed_language(language: &str) -> bool {
    ALLOWED_LANGUAGES.contains(&language)
}

pub(crate) fn normalize(mut settings: AppSettings) -> AppSettings {
    #[cfg(target_os = "linux")]
    if settings.shortcut_id == "command_shift_r" {
        settings.shortcut_id = DEFAULT_SHORTCUT_ID.to_string();
    }
    if shortcut_for_id(&settings.shortcut_id).is_none() {
        settings.shortcut_id = default_shortcut_id();
    }
    if !is_allowed_language(&settings.transcription_language) {
        settings.transcription_language = default_language();
    }
    settings
}

fn path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub(crate) fn read(root: &Path) -> AppSettings {
    fs::read_to_string(path(root))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .map(normalize)
        .unwrap_or_default()
}

pub(crate) fn language(root: &Path) -> String {
    read(root).transcription_language
}

pub(crate) fn write(root: &Path, settings: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("Could not serialize Dicta settings: {error}"))?;
    fs::write(path(root), format!("{json}\n"))
        .map_err(|error| format!("Could not save Dicta settings: {error}"))
}

pub(crate) fn shortcut_for_id(id: &str) -> Option<Shortcut> {
    match id {
        "command_shift_r" => Some(Shortcut::new(
            Some(Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyR,
        )),
        "alt_shift_r" => Some(Shortcut::new(
            Some(Modifiers::ALT | Modifiers::SHIFT),
            Code::KeyR,
        )),
        "command_shift_d" => Some(Shortcut::new(
            Some(Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyD,
        )),
        "option_space" => Some(Shortcut::new(Some(Modifiers::ALT), Code::Space)),
        "control_space" => Some(Shortcut::new(Some(Modifiers::CONTROL), Code::Space)),
        _ => None,
    }
}
