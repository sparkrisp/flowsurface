use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::data_path;

const LOG_FILE: &str = "flowsurface-current.log";

pub fn file() -> Result<fs::File, Error> {
    let path = path()?;

    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(false)
        .truncate(true)
        .open(path)?)
}

pub fn path() -> Result<PathBuf, Error> {
    if let Ok(path) = std::env::var("FLOWSURFACE_LOG_PATH") {
        return ensure_parent(PathBuf::from(path));
    }

    if let Ok(dir) = std::env::var("FLOWSURFACE_LOG_DIR") {
        return ensure_parent(PathBuf::from(dir).join(LOG_FILE));
    }

    let full_path = data_path(Some(LOG_FILE));
    ensure_parent(full_path)
}

fn ensure_parent(full_path: PathBuf) -> Result<PathBuf, Error> {
    let parent = full_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }

    Ok(full_path)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    SetLog(#[from] log::SetLoggerError),
    #[error(transparent)]
    ParseLevel(#[from] log::ParseLevelError),
}
