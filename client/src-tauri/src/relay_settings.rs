use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};

pub trait RelaySettingsStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, relay_url: &str) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub struct FileRelaySettingsStore {
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct RelaySettingsRecord {
    relay_url: String,
}

impl FileRelaySettingsStore {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn ensure_parent_dir(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(store_error)
    }
}

impl RelaySettingsStore for FileRelaySettingsStore {
    fn load(&self) -> Result<Option<String>, String> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => {
                let record = serde_json::from_str::<RelaySettingsRecord>(&contents)
                    .map_err(|_| "relay settings are not valid JSON".to_owned())?;
                (!record.relay_url.trim().is_empty())
                    .then_some(record.relay_url)
                    .ok_or_else(|| "relay settings URL is empty".to_owned())
                    .map(Some)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn save(&self, relay_url: &str) -> Result<(), String> {
        if relay_url.trim().is_empty() {
            return Err("relay settings URL is empty".to_owned());
        }
        self.ensure_parent_dir()?;
        let contents = serde_json::to_vec(&RelaySettingsRecord {
            relay_url: relay_url.to_owned(),
        })
        .map_err(|_| "relay settings cannot be serialized".to_owned())?;
        let mut file = AtomicWriteFile::open(&self.path).map_err(store_error)?;
        file.write_all(&contents).map_err(store_error)?;
        file.sync_all().map_err(store_error)?;
        file.commit().map_err(store_error)
    }
}

fn store_error(error: std::io::Error) -> String {
    format!("relay settings file operation failed: {error}")
}
