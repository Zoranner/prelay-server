use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use atomic_write_file::AtomicWriteFile;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CredentialRecord {
    pub current: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
}

pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<CredentialRecord>, String>;
    fn save_initial(&self, credential: &str) -> Result<CredentialRecord, String>;
    fn begin_rotation(&self, credential: &str) -> Result<(), String>;
    fn complete_rotation(&self, expected_credential: &str) -> Result<(), String>;
    fn confirm_pending(&self) -> Result<(), String>;
    fn discard_pending(&self) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub struct FileCredentialStore {
    path: PathBuf,
    lock_path: PathBuf,
}

impl FileCredentialStore {
    pub fn at(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let lock_name = format!(
            "{}.lock",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("device-credential.json")
        );
        let lock_path = path.with_file_name(lock_name);
        Self { path, lock_path }
    }

    fn with_lock<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(store_error)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(store_error)?;
        lock.lock_exclusive().map_err(store_error)?;
        let result = operation();
        let unlock_result = FileExt::unlock(&lock).map_err(store_error);
        match (result, unlock_result) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }

    fn read_record(&self) -> Result<Option<CredentialRecord>, String> {
        match fs::read_to_string(&self.path) {
            Ok(value) => {
                if value.trim().is_empty() {
                    return Err("credential record is empty".into());
                }
                let record = serde_json::from_str::<CredentialRecord>(&value)
                    .map_err(|_| "credential record is not valid JSON".to_owned())?;
                validate_record(&record)?;
                Ok(Some(record))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn write_record(&self, record: &CredentialRecord) -> Result<(), String> {
        validate_record(record)?;
        let contents = serde_json::to_vec(record)
            .map_err(|_| "credential record cannot be serialized".to_owned())?;
        let mut file = AtomicWriteFile::open(&self.path).map_err(store_error)?;
        file.write_all(&contents).map_err(store_error)?;
        file.sync_all().map_err(store_error)?;
        file.commit().map_err(store_error)
    }
}

impl CredentialStore for FileCredentialStore {
    fn load(&self) -> Result<Option<CredentialRecord>, String> {
        self.with_lock(|| self.read_record())
    }

    fn save_initial(&self, credential: &str) -> Result<CredentialRecord, String> {
        validate_credential(credential, "current")?;
        self.with_lock(|| match self.read_record()? {
            Some(record) => Ok(record),
            None => {
                let record = CredentialRecord {
                    current: credential.into(),
                    pending: None,
                };
                self.write_record(&record)?;
                Ok(record)
            }
        })
    }

    fn begin_rotation(&self, credential: &str) -> Result<(), String> {
        validate_credential(credential, "pending")?;
        self.with_lock(|| {
            let mut record = self
                .read_record()?
                .ok_or_else(|| "credential record does not exist".to_owned())?;
            if record.pending.is_some() {
                return Err("credential record already has a pending credential".into());
            }
            record.pending = Some(credential.into());
            self.write_record(&record)
        })
    }

    fn confirm_pending(&self) -> Result<(), String> {
        self.with_lock(|| {
            let mut record = self
                .read_record()?
                .ok_or_else(|| "credential record does not exist".to_owned())?;
            let pending = record
                .pending
                .take()
                .ok_or_else(|| "credential record has no pending credential".to_owned())?;
            record.current = pending;
            self.write_record(&record)
        })
    }

    fn complete_rotation(&self, expected_credential: &str) -> Result<(), String> {
        validate_credential(expected_credential, "expected")?;
        self.with_lock(|| {
            let mut record = self
                .read_record()?
                .ok_or_else(|| "credential record does not exist".to_owned())?;
            if record
                .pending
                .as_deref()
                .is_some_and(|pending| pending != expected_credential)
            {
                return Err("credential record pending credential does not match rotation".into());
            }
            record.current = expected_credential.into();
            record.pending = None;
            self.write_record(&record)
        })
    }

    fn discard_pending(&self) -> Result<(), String> {
        self.with_lock(|| {
            let mut record = self
                .read_record()?
                .ok_or_else(|| "credential record does not exist".to_owned())?;
            if record.pending.take().is_some() {
                self.write_record(&record)?;
            }
            Ok(())
        })
    }

    fn delete(&self) -> Result<(), String> {
        self.with_lock(|| match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(store_error(error)),
        })
    }
}

#[derive(Clone, Default)]
pub struct MemoryCredentialStore(Arc<Mutex<Option<CredentialRecord>>>);

impl MemoryCredentialStore {
    pub fn with_record(current: &str, pending: Option<&str>) -> Self {
        Self(Arc::new(Mutex::new(Some(CredentialRecord {
            current: current.into(),
            pending: pending.map(str::to_owned),
        }))))
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn load(&self) -> Result<Option<CredentialRecord>, String> {
        self.0
            .lock()
            .map(|record| record.clone())
            .map_err(|_| "in-memory credential store lock is poisoned".into())
    }

    fn save_initial(&self, credential: &str) -> Result<CredentialRecord, String> {
        validate_credential(credential, "current")?;
        let mut record = self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())?;
        Ok(record
            .get_or_insert_with(|| CredentialRecord {
                current: credential.into(),
                pending: None,
            })
            .clone())
    }

    fn begin_rotation(&self, credential: &str) -> Result<(), String> {
        validate_credential(credential, "pending")?;
        let mut record = self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())?;
        let record = record
            .as_mut()
            .ok_or_else(|| "credential record does not exist".to_owned())?;
        if record.pending.is_some() {
            return Err("credential record already has a pending credential".into());
        }
        record.pending = Some(credential.into());
        Ok(())
    }

    fn confirm_pending(&self) -> Result<(), String> {
        let mut record = self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())?;
        let record = record
            .as_mut()
            .ok_or_else(|| "credential record does not exist".to_owned())?;
        record.current = record
            .pending
            .take()
            .ok_or_else(|| "credential record has no pending credential".to_owned())?;
        Ok(())
    }

    fn complete_rotation(&self, expected_credential: &str) -> Result<(), String> {
        validate_credential(expected_credential, "expected")?;
        let mut record = self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())?;
        let record = record
            .as_mut()
            .ok_or_else(|| "credential record does not exist".to_owned())?;
        if record
            .pending
            .as_deref()
            .is_some_and(|pending| pending != expected_credential)
        {
            return Err("credential record pending credential does not match rotation".into());
        }
        record.current = expected_credential.into();
        record.pending = None;
        Ok(())
    }

    fn discard_pending(&self) -> Result<(), String> {
        let mut record = self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())?;
        if let Some(record) = record.as_mut() {
            record.pending = None;
        }
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())? = None;
        Ok(())
    }
}

fn validate_record(record: &CredentialRecord) -> Result<(), String> {
    validate_credential(&record.current, "current")?;
    if let Some(pending) = &record.pending {
        validate_credential(pending, "pending")?;
    }
    Ok(())
}

fn validate_credential(value: &str, field: &str) -> Result<(), String> {
    (!value.trim().is_empty())
        .then_some(())
        .ok_or_else(|| format!("credential record {field} credential is empty"))
}

fn store_error(error: std::io::Error) -> String {
    format!("credential file operation failed: {error}")
}
