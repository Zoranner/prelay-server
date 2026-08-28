use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::Result;
use prelay_protocol::{
    ExtensionFile, ExtensionInstallBundle, ExtensionKind, ExtensionSummary, ExtensionVersion,
};
use tokio::sync::{Mutex, RwLock};

use super::{
    config::ExtensionCatalogConfig,
    gitea::GiteaClient,
    package::{
        classify_paths, valid_extension_file_path, valid_repository_name, versions_from_tags,
    },
};

const README_PATH: &str = "README.md";
const RULES_PATH: &str = "AGENTS.md";
const SKILLS_PREFIX: &str = "skills/";

#[derive(Clone)]
pub struct ExtensionCatalog {
    config: ExtensionCatalogConfig,
    gitea: GiteaClient,
    cache: Arc<RwLock<Option<CachedCatalog>>>,
    files: Arc<RwLock<HashMap<FileCacheKey, String>>>,
    refresh_lock: Arc<Mutex<()>>,
    refreshing: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogError {
    Unavailable,
    ExtensionNotFound,
    VersionNotFound,
    InstallUnsupported,
}

#[derive(Clone)]
struct CatalogEntry {
    summary: ExtensionSummary,
    kind: ExtensionKind,
}

struct CachedCatalog {
    loaded_at: Instant,
    entries: Vec<CatalogEntry>,
}

#[derive(Hash, Eq, PartialEq)]
struct FileCacheKey {
    repository: String,
    commit_sha: String,
    path: String,
}

impl ExtensionCatalog {
    pub fn from_environment(client: reqwest::Client) -> Result<Self> {
        Ok(Self::new(
            client,
            ExtensionCatalogConfig::from_environment()?,
        ))
    }

    pub fn unavailable(client: reqwest::Client) -> Self {
        Self::new(
            client,
            ExtensionCatalogConfig::new("http://127.0.0.1:9", "agents", None, 300)
                .expect("default extension catalog configuration is valid"),
        )
    }

    pub fn new(client: reqwest::Client, config: ExtensionCatalogConfig) -> Self {
        Self {
            gitea: GiteaClient::new(client, config.clone()),
            config,
            cache: Arc::new(RwLock::new(None)),
            files: Arc::new(RwLock::new(HashMap::new())),
            refresh_lock: Arc::new(Mutex::new(())),
            refreshing: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn warm(&self) {
        self.refresh_in_background();
    }

    pub async fn list(&self, kind: ExtensionKind) -> Result<Vec<ExtensionSummary>, CatalogError> {
        Ok(self
            .entries()
            .await?
            .into_iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.summary)
            .collect())
    }

    pub async fn versions(&self, repository: &str) -> Result<Vec<ExtensionVersion>, CatalogError> {
        self.entry(repository).await?;
        let versions = versions_from_tags(
            self.gitea
                .versions(repository)
                .await
                .map_err(|_| CatalogError::Unavailable)?,
        );
        (!versions.is_empty())
            .then_some(versions)
            .ok_or(CatalogError::VersionNotFound)
    }

    pub async fn readme(&self, repository: &str, tag: &str) -> Result<String, CatalogError> {
        let version = self.version(repository, tag).await?;
        self.file(repository, &version.commit_sha, README_PATH)
            .await
    }

    pub async fn install_bundle(
        &self,
        repository: &str,
        tag: &str,
    ) -> Result<ExtensionInstallBundle, CatalogError> {
        let entry = self.entry(repository).await?;
        let version = self.version(repository, tag).await?;
        let paths = self
            .gitea
            .tree_paths(repository, &version.commit_sha)
            .await
            .map_err(|_| CatalogError::Unavailable)?;
        let install_paths = match entry.kind {
            ExtensionKind::Rule if paths.iter().any(|path| path == RULES_PATH) => {
                vec![RULES_PATH.to_string()]
            }
            ExtensionKind::Skill => paths
                .into_iter()
                .filter(|path| path.starts_with(SKILLS_PREFIX) && valid_extension_file_path(path))
                .collect(),
            ExtensionKind::Plugin | ExtensionKind::Mcp => {
                return Err(CatalogError::InstallUnsupported);
            }
            ExtensionKind::Rule => return Err(CatalogError::VersionNotFound),
        };
        if install_paths.is_empty() {
            return Err(CatalogError::VersionNotFound);
        }

        let mut files = Vec::with_capacity(install_paths.len());
        for path in install_paths {
            files.push(ExtensionFile {
                content: self.file(repository, &version.commit_sha, &path).await?,
                path,
            });
        }
        Ok(ExtensionInstallBundle {
            name: repository.to_string(),
            kind: entry.kind,
            version,
            files,
        })
    }

    async fn entry(&self, repository: &str) -> Result<CatalogEntry, CatalogError> {
        if !valid_repository_name(repository) {
            return Err(CatalogError::ExtensionNotFound);
        }
        self.entries()
            .await?
            .into_iter()
            .find(|entry| entry.summary.name == repository)
            .ok_or(CatalogError::ExtensionNotFound)
    }

    async fn version(&self, repository: &str, tag: &str) -> Result<ExtensionVersion, CatalogError> {
        self.versions(repository)
            .await?
            .into_iter()
            .find(|version| version.tag == tag)
            .ok_or(CatalogError::VersionNotFound)
    }

    async fn entries(&self) -> Result<Vec<CatalogEntry>, CatalogError> {
        if let Some(cached) = self.cache.read().await.as_ref() {
            if cached.loaded_at.elapsed() < self.config.cache_ttl() {
                return Ok(cached.entries.clone());
            }
            let entries = cached.entries.clone();
            self.refresh_in_background();
            return Ok(entries);
        }
        self.refresh().await
    }

    fn refresh_in_background(&self) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let catalog = self.clone();
        tokio::spawn(async move {
            if catalog.refresh().await.is_err() {
                tracing::warn!("failed to refresh extension catalog");
            }
            catalog.refreshing.store(false, Ordering::Release);
        });
    }

