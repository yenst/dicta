use crate::StorageLayout;
use chrono::{DateTime, Utc};
use dicta_core::RecordingId;
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
    let projects =
        fs::read_dir(root).map_err(|error| io_port_error("scan recording IDs", &error))?;
    for project in projects {
        let project = project.map_err(|error| io_port_error("inspect storage entry", &error))?;
        let project_type = project
            .file_type()
            .map_err(|error| io_port_error("inspect storage entry type", &error))?;
        if !project_type.is_dir() || project_type.is_symlink() {
            continue;
        }
        let days = match fs::read_dir(project.path().join("recordings")) {
            Ok(days) => days,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
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
}
