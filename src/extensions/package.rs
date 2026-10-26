use prelay_protocol::{
    CatalogKind, ExtensionArtifactSummary, ExtensionKind, ExtensionVersion, HOOKS_MANIFEST_PATH,
};
use semver::Version;

pub const RULES_PATH: &str = "AGENTS.md";
pub const MCP_MANIFEST_PATH: &str = "server.json";
pub const SKILLS_PREFIX: &str = "skills/";
pub const HOOKS_PREFIX: &str = "hooks/";

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

fn is_skill_manifest_path(path: &str) -> bool {
    path.starts_with(SKILLS_PREFIX) && path.ends_with("/SKILL.md")
}

fn is_hook_file_path(path: &str) -> bool {
    path.starts_with(HOOKS_PREFIX) && valid_extension_file_path(path)
}
