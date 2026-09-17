use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use chrono::Datelike as _;

use crate::model::{Store, CURRENT_FORMAT_VERSION};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("could not decode {path}: {source}")]
    Decode {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path} uses unsupported data format version {version}")]
    UnsupportedVersion { path: PathBuf, version: u32 },
    #[error("invalid data in {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("refusing to save invalid data: {0}")]
    Invalid(String),
    #[error("could not encode tracker data: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("could not save {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
}

#[derive(Debug, Default)]
pub struct SaveOutcome {
    pub backup_warning: Option<String>,
}

#[derive(Clone, Debug)]
pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A missing file starts a new tracker. Every other load failure is fatal so
    /// existing data can never be replaced by an accidental empty store.
    pub fn load(&self) -> Result<Store, LoadError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Store::default()),
            Err(source) => {
                return Err(LoadError::Read {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let mut store: Store =
            serde_json::from_slice(&bytes).map_err(|source| LoadError::Decode {
                path: self.path.clone(),
                source,
            })?;
        match store.format_version {
            1 => store = store.upgrade_v1(),
            CURRENT_FORMAT_VERSION => {}
            version => {
                return Err(LoadError::UnsupportedVersion {
                    path: self.path.clone(),
                    version,
                });
            }
        }
        store.validate().map_err(|error| LoadError::Invalid {
            path: self.path.clone(),
            reason: error.to_string(),
        })?;
        Ok(store)
    }

    pub fn save(&self, store: &Store) -> Result<SaveOutcome, SaveError> {
        store
            .validate()
            .map_err(|error| SaveError::Invalid(error.to_string()))?;
        let backup_warning = back_up_if_due(&self.path).err();
        let mut json = serde_json::to_vec_pretty(store)?;
        json.push(b'\n');
        atomic_write(&self.path, &json).map_err(|source| SaveError::Write {
            path: self.path.clone(),
            source,
        })?;
        Ok(SaveOutcome { backup_warning })
    }
}

fn back_up_if_due(source: &Path) -> Result<(), String> {
    if !source.is_file() {
        return Ok(());
    }
    let week = chrono::Local::now().iso_week();
    let directory = source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups");
    back_up_for_week(source, &directory, week.year(), week.week())
}

fn back_up_for_week(source: &Path, directory: &Path, year: i32, week: u32) -> Result<(), String> {
    let backup = directory.join(format!("data-{year}-W{week:02}.json"));
    if backup.exists() {
        return Ok(());
    }
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create the backup folder: {error}"))?;
    let mut temporary = AtomicWriteFile::open(&backup)
        .map_err(|error| format!("could not create the weekly backup: {error}"))?;
    let bytes = fs::read(source)
        .map_err(|error| format!("could not read data for the weekly backup: {error}"))?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.commit())
        .map_err(|error| format!("could not finish the weekly backup: {error}"))
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(contents)?;
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Activity, ActivityId, EntryId, TimeEntry, TimeRange, UnixSeconds};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "opto-tracker-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn malformed_existing_file_is_not_treated_as_empty() {
        let directory = test_directory("malformed");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("data.json");
        fs::write(&path, b"not JSON").unwrap();

        assert!(matches!(
            JsonStore::new(path).load(),
            Err(LoadError::Decode { .. })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn version_one_entries_receive_stable_ids() {
        let directory = test_directory("upgrade");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("data.json");
        fs::write(
            &path,
            br#"{
                "format_version": 1,
                "projects": [],
                "activities": [{"name":"Work"}],
                "time_entries": [{
                    "activity": 0,
                    "project": null,
                    "time": {"started_at": 10, "stopped_at": 20}
                }],
                "active_entry": null
            }"#,
        )
        .unwrap();

        let store = JsonStore::new(path).load().unwrap();
        assert_eq!(store.format_version, CURRENT_FORMAT_VERSION);
        assert_eq!(store.time_entries[0].id, EntryId(1));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saving_replaces_existing_contents() {
        let directory = test_directory("atomic");
        let path = directory.join("data.json");
        let repository = JsonStore::new(path.clone());
        let mut store = Store {
            activities: vec![Activity {
                name: "Work".to_owned(),
            }],
            ..Store::default()
        };
        store.time_entries.push(TimeEntry {
            id: EntryId(1),
            activity: ActivityId(0),
            project: None,
            time: TimeRange {
                started_at: UnixSeconds(10),
                stopped_at: UnixSeconds(20),
            },
        });

        repository.save(&store).unwrap();
        assert_eq!(repository.load().unwrap(), store);
        fs::remove_dir_all(directory).unwrap();
    }
}
