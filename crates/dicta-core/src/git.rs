use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitError(String);

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GitError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitAdminPaths {
    /// Worktree-specific administrative directory. This is `.git/` for a main
    /// checkout and `<common>/worktrees/<name>/` for a linked worktree.
    pub worktree: PathBuf,
    /// Shared repository administrative directory.
    pub common: PathBuf,
    /// Worktree-specific HEAD, used to prove that a redirection target is a
    /// Git worktree administration directory rather than an arbitrary folder.
    pub head: PathBuf,
}

/// Resolves the Git administrative directories for a normal checkout or a
/// linked worktree without invoking a shell or trusting an unconstrained
/// redirection file.
pub fn admin_paths(source_path: &Path) -> Result<GitAdminPaths, GitError> {
    let source = source_path
        .canonicalize()
        .map_err(|error| GitError(format!("Could not resolve the Git working copy: {error}")))?;
    let marker = source.join(".git");
    let metadata = fs::symlink_metadata(&marker)
        .map_err(|error| GitError(format!("Could not inspect Git metadata: {error}")))?;
    if metadata.file_type().is_symlink() {
        return Err(GitError("The .git marker cannot be a symlink".to_owned()));
    }

    if metadata.is_dir() {
        let common = marker
            .canonicalize()
            .map_err(|error| GitError(format!("Could not resolve Git metadata: {error}")))?;
        let head = validated_regular_file(&common.join("HEAD"), "Git HEAD")?;
        return Ok(GitAdminPaths {
            worktree: common.clone(),
            common,
            head,
        });
    }
    if !metadata.is_file() {
        return Err(GitError(
            "The .git marker must be a real directory or redirection file".to_owned(),
        ));
    }

    let redirection = read_small_text(&marker, "Git redirection")?;
    let target_text = redirection
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))
        .ok_or_else(|| GitError("The .git redirection is malformed".to_owned()))?;
    let target = PathBuf::from(target_text);
    let target = if target.is_absolute() {
        target
    } else {
        source.join(target)
    };
    let target_metadata = fs::symlink_metadata(&target).map_err(|error| {
        GitError(format!(
            "Could not inspect redirected Git metadata: {error}"
        ))
    })?;
    if target_metadata.file_type().is_symlink() || !target_metadata.is_dir() {
        return Err(GitError(
            "The redirected Git metadata must be a real directory".to_owned(),
        ));
    }
    let worktree = target.canonicalize().map_err(|error| {
        GitError(format!(
            "Could not resolve redirected Git metadata: {error}"
        ))
    })?;

    let common_marker = worktree.join("commondir");
    let common_text = read_small_text(&common_marker, "Git common-directory marker")?;
    let common_text = common_text.trim();
    if common_text.is_empty() || common_text.contains(['\n', '\r']) {
        return Err(GitError(
            "The Git common-directory marker is malformed".to_owned(),
        ));
    }
    let common_candidate = PathBuf::from(common_text);
    let common_candidate = if common_candidate.is_absolute() {
        common_candidate
    } else {
        worktree.join(common_candidate)
    };
    let common_metadata = fs::symlink_metadata(&common_candidate).map_err(|error| {
        GitError(format!(
            "Could not inspect the shared Git administration directory: {error}"
        ))
    })?;
    if common_metadata.file_type().is_symlink() || !common_metadata.is_dir() {
        return Err(GitError(
            "The shared Git administration path must be a real directory".to_owned(),
        ));
    }
    let common = common_candidate.canonicalize().map_err(|error| {
        GitError(format!(
            "Could not resolve the shared Git administration directory: {error}"
        ))
    })?;
    let worktrees = common.join("worktrees");
    let canonical_worktrees = worktrees
        .canonicalize()
        .map_err(|error| GitError(format!("Could not resolve Git worktree metadata: {error}")))?;
    if worktree.parent() != Some(canonical_worktrees.as_path()) {
        return Err(GitError(
            "The .git redirection escaped the repository worktree administration root".to_owned(),
        ));
    }

    let head = validated_regular_file(&worktree.join("HEAD"), "worktree Git HEAD")?;
    let backlink = validated_regular_file(&worktree.join("gitdir"), "worktree Git backlink")?;
    let backlink_text = read_small_text(&backlink, "worktree Git backlink")?;
    let backlink_path = PathBuf::from(backlink_text.trim());
    let backlink = backlink_path.canonicalize().map_err(|error| {
        GitError(format!(
            "Could not resolve the worktree Git backlink: {error}"
        ))
    })?;
    let marker = marker.canonicalize().map_err(|error| {
        GitError(format!(
            "Could not resolve the worktree .git marker: {error}"
        ))
    })?;
    if backlink != marker {
        return Err(GitError(
            "The redirected Git metadata does not point back to this worktree".to_owned(),
        ));
    }

    Ok(GitAdminPaths {
        worktree,
        common,
        head,
    })
}

