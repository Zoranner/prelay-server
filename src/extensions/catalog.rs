use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prelay_protocol::{
    CatalogKind, ExtensionArtifact, ExtensionFile, ExtensionInstallBundle, ExtensionKind,
    ExtensionMcpManifest, ExtensionSummary, ExtensionVersion, HOOKS_MANIFEST_PATH,
};
use tokio::sync::{Mutex, RwLock};

use super::{
    config::ExtensionCatalogConfig,
    gitea::{GiteaClient, GiteaFileError},
    mcp::validate_mcp_manifest,
    package::{
        classify_paths, valid_extension_file_path, valid_repository_name, versions_from_tags,
        ClassifiedPackage,
    },
};

const README_PATH: &str = "README.md";

#[derive(Clone)]
pub struct ExtensionCatalog {
    config: ExtensionCatalogConfig,
    gitea: GiteaClient,
    cache: Arc<RwLock<Option<CachedCatalog>>>,
    files: Arc<RwLock<HashMap<FileCacheKey, Vec<u8>>>>,
    refresh_lock: Arc<Mutex<()>>,
    refreshing: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogError {
    Unavailable,
    ContentInvalid,
    ExtensionNotFound,
    VersionNotFound,
}

#[derive(Clone)]
struct CatalogEntry {
    summary: ExtensionSummary,
    classification: ClassifiedPackage,
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

    pub async fn list(
        &self,
        catalog_kind: CatalogKind,
    ) -> Result<Vec<ExtensionSummary>, CatalogError> {
        Ok(self
            .entries()
            .await?
            .into_iter()
            .filter(|entry| entry.summary.catalog_kind == Some(catalog_kind))
            .map(|entry| entry.summary)
            .collect())
    }

    pub async fn list_by_artifact(
        &self,
        kind: ExtensionKind,
    ) -> Result<Vec<ExtensionSummary>, CatalogError> {
        Ok(self
            .entries()
            .await?
            .into_iter()
            .filter(|entry| {
                entry
                    .classification
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.kind == kind)
            })
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
        String::from_utf8(
            self.file(repository, &version.commit_sha, README_PATH)
                .await?,
        )
        .map_err(|_| CatalogError::ContentInvalid)
    }

    pub async fn install_bundle(
        &self,
        repository: &str,
        tag: &str,
    ) -> Result<ExtensionInstallBundle, CatalogError> {
        self.entry(repository).await?;
        let version = self.version(repository, tag).await?;
        let paths = self
            .gitea
            .tree_paths(repository, &version.commit_sha)
            .await
            .map_err(|_| CatalogError::Unavailable)?;
        let classification = self
            .classify_version(&paths)
            .await?
            .ok_or(CatalogError::VersionNotFound)?;
        let mut artifacts = Vec::with_capacity(classification.artifacts.len());
        let mut included = Vec::new();
        for artifact in classification.artifacts {
            let mut files = Vec::with_capacity(artifact.paths.len());
            for path in artifact.paths {
                let content = self.file(repository, &version.commit_sha, &path).await?;
                if requires_utf8_content(artifact.kind, &path)
                    && std::str::from_utf8(&content).is_err()
                {
                    return Err(CatalogError::ContentInvalid);
                }
                if artifact.kind == ExtensionKind::Hook
                    && path == HOOKS_MANIFEST_PATH
                    && serde_json::from_slice::<serde_json::Value>(&content).is_err()
                {
                    return Err(CatalogError::ContentInvalid);
                }
                included.push(path.clone());
                files.push(ExtensionFile {
                    content_base64: BASE64.encode(content),
                    path,
                });
            }
            if artifact.kind == ExtensionKind::Mcp {
                let content = BASE64
                    .decode(
                        files
                            .first()
                            .ok_or(CatalogError::ContentInvalid)?
                            .content_base64
                            .as_str(),
                    )
                    .map_err(|_| CatalogError::ContentInvalid)?;
                let manifest = serde_json::from_slice::<ExtensionMcpManifest>(&content)
                    .map_err(|_| CatalogError::ContentInvalid)?;
                validate_mcp_manifest(&manifest)?;
            }
            artifacts.push(ExtensionArtifact {
                kind: artifact.kind,
                files,
            });
        }
        let skipped = paths
            .into_iter()
            .filter(|path| !included.contains(path))
            .collect();
        Ok(ExtensionInstallBundle {
            name: repository.to_string(),
            version,
            artifacts,
            skipped,
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
            let classification = match self.classify_version(&paths).await {
                Ok(Some(classification)) => classification,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(
                        repository = %repository,
                        ?error,
                        "skipping extension package with invalid content"
                    );
                    continue;
                }
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
                    catalog_kind: Some(classification.catalog_kind),
                    artifacts: classification.artifact_summaries(),
                },
                classification,
            });
        }
        entries.sort_by(|left, right| left.summary.repository.cmp(&right.summary.repository));
        Ok(entries)
    }

    async fn classify_version(
        &self,
        paths: &[String],
    ) -> Result<Option<ClassifiedPackage>, CatalogError> {
        let path_refs = paths.iter().map(String::as_str).collect::<Vec<_>>();
        Ok(classify_paths(&path_refs))
    }

    async fn file(
        &self,
        repository: &str,
        commit_sha: &str,
        path: &str,
    ) -> Result<Vec<u8>, CatalogError> {
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
        let content = match self.gitea.read_file(repository, commit_sha, path).await {
            Ok(content) => content,
            Err(GiteaFileError::Unavailable) => return Err(CatalogError::Unavailable),
            Err(GiteaFileError::ContentInvalid) => return Err(CatalogError::ContentInvalid),
        };
        self.files.write().await.insert(key, content.clone());
        Ok(content)
    }
}

fn requires_utf8_content(kind: ExtensionKind, path: &str) -> bool {
    match kind {
        ExtensionKind::Rule | ExtensionKind::Mcp => true,
        ExtensionKind::Hook => path == HOOKS_MANIFEST_PATH,
        ExtensionKind::Skill => path.ends_with("/SKILL.md"),
        ExtensionKind::Unknown => false,
    }
}
