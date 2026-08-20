use super::atomic;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

type PathLock = Arc<Mutex<()>>;
static PATH_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

fn lock_for(path: &Path) -> Result<PathLock, String> {
    let mut locks = PATH_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Recording lock registry is unavailable".to_string())?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    Ok(lock)
}

fn read_unlocked<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_str(&content).map_err(|error| format!("Invalid recording metadata: {error}"))
}

pub(crate) fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let path_lock = lock_for(path)?;
    let _guard = path_lock
        .lock()
        .map_err(|_| "Recording update lock is unavailable".to_string())?;
    atomic::write_recording_json(path, value)
}

pub(crate) fn update<T, R>(
    path: &Path,
    update: impl FnOnce(&mut T) -> Result<R, String>,
) -> Result<(T, R), String>
where
    T: DeserializeOwned + Serialize,
{
    let path_lock = lock_for(path)?;
    let _guard = path_lock
        .lock()
        .map_err(|_| "Recording update lock is unavailable".to_string())?;
    let mut value = read_unlocked(path)?;
    let result = update(&mut value)?;
    atomic::write_recording_json(path, &value)?;
    Ok((value, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::thread;

    #[derive(Debug, Deserialize, Serialize)]
    struct Fixture {
        notes: usize,
        transcript: bool,
    }

    #[test]
    fn concurrent_read_modify_write_updates_do_not_lose_fields() {
        let root = std::env::temp_dir().join(format!(
            "dicta-recording-transaction-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("recording.json");
        write(
            &path,
            &Fixture {
                notes: 0,
                transcript: false,
            },
        )
        .unwrap();

        let notes_path = path.clone();
        let notes = thread::spawn(move || {
            update::<Fixture, _>(&notes_path, |recording| {
                thread::sleep(std::time::Duration::from_millis(20));
                recording.notes = 3;
                Ok(())
            })
            .unwrap();
        });
        let transcript_path = path.clone();
        let transcript = thread::spawn(move || {
            update::<Fixture, _>(&transcript_path, |recording| {
                recording.transcript = true;
                Ok(())
            })
            .unwrap();
        });
        notes.join().unwrap();
        transcript.join().unwrap();

        let saved: Fixture = read_unlocked(&path).unwrap();
        assert_eq!(saved.notes, 3);
        assert!(saved.transcript);
        fs::remove_dir_all(root).unwrap();
    }
}
