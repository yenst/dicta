use crate::StorageLayout;
use chrono::{DateTime, Utc};
use dicta_core::{catalog, RecordingId};
use dicta_runtime::{IdSource, PortError, PortErrorKind};
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
    time::SystemTime,
};

#[derive(Debug)]
pub struct FilesystemIdSource {
    layout: StorageLayout,
}

impl FilesystemIdSource {
    #[must_use]
    pub const fn new(layout: StorageLayout) -> Self {
        Self { layout }
    }
}

impl IdSource for FilesystemIdSource {
    fn next_recording_id(&mut self, now: SystemTime) -> Result<RecordingId, PortError> {
        let reservation_directory = self.layout.reservation_directory();
        fs::create_dir_all(&reservation_directory)
            .map_err(|error| io_port_error("create recording ID reservation directory", &error))?;
        let timestamp = DateTime::<Utc>::from(now).format("%Y%m%d-%H-%M-%S");
        for sequence in 0..10_000_u16 {
            let candidate = if sequence == 0 {
                timestamp.to_string()
            } else {
                format!("{timestamp}-{sequence:04}")
            };
            let id = RecordingId::new(candidate).map_err(|error| {
                PortError::new(
                    PortErrorKind::Internal,
                    format!("generated an invalid recording ID: {error}"),
                )
            })?;
            let reservation = reservation_directory.join(id.as_str());
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(reservation)
            {
                Ok(_) if recording_exists(self.layout.root(), &id)? => {}
                Ok(_) => return Ok(id),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_port_error("reserve recording ID", &error)),
            }
        }
        Err(PortError::new(
            PortErrorKind::Internal,
            "could not reserve a collision-free recording ID after 10000 attempts",
        ))
    }
}

fn recording_exists(root: &Path, id: &RecordingId) -> Result<bool, PortError> {
    let mut sources = catalog::registered_sources(root);
    sources.extend(catalog::general_sources(root));
    catalog::deduplicate_sources(&mut sources);
    for source in sources {
        for tree in catalog::recording_trees(&source) {
            if recording_artifact_exists(&tree, id)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn recording_artifact_exists(tree: &Path, id: &RecordingId) -> Result<bool, PortError> {
    let days = match fs::read_dir(tree.join("recordings")) {
        Ok(days) => days,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_port_error("scan recording directories", &error)),
    };
    for day in days {
        let day = day.map_err(|error| io_port_error("inspect recording directory", &error))?;
        let day_type = day
            .file_type()
            .map_err(|error| io_port_error("inspect recording directory type", &error))?;
        if !day_type.is_dir() || day_type.is_symlink() {
            continue;
        }
        if day.path().join(format!("{id}.json")).is_file()
            || day.path().join(format!("{id}.mp4")).is_file()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn io_port_error(action: &str, error: &io::Error) -> PortError {
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
    use std::{fs, time::UNIX_EPOCH};

    #[test]
    fn reservations_are_collision_safe_for_equal_timestamps() {
        let root = std::env::temp_dir().join(format!(
            "dicta-linux-id-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut ids = FilesystemIdSource::new(StorageLayout::new(&root));
        let first = ids.next_recording_id(UNIX_EPOCH).unwrap();
        let second = ids.next_recording_id(UNIX_EPOCH).unwrap();
        assert_eq!(first.as_str(), "19700101-00-00-00");
        assert_eq!(second.as_str(), "19700101-00-00-00-0001");
        assert_eq!(fs::read_dir(root.join(".ids")).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_recording_without_a_reservation_is_never_reused() {
        let root = std::env::temp_dir().join(format!(
            "dicta-linux-existing-id-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let existing = root.join("General/recordings/1970-01-01/19700101-00-00-00.json");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(existing, b"{}").unwrap();
        let mut ids = FilesystemIdSource::new(StorageLayout::new(&root));
        let id = ids.next_recording_id(UNIX_EPOCH).unwrap();
        assert_eq!(id.as_str(), "19700101-00-00-00-0001");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_linked_recording_is_never_reused() {
        let root = std::env::temp_dir().join(format!(
            "dicta-linux-linked-id-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let repository = root.join("repo");
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let existing = repository.join(".dicta/recordings/1970-01-01/19700101-00-00-00.mp4");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, b"video").unwrap();
        dicta_core::storage::write_json_atomic(
            &storage.join("demo/project.json"),
            &dicta_core::ProjectFile {
                id: dicta_core::ProjectId::new("demo").unwrap(),
                name: "Demo".to_owned(),
                created_at: std::time::UNIX_EPOCH.into(),
                source_path: Some(repository.to_string_lossy().into_owned()),
                extra: serde_json::Map::new(),
            },
        )
        .unwrap();
        let mut ids = FilesystemIdSource::new(StorageLayout::new(&storage));
        let id = ids.next_recording_id(UNIX_EPOCH).unwrap();
        assert_eq!(id.as_str(), "19700101-00-00-00-0001");
        fs::remove_dir_all(root).unwrap();
    }
}