fn read_small_text(path: &Path, label: &str) -> Result<String, GitError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| GitError(format!("Could not inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(GitError(format!(
            "{label} must be a small, regular, non-symlinked file"
        )));
    }
    fs::read_to_string(path).map_err(|error| GitError(format!("Could not read {label}: {error}")))
}

fn validated_regular_file(path: &Path, label: &str) -> Result<PathBuf, GitError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| GitError(format!("Could not inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(GitError(format!(
            "{label} must be a regular, non-symlinked file"
        )));
    }
    path.canonicalize()
        .map_err(|error| GitError(format!("Could not resolve {label}: {error}")))
}

pub fn output(source_path: &Path, arguments: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source_path)
        .args(arguments)
        .output()
        .map_err(|error| GitError(format!("Could not run Git: {error}")))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError(if message.is_empty() {
            "The selected folder is not a Git working copy".to_string()
        } else {
            message
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn root(source_path: &Path) -> Result<PathBuf, GitError> {
    PathBuf::from(output(source_path, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|error| GitError(format!("Could not resolve the Git working copy: {error}")))
}

pub fn branch(source_path: &Path) -> Result<String, GitError> {
    match output(source_path, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if !branch.is_empty() => Ok(branch),
        _ => {
            let revision = output(source_path, &["rev-parse", "--short", "HEAD"])?;
            Ok(format!("detached@{revision}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(arguments: &[&str]) {
        assert!(Command::new("git")
            .args(arguments)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn discovers_root_branch_and_detached_revision() {
        let test_root =
            std::env::temp_dir().join(format!("dicta-core-git-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&test_root);
        assert!(Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&test_root)
            .status()
            .unwrap()
            .success());
        assert_eq!(root(&test_root).unwrap(), test_root.canonicalize().unwrap());
        assert_eq!(branch(&test_root).unwrap(), "main");
        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn resolves_a_real_linked_worktree_and_rejects_an_unproven_redirect() {
        let test_root = std::env::temp_dir().join(format!(
            "dicta-core-worktree-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let repository = test_root.join("repository");
        let worktree = test_root.join("linked");
        let _ = fs::remove_dir_all(&test_root);
        git(&["init", "-b", "main", repository.to_str().unwrap()]);
        git(&[
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.email",
            "dicta@example.invalid",
        ]);
        git(&[
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.name",
            "Dicta Test",
        ]);
        fs::write(repository.join("tracked"), "fixture").unwrap();
        git(&["-C", repository.to_str().unwrap(), "add", "tracked"]);
        git(&[
            "-C",
            repository.to_str().unwrap(),
            "commit",
            "-m",
            "fixture",
        ]);
        git(&[
            "-C",
            repository.to_str().unwrap(),
            "worktree",
            "add",
            "-b",
            "linked-branch",
            worktree.to_str().unwrap(),
        ]);

        let main = admin_paths(&repository).unwrap();
        assert_eq!(main.common, main.worktree);
        let linked = admin_paths(&worktree).unwrap();
        assert_ne!(linked.common, linked.worktree);
        assert_eq!(linked.common, main.common);
        assert_eq!(branch(&worktree).unwrap(), "linked-branch");
        assert_eq!(root(&worktree).unwrap(), worktree.canonicalize().unwrap());

        let malicious = test_root.join("malicious");
        fs::create_dir(&malicious).unwrap();
        fs::write(
            malicious.join(".git"),
            format!("gitdir: {}\n", linked.common.display()),
        )
        .unwrap();
        assert!(admin_paths(&malicious).is_err());
        fs::remove_dir_all(test_root).unwrap();
    }
}
