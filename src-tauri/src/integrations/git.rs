use crate::*;

pub(crate) fn linked_storage_dir(metadata: &ProjectFile) -> Option<PathBuf> {
    metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .map(|source| source.join(".dicta"))
}

pub(crate) fn project_storage_dir(root: &Path, metadata: &ProjectFile) -> PathBuf {
    linked_storage_dir(metadata).unwrap_or_else(|| project_dir(root, &metadata.id))
}

pub(crate) fn copy_directory_missing(source: &Path, destination: &Path) -> Result<(), String> {
    core_branch::copy_directory_missing(source, destination).map_err(|error| {
        format!(
            "Could not copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

pub(crate) fn rewrite_migrated_paths(directory: &Path, old_root: &Path, new_root: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            rewrite_migrated_paths(&path, old_root, new_root);
            continue;
        }
        if !file_type.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let mut changed = false;
        if value.get("metadata_path").is_some() {
            let local_metadata_path = path_string(&path);
            if value.get("metadata_path").and_then(|item| item.as_str())
                != Some(local_metadata_path.as_str())
            {
                value["metadata_path"] = serde_json::Value::String(local_metadata_path);
                changed = true;
            }
        }
        let local_video_path = path.with_extension("mp4");
        if local_video_path.is_file() {
            let local_video_path = path_string(&local_video_path);
            if value.get("video_path").and_then(|item| item.as_str())
                != Some(local_video_path.as_str())
            {
                value["video_path"] = serde_json::Value::String(local_video_path);
                changed = true;
            }
        }
        for key in [
            "video_path",
            "metadata_path",
            "transcript_path",
            "poster_path",
        ] {
            let Some(raw_path) = value.get(key).and_then(|item| item.as_str()) else {
                continue;
            };
            let original = Path::new(raw_path);
            let Ok(relative) = original.strip_prefix(old_root) else {
                continue;
            };
            value[key] = serde_json::Value::String(path_string(&new_root.join(relative)));
            changed = true;
        }
        if changed {
            if let Ok(json) = serde_json::to_string_pretty(&value) {
                let _ = fs::write(&path, format!("{json}\n"));
            }
        }
    }
}

pub(crate) fn exclude_dicta_from_git(source: &Path) -> Result<(), String> {
    let raw_path = git_output(source, &["rev-parse", "--git-path", "info/exclude"])?;
    let exclude_path = {
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            path
        } else {
            source.join(path)
        }
    };
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".dicta/") {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not open Git exclude rules: {error}"))?;
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(
        &exclude_path,
        format!("{existing}{separator}# Dicta local recordings\n.dicta/\n"),
    )
    .map_err(|error| format!("Could not exclude .dicta from Git: {error}"))
}

pub(crate) fn prepare_linked_storage(
    root: &Path,
    metadata: &ProjectFile,
) -> Result<PathBuf, String> {
    let source = metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "This project is not linked to Git".to_string())?;
    let destination = source.join(".dicta");
    fs::create_dir_all(&destination)
        .map_err(|error| format!("Could not create repository-local Dicta storage: {error}"))?;

    let json = serde_json::to_string_pretty(metadata)
        .map_err(|error| format!("Could not serialize linked project: {error}"))?;
    fs::write(destination.join("project.json"), format!("{json}\n")).map_err(|error| {
        format!("Could not publish project metadata to the repository: {error}")
    })?;

    let legacy = project_dir(root, &metadata.id);
    let legacy_branches = legacy.join("branches");
    let destination_branches = destination.join("branches");
    copy_directory_missing(&legacy_branches, &destination_branches)?;
    rewrite_migrated_paths(&destination_branches, &legacy, &destination);
    exclude_dicta_from_git(source)?;
    Ok(destination)
}

pub(crate) fn git_output(source_path: &Path, arguments: &[&str]) -> Result<String, String> {
    core_git::output(source_path, arguments).map_err(|error| error.to_string())
}

pub(crate) fn git_root(source_path: &Path) -> Result<PathBuf, String> {
    core_git::root(source_path).map_err(|error| error.to_string())
}

pub(crate) fn git_branch(source_path: &Path) -> Result<String, String> {
    core_git::branch(source_path).map_err(|error| error.to_string())
}

pub(crate) fn git_revision(source_path: &Path) -> Option<String> {
    git_output(source_path, &["rev-parse", "HEAD"])
        .ok()
        .filter(|revision| !revision.is_empty())
}

