use crate::BranchMetadata;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchPaths {
    pub current: PathBuf,
    pub legacy: PathBuf,
}

pub fn folder_name(branch: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut folder = String::from("v2-");
    for byte in branch.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            folder.push(char::from(byte));
        } else {
            folder.push('%');
            folder.push(char::from(HEX[usize::from(byte >> 4)]));
            folder.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    folder
}

pub fn legacy_folder_name(branch: &str) -> String {
    let mut folder = String::new();
    for character in branch.chars() {
        match character {
            '/' => folder.push_str("__"),
            character
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') =>
            {
                folder.push(character)
            }
            _ => folder.push('-'),
        }
    }
    let folder = folder.trim_matches(['.', '-']).to_string();
    if folder.is_empty() {
        "unknown".to_string()
    } else {
        folder
    }
}

pub fn paths(branches_root: &Path, branch: &str) -> BranchPaths {
    BranchPaths {
        current: branches_root.join(folder_name(branch)),
        legacy: branches_root.join(legacy_folder_name(branch)),
    }
}

fn metadata_matches(path: &Path, branch: &str) -> bool {
    fs::read_to_string(path.join("branch.json"))
        .ok()
        .and_then(|content| serde_json::from_str::<BranchMetadata>(&content).ok())
        .is_some_and(|metadata| metadata.git_branch == branch)
}

pub fn existing_dirs(branches_root: &Path, branch: &str) -> Vec<PathBuf> {
    let paths = paths(branches_root, branch);
    let mut existing = Vec::new();
    if paths.current.is_dir() {
        existing.push(paths.current);
    }
    if paths.legacy.is_dir()
        && !existing.contains(&paths.legacy)
        && metadata_matches(&paths.legacy, branch)
    {
        existing.push(paths.legacy);
    }
    existing
}

pub fn preferred_dir(branches_root: &Path, branch: &str) -> PathBuf {
    existing_dirs(branches_root, branch)
        .into_iter()
        .next()
        .unwrap_or_else(|| paths(branches_root, branch).current)
}

pub fn copy_directory_missing(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_directory_missing(&source_path, &destination_path)?;
        } else if file_type.is_file() && !destination_path.exists() {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

pub fn migrate_legacy_dir(branches_root: &Path, branch: &str) -> io::Result<PathBuf> {
    let paths = paths(branches_root, branch);
    if paths.current.exists() {
        if paths.legacy.is_dir() && metadata_matches(&paths.legacy, branch) {
            copy_directory_missing(&paths.legacy, &paths.current)?;
            let _ = fs::remove_dir_all(&paths.legacy);
        }
        return Ok(paths.current);
    }
    if paths.legacy.is_dir() && metadata_matches(&paths.legacy, branch) {
        if fs::rename(&paths.legacy, &paths.current).is_ok() {
            return Ok(paths.current);
        }
        return Ok(paths.legacy);
    }
    Ok(paths.current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_encoding_is_reversible_in_shape_and_non_colliding() {
        assert_eq!(folder_name("feature/oauth"), "v2-feature%2Foauth");
        assert_eq!(folder_name("detached@abc"), "v2-detached%40abc");
        assert_ne!(folder_name("feature/oauth"), folder_name("feature__oauth"));
        assert_ne!(folder_name("a/b"), folder_name("a%2Fb"));
    }

    #[test]
    fn matching_legacy_packets_are_migrated_and_merged() {
        let root =
            std::env::temp_dir().join(format!("dicta-core-branch-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let branch = "feature/oauth";
        let paths = paths(&root, branch);
        fs::create_dir_all(paths.legacy.join("recordings")).unwrap();
        fs::write(
            paths.legacy.join("branch.json"),
            serde_json::json!({ "git_branch": branch }).to_string(),
        )
        .unwrap();
        fs::write(paths.legacy.join("recordings/keep.mp4"), "video").unwrap();

        let migrated = migrate_legacy_dir(&root, branch).unwrap();

        assert_eq!(migrated, paths.current);
        assert!(migrated.join("recordings/keep.mp4").is_file());
        assert!(!paths.legacy.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
