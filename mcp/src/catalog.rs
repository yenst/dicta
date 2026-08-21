use dicta_core::{catalog, storage, ProjectFile, ProjectId, GENERAL_PROJECT_ID};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub(crate) struct CatalogProject {
    pub(crate) project: ProjectFile,
    pub(crate) storage_path: PathBuf,
}

pub(crate) fn load() -> Result<Vec<CatalogProject>, String> {
    load_at(&crate::context::dicta_root()?)
}

pub(crate) fn load_at(root: &Path) -> Result<Vec<CatalogProject>, String> {
    reject_symlink(root, "Dicta storage root")?;
    let mut projects = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(general_only(root))
        }
        Err(error) => {
            return Err(format!(
                "Could not read Dicta storage `{}`: {error}",
                root.display()
            ));
        }
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let metadata = entry.path().join("project.json");
        let Ok(metadata_type) = fs::symlink_metadata(&metadata).map(|value| value.file_type())
        else {
            continue;
        };
        if !metadata_type.is_file() || metadata_type.is_symlink() {
            continue;
        }
        let Ok(project) = storage::read_json::<ProjectFile>(&metadata) else {
            continue;
        };
        if project.id.as_str() == GENERAL_PROJECT_ID
            || project.id.as_str() != entry.file_name().to_string_lossy().as_ref()
            || project.name.trim().is_empty()
        {
            continue;
        }
        projects.push(CatalogProject {
            project,
            storage_path: entry.path(),
        });
    }
    projects.sort_by(|left, right| {
        left.project
            .name
            .to_lowercase()
            .cmp(&right.project.name.to_lowercase())
            .then_with(|| left.project.id.cmp(&right.project.id))
    });
    projects.extend(general_only(root));
    Ok(projects)
}

fn general_only(root: &Path) -> Vec<CatalogProject> {
    let settings = catalog::load_general_settings(root);
    let path = storage::general_storage_path(root, settings.general_path.as_deref());
    vec![CatalogProject {
        project: ProjectFile {
            id: ProjectId::new(GENERAL_PROJECT_ID)
                .unwrap_or_else(|_| unreachable!("core General project ID is valid")),
            name: "General".to_owned(),
            created_at: std::time::UNIX_EPOCH.into(),
            source_path: Some(path.to_string_lossy().into_owned()),
            extra: serde_json::Map::new(),
        },
        storage_path: path,
    }]
}

pub(crate) fn current(
    projects: &[CatalogProject],
    repo_path: Option<&str>,
) -> Result<Option<CatalogProject>, String> {
    let requested = repo_path
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(
            std::env::current_dir()
                .map_err(|error| format!("Could not determine the current directory: {error}"))?,
        );
    let repo_root = match dicta_core::git::root(&requested) {
        Ok(root) => root,
        Err(_) => return Ok(None),
    };
    let local_storage = repo_root.join(".dicta");
    let local_metadata = local_storage.join("project.json");
    match fs::symlink_metadata(&local_metadata) {
        Ok(metadata) => {
            reject_symlink(&local_storage, "Repository-local Dicta storage")?;
            reject_symlink(&local_metadata, "Repository-local Dicta project metadata")?;
            if !metadata.is_file() {
                return Err(format!(
                    "Repository-local Dicta project metadata is not a regular file: `{}`",
                    local_metadata.display()
                ));
            }
            let project = storage::read_json::<ProjectFile>(&local_metadata)?;
            return Ok(Some(CatalogProject {
                project,
                storage_path: local_storage,
            }));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Could not inspect repository-local Dicta metadata `{}`: {error}",
                local_metadata.display()
            ));
        }
    }
    Ok(projects.iter().find_map(|candidate| {
        let source = candidate.project.source_path.as_deref()?;
        let canonical = Path::new(source).canonicalize().ok()?;
        (canonical == repo_root).then(|| candidate.clone())
    }))
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!(
            "{label} must not be a symlink: `{}`",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, name: &str, source_path: Option<String>) -> ProjectFile {
        ProjectFile {
            id: ProjectId::new(id).unwrap(),
            name: name.to_owned(),
            created_at: std::time::UNIX_EPOCH.into(),
            source_path,
            extra: serde_json::Map::new(),
        }
    }

    fn init_git(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn catalog_rejects_symlinked_registrations_and_adds_general() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let valid = root.path().join("valid");
        fs::create_dir_all(&valid).unwrap();
        storage::write_json_atomic(
            &valid.join("project.json"),
            &project("valid", "Valid", None),
        )
        .unwrap();
        symlink(&valid, root.path().join("linked")).unwrap();

        let projects = load_at(root.path()).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].project.id.as_str(), "valid");
        assert_eq!(projects[1].project.id.as_str(), GENERAL_PROJECT_ID);
    }

    #[test]
    fn current_project_uses_confined_repository_metadata() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        init_git(&repo);
        let local = repo.join(".dicta");
        fs::create_dir(&local).unwrap();
        let metadata = project(
            "repo",
            "Repository",
            Some(repo.to_string_lossy().into_owned()),
        );
        storage::write_json_atomic(&local.join("project.json"), &metadata).unwrap();

        let current = current(&[], Some(repo.to_str().unwrap())).unwrap().unwrap();
        assert_eq!(current.project.id.as_str(), "repo");
        assert_eq!(current.storage_path, local);
    }

    #[test]
    fn symlinked_repository_metadata_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        init_git(&repo);
        let local = repo.join(".dicta");
        fs::create_dir(&local).unwrap();
        let outside = root.path().join("outside.json");
        storage::write_json_atomic(&outside, &project("outside", "Outside", None)).unwrap();
        symlink(&outside, local.join("project.json")).unwrap();

        let error = current(&[], Some(repo.to_str().unwrap())).unwrap_err();
        assert!(error.contains("must not be a symlink"));
    }
}