    async fn refresh(&self) -> Result<Vec<CatalogEntry>, CatalogError> {
        let _guard = self.refresh_lock.lock().await;
        let entries = self.scan().await.map_err(|_| CatalogError::Unavailable)?;
        *self.cache.write().await = Some(CachedCatalog {
            loaded_at: Instant::now(),
            entries: entries.clone(),
        });
        Ok(entries)
    }

    async fn scan(&self) -> Result<Vec<CatalogEntry>> {
        let mut entries = Vec::new();
        for repository in self.gitea.repositories().await? {
            if !valid_repository_name(&repository) {
                continue;
            }
            let Some(latest) = versions_from_tags(self.gitea.versions(&repository).await?)
                .into_iter()
                .next()
            else {
                continue;
            };
            let paths = self
                .gitea
                .tree_paths(&repository, &latest.commit_sha)
                .await?;
            let path_refs = paths.iter().map(String::as_str).collect::<Vec<_>>();
            let Some(kind) = classify_paths(&path_refs) else {
                continue;
            };
            entries.push(CatalogEntry {
                summary: ExtensionSummary {
                    name: repository.clone(),
                    repository: format!(
                        "{}/{}/{}",
                        self.config.gitea_url(),
                        self.config.organization(),
                        repository
                    ),
                    latest,
                },
                kind,
            });
        }
        entries.sort_by(|left, right| left.summary.repository.cmp(&right.summary.repository));
        Ok(entries)
    }

    async fn file(
        &self,
        repository: &str,
        commit_sha: &str,
        path: &str,
    ) -> Result<String, CatalogError> {
        if !valid_extension_file_path(path) {
            return Err(CatalogError::VersionNotFound);
        }
        let key = FileCacheKey {
            repository: repository.to_string(),
            commit_sha: commit_sha.to_string(),
            path: path.to_string(),
        };
        if let Some(content) = self.files.read().await.get(&key) {
            return Ok(content.clone());
        }
        let content = self
            .gitea
            .read_file(repository, commit_sha, path)
            .await
            .map_err(|_| CatalogError::Unavailable)?;
        self.files.write().await.insert(key, content.clone());
        Ok(content)
    }
}
