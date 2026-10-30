use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::RolloutStore;
use crate::activity::rollout::RolloutError;

impl RolloutStore {
    pub fn part_files(&self) -> Result<Vec<PathBuf>, RolloutError> {
        let mut files = Vec::new();
        collect_part_files(&self.root, &mut files)?;
        Ok(files)
    }

    pub fn rollout_files(&self) -> Result<Vec<String>, RolloutError> {
        let mut files = Vec::new();
        collect_rollout_files(&self.root, &self.root, &mut files)?;
        Ok(files)
    }
}

fn collect_part_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), RolloutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_part_files(&entry.path(), files)?;
        } else if file_type.is_file() && entry.path().extension().is_some_and(|ext| ext == "part") {
            files.push(entry.path());
        }
    }
    Ok(())
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
