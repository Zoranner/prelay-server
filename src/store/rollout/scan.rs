use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use super::RolloutStore;
use crate::domain::activity::rollout::RolloutError;

impl RolloutStore {
    pub fn incomplete_rollout_files(&self) -> Result<Vec<PathBuf>, RolloutError> {
        let mut files = Vec::new();
        collect_incomplete_rollout_files(&self.root, &mut files)?;
        files.sort();
        Ok(files)
    }

    pub fn rollout_files(&self) -> Result<Vec<String>, RolloutError> {
        let mut files = Vec::new();
        collect_rollout_files(&self.root, &self.root, &mut files)?;
        Ok(files)
    }
}

fn collect_incomplete_rollout_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), RolloutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_incomplete_rollout_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            && !has_complete_tail(&path)?
        {
            files.push(path);
        }
    }
    Ok(())
}

fn has_complete_tail(path: &Path) -> Result<bool, io::Error> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(length - 1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

fn collect_rollout_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<(), RolloutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_rollout_files(root, &path, files)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| RolloutError::InvalidPath(path.display().to_string()))?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
