use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
};

static RECORDING_WRITES: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn write_recording_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let _guard = RECORDING_WRITES
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Recording storage is unavailable".to_string())?;
    let json = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Could not serialize recording metadata: {error}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid recording metadata path".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create recording storage: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid recording metadata filename".to_string())?;
    let staging = parent.join(format!(".{file_name}.writing-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&staging)
        .map_err(|error| format!("Could not stage recording metadata: {error}"))?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&json)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&staging, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Could not atomically save recording metadata: {error}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recording_json_replaces_existing_files_without_staging_leaks() {
        let root = std::env::temp_dir().join(format!(
            "dicta-atomic-write-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("recording.json");
        fs::write(&target, "old").unwrap();

        write_recording_json(&target, &json!({ "id": "recording" })).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(saved["id"], "recording");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
