use std::{
    fmt,
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
}
