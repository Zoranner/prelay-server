use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

const DEFAULT_LISTEN_ADDRESS: &str = "0.0.0.0:18080";
const DEFAULT_CATALOG_DIRECTORY: &str = "config/catalog";

pub(super) fn catalog_directory() -> PathBuf {
    PathBuf::from(DEFAULT_CATALOG_DIRECTORY)
}

pub(super) fn load_environment_file(path: &Path) -> anyhow::Result<()> {
    match dotenvy::from_path(path) {
        Ok(()) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn listen_address(value: Option<&str>) -> anyhow::Result<SocketAddr> {
    value
        .unwrap_or(DEFAULT_LISTEN_ADDRESS)
        .parse()
        .map_err(|error| anyhow::anyhow!("LISTEN_ADDRESS must be a valid socket address: {error}"))
}
