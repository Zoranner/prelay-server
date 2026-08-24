use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, RwLock},
};
use uuid::Uuid;

const DEFAULT_REPOSITORY: &str = "Zoranner/prelay-client";
const DEFAULT_CACHE_DIRECTORY: &str = "data/client-updates";
const GITHUB_API_BASE_URL: &str = "https://api.github.com";
const MANIFEST_FILE_NAME: &str = "client-update.json";

#[derive(Clone)]
pub struct ClientUpdateCache {
    cache_directory: PathBuf,
    client: reqwest::Client,
    repository: String,
    latest: Arc<RwLock<Option<CachedClientUpdate>>>,
    refresh_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CachedClientUpdate {
    pub version: String,
    file_name: String,
}

impl CachedClientUpdate {
    pub fn installer_path(&self, cache_directory: &Path) -> PathBuf {
        cache_directory.join(&self.file_name)
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
        let cache = Self::new(client, repository, cache_directory);
        cache.load_manifest().await;
        Ok(cache)
    }

    pub fn unavailable(client: reqwest::Client) -> Self {
        Self::new(client, DEFAULT_REPOSITORY.to_string(), PathBuf::new())
    }

    fn new(client: reqwest::Client, repository: String, cache_directory: PathBuf) -> Self {
        Self {
            cache_directory,
            client,
            repository,
            latest: Arc::new(RwLock::new(None)),
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn latest(&self) -> Option<CachedClientUpdate> {
        self.latest.read().await.clone()
    }

    pub fn cache_directory(&self) -> PathBuf {
        self.cache_directory.clone()
    }

    pub async fn refresh(&self) -> Result<()> {
        let _refresh_guard = self.refresh_lock.lock().await;
        let release = self.fetch_latest_release().await?;
        let asset = release
            .assets
            .iter()
            .find(|asset| is_windows_nsis_asset(&asset.name))
            .context("latest GitHub release does not contain a Windows NSIS installer")?;
        let version = normalize_version(&release.tag_name)?;
        let update = CachedClientUpdate {
            file_name: format!("prelay-client-{version}.exe"),
            version,
        };

        tokio::fs::create_dir_all(&self.cache_directory)
            .await
            .context("create client update cache directory")?;
        self.download_asset(asset, &update).await?;
        self.write_manifest(&update).await?;
        *self.latest.write().await = Some(update);
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
        let target = update.installer_path(&self.cache_directory);
        let temporary =
            self.cache_directory
                .join(format!(".{}.{}.tmp", update.file_name, Uuid::new_v4()));
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

        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            tokio::fs::remove_file(&target)
                .await
                .context("replace cached client installer")?;
        }
        tokio::fs::rename(&temporary, target)
            .await
            .context("publish cached client installer")
    }

    async fn load_manifest(&self) {
        let path = self.manifest_path();
        let Some(update) = tokio::fs::read(&path)
            .await
            .ok()
            .and_then(|content| serde_json::from_slice::<CachedClientUpdate>(&content).ok())
        else {
            return;
        };
        if tokio::fs::try_exists(update.installer_path(&self.cache_directory))
            .await
            .unwrap_or(false)
        {
            *self.latest.write().await = Some(update);
        }
    }

    async fn write_manifest(&self, update: &CachedClientUpdate) -> Result<()> {
        let temporary = self
            .cache_directory
            .join(format!(".{MANIFEST_FILE_NAME}.{}.tmp", Uuid::new_v4()));
        let manifest = serde_json::to_vec(update).context("serialize client update manifest")?;
        tokio::fs::write(&temporary, manifest)
            .await
            .context("write client update manifest")?;
        let path = self.manifest_path();
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            tokio::fs::remove_file(&path)
                .await
                .context("replace client update manifest")?;
        }
        tokio::fs::rename(temporary, path)
            .await
            .context("publish client update manifest")
    }

    fn manifest_path(&self) -> PathBuf {
        self.cache_directory.join(MANIFEST_FILE_NAME)
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
    name.to_ascii_lowercase().ends_with("-setup.exe")
}

#[cfg(test)]
mod tests {
    use super::{is_windows_nsis_asset, normalize_version, validate_repository};

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
}
