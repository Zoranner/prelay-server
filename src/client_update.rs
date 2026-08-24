use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use prelay_protocol::ClientUpdateTarget;
use serde::Deserialize;
use tokio::{io::AsyncWriteExt, sync::Mutex};
use uuid::Uuid;

const DEFAULT_REPOSITORY: &str = "Zoranner/prelay-client";
const DEFAULT_CACHE_DIRECTORY: &str = "updates";
const GITHUB_API_BASE_URL: &str = "https://api.github.com";

#[derive(Clone)]
pub struct ClientUpdateCache {
    cache_directory: PathBuf,
    client: reqwest::Client,
    repository: String,
    refresh_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug)]
pub struct CachedClientUpdate {
    pub version: String,
    file_name: String,
}

impl CachedClientUpdate {
    pub fn installer_path(
        &self,
        cache_directory: &Path,
        target: &ClientUpdateTarget,
    ) -> Option<PathBuf> {
        Some(
            cache_directory_for_target(cache_directory, target, &self.version)?
                .join(&self.file_name),
        )
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl ClientUpdateCache {
    pub async fn from_environment(client: reqwest::Client) -> Result<Self> {
        let repository = std::env::var("CLIENT_UPDATE_REPOSITORY")
            .unwrap_or_else(|_| DEFAULT_REPOSITORY.to_string());
        validate_repository(&repository)?;

        let cache_directory = std::env::var("CLIENT_UPDATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CACHE_DIRECTORY));
        Ok(Self::new(client, repository, cache_directory))
    }

    pub fn unavailable(client: reqwest::Client) -> Self {
        Self::new(client, DEFAULT_REPOSITORY.to_string(), PathBuf::new())
    }

    fn new(client: reqwest::Client, repository: String, cache_directory: PathBuf) -> Self {
        Self {
            cache_directory,
            client,
            repository,
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn latest(&self, target: &ClientUpdateTarget) -> Option<CachedClientUpdate> {
        self.load_cached_update(target).await
    }

    pub fn cache_directory(&self) -> PathBuf {
        self.cache_directory.clone()
    }

    pub async fn refresh(&self) -> Result<()> {
        let _refresh_guard = self.refresh_lock.lock().await;
        let release = self.fetch_latest_release().await?;
        let updates = release
            .assets
            .iter()
            .filter_map(|asset| windows_nsis_target(&asset.name).map(|target| (asset, target)))
            .collect::<Vec<_>>();
        if updates.is_empty() {
            bail!("latest GitHub release does not contain a Windows NSIS installer");
        }
        let version = normalize_version(&release.tag_name)?;
        for (asset, target) in updates {
            let update = CachedClientUpdate {
                file_name: asset.name.clone(),
                version: version.clone(),
            };
            self.download_asset(asset, &update, &target).await?;
        }
        Ok(())
    }

    async fn fetch_latest_release(&self) -> Result<GithubRelease> {
        self.client
            .get(format!(
                "{GITHUB_API_BASE_URL}/repos/{}/releases/latest",
                self.repository
            ))
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, "prelay-server")
            .send()
            .await
            .context("request latest client release")?
            .error_for_status()
            .context("latest client release request failed")?
            .json()
            .await
            .context("decode latest client release")
    }

    async fn download_asset(
        &self,
        asset: &GithubReleaseAsset,
        update: &CachedClientUpdate,
        target: &ClientUpdateTarget,
    ) -> Result<()> {
        let response = self
            .client
            .get(&asset.browser_download_url)
            .header(reqwest::header::USER_AGENT, "prelay-server")
            .send()
            .await
            .context("download client installer")?
            .error_for_status()
            .context("client installer download failed")?;
        let directory = cache_directory_for_target(&self.cache_directory, target, &update.version)
            .context("invalid client update target")?;
        tokio::fs::create_dir_all(&directory)
            .await
            .context("create client update cache directory")?;
        let installer_path = directory.join(&update.file_name);
        let temporary = directory.join(format!(".{}.{}.tmp", update.file_name, Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temporary)
            .await
            .context("create temporary client installer")?;
        let mut stream = response.bytes_stream();
        let write_result = async {
            while let Some(chunk) = stream.next().await {
                file.write_all(&chunk.context("read client installer download")?)
                    .await
                    .context("write client installer")?;
            }
            file.flush().await.context("flush client installer")
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }

        if tokio::fs::try_exists(&installer_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&installer_path)
                .await
                .context("replace cached client installer")?;
        }
        tokio::fs::rename(&temporary, installer_path)
            .await
            .context("publish cached client installer")
    }

    async fn load_cached_update(&self, target: &ClientUpdateTarget) -> Option<CachedClientUpdate> {
        self.find_installer_in_cache_directory(target).await
    }

    async fn find_installer_in_cache_directory(
        &self,
        target: &ClientUpdateTarget,
    ) -> Option<CachedClientUpdate> {
        let directory = cache_directory_for_target(&self.cache_directory, target, "")?;
        let mut entries = tokio::fs::read_dir(directory).await.ok()?;
        let mut latest = None;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if !entry.file_type().await.ok()?.is_dir() {
                continue;
            }
            let version = entry.file_name().to_string_lossy().to_string();
            if parse_version(&version).is_none() {
                continue;
            }
            let Some(update) = find_installer_in_version_directory(entry.path(), version).await
            else {
                continue;
            };
            latest = select_newer_update(latest, Some(update));
        }
        latest
    }
}

fn validate_repository(repository: &str) -> Result<()> {
    let mut components = repository.split('/');
    let owner = components.next().unwrap_or_default();
    let name = components.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || components.next().is_some()
        || repository.chars().any(char::is_whitespace)
    {
        bail!("CLIENT_UPDATE_REPOSITORY must use the owner/repository format");
    }
    Ok(())
}

fn normalize_version(tag_name: &str) -> Result<String> {
    let version = tag_name.strip_prefix('v').unwrap_or(tag_name);
    if version.is_empty()
        || !version.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
    {
        bail!("latest client release has an invalid version tag");
    }
    Ok(version.to_string())
}

fn is_windows_nsis_asset(name: &str) -> bool {
    is_safe_file_name(name) && name.to_ascii_lowercase().ends_with("-setup.exe")
}

fn windows_nsis_target(name: &str) -> Option<ClientUpdateTarget> {
    if !is_windows_nsis_asset(name) {
        return None;
    }
    let file_name = name.strip_suffix("-setup.exe")?;
    let (_, architecture) = file_name.rsplit_once('_')?;
    is_safe_path_component(architecture).then(|| ClientUpdateTarget {
        platform: "windows".to_string(),
        architecture: architecture.to_string(),
    })
}

fn cache_directory_for_target(
    cache_directory: &Path,
    target: &ClientUpdateTarget,
    version: &str,
) -> Option<PathBuf> {
    if !is_safe_path_component(&target.platform)
        || !is_safe_path_component(&target.architecture)
        || (!version.is_empty() && parse_version(version).is_none())
    {
        return None;
    }
    let directory = cache_directory
        .join(&target.platform)
        .join(&target.architecture);
    (!version.is_empty())
        .then(|| directory.join(version))
        .or(Some(directory))
}

fn is_safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn is_safe_file_name(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .file_name()
            .is_some_and(|file_name| file_name == value)
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
}

async fn find_installer_in_version_directory(
    directory: PathBuf,
    version: String,
) -> Option<CachedClientUpdate> {
    let mut entries = tokio::fs::read_dir(directory).await.ok()?;
    let mut file_name = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !entry.file_type().await.ok()?.is_file() {
            continue;
        }
        let candidate = entry.file_name().to_string_lossy().to_string();
        if !is_safe_file_name(&candidate) {
            continue;
        }
        if file_name.replace(candidate).is_some() {
            return None;
        }
    }
    Some(CachedClientUpdate {
        version,
        file_name: file_name?,
    })
}

fn select_newer_update(
    first: Option<CachedClientUpdate>,
    second: Option<CachedClientUpdate>,
) -> Option<CachedClientUpdate> {
    match (first, second) {
        (Some(first), Some(second)) if is_newer_version(&second.version, &first.version) => {
            Some(second)
        }
        (Some(first), _) => Some(first),
        (None, second) => second,
    }
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    matches!(
        (parse_version(candidate), parse_version(current)),
        (Some(candidate), Some(current)) if candidate > current
    )
}

fn parse_version(value: &str) -> Option<[u64; 3]> {
    let parts = value.split('.').collect::<Vec<_>>();
    (parts.len() == 3).then_some(())?;
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        cache_directory_for_target, is_windows_nsis_asset, normalize_version, validate_repository,
        windows_nsis_target, DEFAULT_CACHE_DIRECTORY,
    };
    use prelay_protocol::ClientUpdateTarget;
    use std::path::Path;

    #[test]
    fn stores_client_updates_outside_the_database_directory_by_default() {
        assert_eq!(DEFAULT_CACHE_DIRECTORY, "updates");
    }

    #[test]
    fn selects_tauri_windows_nsis_installer() {
        assert!(is_windows_nsis_asset("Prelay_0.2.0_x64-setup.exe"));
        assert!(!is_windows_nsis_asset("Prelay_0.2.0_x64_en-US.msi"));
        assert!(!is_windows_nsis_asset("Prelay_0.2.0_x64.AppImage"));
    }

    #[test]
    fn validates_repository_and_version_values_before_using_them_as_paths() {
        assert!(validate_repository("Zoranner/prelay-client").is_ok());
        assert!(validate_repository("https://github.com/Zoranner/prelay-client").is_err());
        assert_eq!(normalize_version("v0.2.0").unwrap(), "0.2.0");
        assert!(normalize_version("../../installer").is_err());
    }

    #[test]
    fn selects_original_installers_from_the_requested_target_directory() {
        let target = ClientUpdateTarget {
            platform: "windows".to_string(),
            architecture: "x64".to_string(),
        };
        assert_eq!(
            cache_directory_for_target(Path::new("updates"), &target, "0.3.0").unwrap(),
            Path::new("updates/windows/x64/0.3.0")
        );
        assert_eq!(
            windows_nsis_target("Prelay_0.3.0_arm64-setup.exe")
                .unwrap()
                .architecture,
            "arm64"
        );
    }
}
