use prelay_protocol::{ExtensionKind, ExtensionVersion};
use semver::Version;

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

pub fn classify_paths(paths: &[&str]) -> Option<ExtensionKind> {
    if paths.iter().any(|path| {
        *path == ".codex-plugin/plugin.json"
            || (path.starts_with(".opencode/plugins/")
                && (path.ends_with(".js") || path.ends_with(".ts")))
    }) {
        return Some(ExtensionKind::Plugin);
    }
    if paths.contains(&"server.json") {
        return Some(ExtensionKind::Mcp);
    }
    if paths
        .iter()
        .any(|path| path.starts_with("skills/") && path.ends_with("/SKILL.md"))
    {
        return Some(ExtensionKind::Skill);
    }
    paths.contains(&"AGENTS.md").then_some(ExtensionKind::Rule)
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
