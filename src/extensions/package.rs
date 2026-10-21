use prelay_protocol::{CatalogKind, ExtensionArtifactSummary, ExtensionKind, ExtensionVersion};
use semver::Version;
use serde::Deserialize;

pub const PRELAY_MANIFEST_PATH: &str = ".prelay.json";
pub const RULES_PATH: &str = "AGENTS.md";
pub const MCP_MANIFEST_PATH: &str = "server.json";
pub const SKILLS_PREFIX: &str = "skills/";
pub const HOOKS_MANIFEST_PATH: &str = "hooks/hooks.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedArtifact {
    pub kind: ExtensionKind,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedPackage {
    pub catalog_kind: CatalogKind,
    pub artifacts: Vec<ClassifiedArtifact>,
}

impl ClassifiedPackage {
    pub fn artifact_summaries(&self) -> Vec<ExtensionArtifactSummary> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.kind != ExtensionKind::Rule)
            .map(|artifact| ExtensionArtifactSummary {
                kind: artifact.kind,
                file_count: artifact.paths.len(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    InvalidJson,
    InvalidSchema,
    NoArtifacts,
    DuplicateArtifact,
    UnsupportedArtifact,
    InvalidPath,
    MissingPath,
    EmptyArtifact,
    ConflictingSelectors,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelayManifest {
    schema: u32,
    artifacts: Vec<PrelayManifestArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrelayManifestArtifact {
    kind: ExtensionKind,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GiteaTag {
    pub name: String,
    pub id: String,
    pub commit_sha: Option<String>,
    pub updated_at: Option<String>,
}

impl GiteaTag {
    pub fn new(
        name: impl Into<String>,
        id: impl Into<String>,
        commit_sha: Option<String>,
        updated_at: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            id: id.into(),
            commit_sha,
            updated_at,
        }
    }
}

pub fn parse_prelay_manifest(bytes: &[u8]) -> Result<PrelayManifest, ManifestError> {
    let manifest: PrelayManifest =
        serde_json::from_slice(bytes).map_err(|_| ManifestError::InvalidJson)?;
    if manifest.schema != 2 {
        return Err(ManifestError::InvalidSchema);
    }
    if manifest.artifacts.is_empty() {
        return Err(ManifestError::NoArtifacts);
    }
    Ok(manifest)
}

pub fn classify_paths(paths: &[&str]) -> Option<ClassifiedPackage> {
    let mut artifacts = Vec::new();
    if paths.contains(&MCP_MANIFEST_PATH) {
        artifacts.push(ClassifiedArtifact {
            kind: ExtensionKind::Mcp,
            paths: vec![MCP_MANIFEST_PATH.to_string()],
        });
    }
    if paths.iter().any(|path| is_skill_manifest_path(path)) {
        let skill_paths = paths
            .iter()
            .filter(|path| path.starts_with(SKILLS_PREFIX) && valid_extension_file_path(path))
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        if !skill_paths.is_empty() {
            artifacts.push(ClassifiedArtifact {
                kind: ExtensionKind::Skill,
                paths: skill_paths,
            });
        }
    }
    if paths.contains(&HOOKS_MANIFEST_PATH) {
        let hook_paths = paths
            .iter()
            .filter(|path| is_hook_file_path(path))
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        if !hook_paths.is_empty() {
            artifacts.push(ClassifiedArtifact {
                kind: ExtensionKind::Hook,
                paths: hook_paths,
            });
        }
    }

    if !artifacts.is_empty() {
        return Some(ClassifiedPackage {
            catalog_kind: CatalogKind::Plugin,
            artifacts,
        });
    }

    paths.contains(&RULES_PATH).then_some(ClassifiedPackage {
        catalog_kind: CatalogKind::Rule,
        artifacts: vec![ClassifiedArtifact {
            kind: ExtensionKind::Rule,
            paths: vec![RULES_PATH.to_string()],
        }],
    })
}

pub fn classify_manifest(
    manifest: &PrelayManifest,
    paths: &[&str],
) -> Result<ClassifiedPackage, ManifestError> {
    let mut artifacts = Vec::<ClassifiedArtifact>::with_capacity(manifest.artifacts.len());
    for artifact in &manifest.artifacts {
        if artifacts
            .iter()
            .any(|existing| existing.kind == artifact.kind)
        {
            return Err(ManifestError::DuplicateArtifact);
        }
        let paths = artifact_paths(artifact, paths)?;
        if paths.is_empty() {
            return Err(ManifestError::EmptyArtifact);
        }
        artifacts.push(ClassifiedArtifact {
            kind: artifact.kind,
            paths,
        });
    }
    if artifacts.is_empty() {
        return Err(ManifestError::NoArtifacts);
    }
    Ok(ClassifiedPackage {
        catalog_kind: CatalogKind::Plugin,
        artifacts,
    })
}

pub fn versions_from_tags(tags: Vec<GiteaTag>) -> Vec<ExtensionVersion> {
    let mut versions = tags
        .into_iter()
        .filter_map(|tag| {
            let version = tag.name.strip_prefix('v')?.parse::<Version>().ok()?;
            let commit_sha = tag.commit_sha.unwrap_or(tag.id);
            let updated_at = tag.updated_at?;
            valid_commit_sha(&commit_sha).then_some((
                version,
                ExtensionVersion {
                    tag: tag.name,
                    commit_sha,
                    updated_at,
                },
            ))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|(left, _), (right, _)| right.cmp(left));
    versions.into_iter().map(|(_, version)| version).collect()
}

pub fn valid_repository_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, b'-' | b'_'))
}

pub fn valid_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|character| character.is_ascii_hexdigit())
}