pub(crate) fn git_ref_exists(source_path: &Path, reference: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn default_git_branch(source_path: &Path) -> Option<String> {
    if let Ok(remote_head) = git_output(
        source_path,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        if let Some(branch) = remote_head.strip_prefix("origin/") {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|branch| git_ref_exists(source_path, &format!("refs/heads/{branch}")))
        .map(str::to_string)
}

pub(crate) fn revision_is_merged(source_path: &Path, revision: &str, default_branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(["merge-base", "--is-ancestor", revision, default_branch])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn resolved_branch_dir(metadata: &ProjectFile, branch: &str) -> Result<PathBuf, String> {
    let branches = linked_storage_dir(metadata)
        .ok_or_else(|| "This is a legacy unlinked project".to_string())?
        .join("branches");
    let paths = core_branch::paths(&branches, branch);
    let resolved = core_branch::migrate_legacy_dir(&branches, branch)
        .map_err(|error| format!("Could not migrate branch recording storage: {error}"))?;
    if resolved == paths.current {
        rewrite_migrated_paths(&resolved, &paths.legacy, &paths.current);
    }
    Ok(resolved)
}

pub(crate) fn linked_branch_dir(
    _root: &Path,
    metadata: &ProjectFile,
) -> Result<(String, PathBuf), String> {
    let source_path = metadata
        .source_path
        .as_ref()
        .ok_or_else(|| "This is a legacy unlinked project".to_string())?;
    let branch = git_branch(Path::new(source_path))?;
    let path = resolved_branch_dir(metadata, &branch)?;
    Ok((branch, path))
}

pub(crate) fn active_recording_root(
    root: &Path,
    metadata: &ProjectFile,
) -> Result<(Option<String>, PathBuf), String> {
    if metadata.source_path.is_some() {
        if read_settings(root).branch_locking {
            let (branch, path) = linked_branch_dir(root, metadata)?;
            Ok((Some(branch), path))
        } else {
            Ok((None, project_storage_dir(root, metadata)))
        }
    } else {
        Ok((None, project_dir(root, &metadata.id)))
    }
}

pub(crate) fn write_branch_metadata(
    path: &Path,
    branch: &str,
    source_path: Option<&Path>,
) -> Result<(), String> {
    fs::create_dir_all(path.join("recordings"))
        .map_err(|error| format!("Could not create branch packet folder: {error}"))?;
    let folder_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| core_branch::folder_name(branch));
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "git_branch": branch,
        "folder_name": folder_name,
        "head_oid": source_path.and_then(git_revision),
        "updated_at": Utc::now(),
    }))
    .map_err(|error| format!("Could not serialize branch metadata: {error}"))?;
    fs::write(path.join("branch.json"), format!("{json}\n"))
        .map_err(|error| format!("Could not save branch metadata: {error}"))
}

pub(crate) fn remove_video_files(directory: &Path) -> Result<(usize, u64), String> {
    let mut removed_files = 0;
    let mut freed_bytes = 0;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("Could not read {}: {error}", directory.display())),
    };
    for entry in entries.flatten() {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let (nested_files, nested_bytes) = remove_video_files(&entry.path())?;
            removed_files += nested_files;
            freed_bytes += nested_bytes;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("mp4")
        {
            let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "Could not remove merged recording {}: {error}",
                    entry.path().display()
                )
            })?;
            removed_files += 1;
            freed_bytes += size;
        }
    }
    Ok((removed_files, freed_bytes))
}

pub(crate) fn cleanup_merged_videos_for_project(
    root: &Path,
    metadata: &ProjectFile,
) -> Result<CleanupSummary, String> {
    let settings = read_settings(root);
    if !settings.cleanup_merged_videos {
        return Ok(CleanupSummary {
            message: "Merged-video cleanup is off.".to_string(),
            ..CleanupSummary::default()
        });
    }
    let source_path = metadata
        .source_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "This project is not linked to Git".to_string())?;
    let default_branch = default_git_branch(source_path)
        .ok_or_else(|| "Could not determine the repository default branch".to_string())?;
    let active_branch = git_branch(source_path).ok();
    let storage_path = linked_storage_dir(metadata)
        .ok_or_else(|| "This project is not linked to Git".to_string())?;
    if fs::symlink_metadata(&storage_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Refusing symlinked repository-local Dicta storage".to_string());
    }
    let canonical_source = source_path
        .canonicalize()
        .map_err(|error| format!("Could not resolve linked project: {error}"))?;
    let canonical_storage = storage_path
        .canonicalize()
        .map_err(|error| format!("Could not resolve linked recording storage: {error}"))?;
    if !canonical_storage.starts_with(&canonical_source) {
        return Err("Recording storage escaped the linked repository".to_string());
    }
    let branches_path = canonical_storage.join("branches");
    if fs::symlink_metadata(&branches_path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("Refusing symlinked branch recording storage".to_string());
    }
    let mut summary = CleanupSummary {
        default_branch: Some(default_branch.clone()),
        ..CleanupSummary::default()
    };
    let entries = match fs::read_dir(&branches_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            summary.message = "No branch recordings to clean.".to_string();
            return Ok(summary);
        }
        Err(error) => return Err(format!("Could not inspect branch recordings: {error}")),
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let branch_path = entry.path();
        let Ok(content) = fs::read_to_string(branch_path.join("branch.json")) else {
            continue;
        };
        let Ok(branch_metadata) = serde_json::from_str::<BranchMetadata>(&content) else {
            continue;
        };
        if branch_metadata.git_branch == default_branch
            || active_branch.as_deref() == Some(branch_metadata.git_branch.as_str())
        {
            continue;
        }
        let revision = branch_metadata.head_oid.or_else(|| {
            let reference = format!("refs/heads/{}", branch_metadata.git_branch);
            git_ref_exists(source_path, &reference).then_some(reference)
        });
        let Some(revision) = revision else {
            continue;
        };
        if !revision_is_merged(source_path, &revision, &default_branch) {
            continue;
        }
        let (removed_files, freed_bytes) = remove_video_files(&branch_path.join("recordings"))?;
        if removed_files > 0 {
            summary.removed_files += removed_files;
            summary.freed_bytes += freed_bytes;
            summary.cleaned_branches.push(branch_metadata.git_branch);
        }
    }
    summary.message = if summary.removed_files == 0 {
        format!("No merged videos found for {default_branch}.")
    } else {
        format!(
            "Removed {} merged video{}.",
            summary.removed_files,
            if summary.removed_files == 1 { "" } else { "s" }
        )
    };
    Ok(summary)
}
