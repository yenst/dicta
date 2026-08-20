use serde::Deserialize;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