pub fn valid_extension_file_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.split('/').all(|component| {
            !component.is_empty() && !matches!(component, "." | "..") && !component.contains('\0')
        })
}

fn artifact_paths(
    artifact: &PrelayManifestArtifact,
    paths: &[&str],
) -> Result<Vec<String>, ManifestError> {
    if artifact.path.is_some() && !artifact.include.is_empty() {
        return Err(ManifestError::ConflictingSelectors);
    }
    match artifact.kind {
        ExtensionKind::Skill => skill_paths(artifact, paths),
        ExtensionKind::Mcp => {
            if !artifact.include.is_empty() {
                return Err(ManifestError::ConflictingSelectors);
            }
            let path = artifact.path.as_deref().unwrap_or(MCP_MANIFEST_PATH);
            require_file(paths, path)?;
            Ok(vec![path.to_string()])
        }
        ExtensionKind::Hook => {
            if !artifact.include.is_empty() {
                return Err(ManifestError::ConflictingSelectors);
            }
            let path = artifact.path.as_deref().unwrap_or(HOOKS_MANIFEST_PATH);
            if !is_hook_file_path(path) {
                return Err(ManifestError::InvalidPath);
            }
            require_file(paths, path)?;
            Ok(vec![path.to_string()])
        }
        ExtensionKind::Rule | ExtensionKind::Unknown => Err(ManifestError::UnsupportedArtifact),
    }
}

fn skill_paths(
    artifact: &PrelayManifestArtifact,
    paths: &[&str],
) -> Result<Vec<String>, ManifestError> {
    let selectors = match artifact.path.as_deref() {
        Some(path) => vec![path],
        None => artifact
            .include
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    };
    if selectors.is_empty() {
        return Err(ManifestError::ConflictingSelectors);
    }

    let mut selected = Vec::new();
    for selector in selectors {
        if !valid_extension_directory_path(selector) {
            return Err(ManifestError::InvalidPath);
        }
        let mut matched = false;
        for path in paths {
            if valid_extension_file_path(path) && path_matches_selector(path, selector) {
                matched = true;
                if !selected.iter().any(|existing| existing == path) {
                    selected.push((*path).to_string());
                }
            }
        }
        if !matched {
            return Err(ManifestError::MissingPath);
        }
    }
    if !selected.iter().any(|path| path.ends_with("/SKILL.md")) {
        return Err(ManifestError::EmptyArtifact);
    }
    Ok(selected)
}

fn require_file(paths: &[&str], path: &str) -> Result<(), ManifestError> {
    if !valid_extension_file_path(path) {
        return Err(ManifestError::InvalidPath);
    }
    paths
        .contains(&path)
        .then_some(())
        .ok_or(ManifestError::MissingPath)
}

fn path_matches_selector(path: &str, selector: &str) -> bool {
    if selector.ends_with('/') {
        return path.starts_with(selector);
    }
    path == selector
        || path
            .strip_prefix(selector)
            .is_some_and(|remainder| remainder.starts_with('/'))
}

fn valid_extension_directory_path(value: &str) -> bool {
    let trimmed = value.strip_suffix('/').unwrap_or(value);
    !trimmed.is_empty() && valid_extension_file_path(trimmed)
}

fn is_skill_manifest_path(path: &str) -> bool {
    path.starts_with(SKILLS_PREFIX) && path.ends_with("/SKILL.md")
}

fn is_hook_file_path(path: &str) -> bool {
    if !valid_extension_file_path(path) {
        return false;
    }
    let Some(file_name) = path.strip_prefix("hooks/") else {
        return false;
    };
    !file_name.is_empty() && !file_name.contains('/') && file_name.ends_with(".json")
}
