use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::{header::AUTHORIZATION, Client};
use serde::Deserialize;

use super::{config::ExtensionCatalogConfig, package::GiteaTag};

#[derive(Clone)]
pub(super) struct GiteaClient {
    client: Client,
    config: ExtensionCatalogConfig,
}

impl GiteaClient {
    pub(super) fn new(client: Client, config: ExtensionCatalogConfig) -> Self {
        Self { client, config }
    }

    pub(super) async fn repositories(&self) -> Result<Vec<String>> {
        let mut repositories = Vec::new();
        let mut page = 1;
        loop {
            let page_text = page.to_string();
            let current: Vec<GiteaRepository> = self
                .get_json(
                    &format!("orgs/{}/repos", self.config.organization()),
                    &[("limit", "50"), ("page", &page_text)],
                )
                .await?;
            let count = current.len();
            repositories.extend(current.into_iter().map(|repository| repository.name));
            if count < 50 {
                return Ok(repositories);
            }
            page += 1;
        }
    }

    pub(super) async fn versions(&self, repository: &str) -> Result<Vec<GiteaTag>> {
        let tags: Vec<GiteaTagResponse> = self
            .get_json(
                &format!("repos/{}/{repository}/tags", self.config.organization()),
                &[("limit", "100"), ("page", "1")],
            )
            .await?;
        Ok(tags
            .into_iter()
            .map(|tag| {
                let (commit_sha, updated_at) = match tag.commit {
                    Some(commit) => (Some(commit.sha), commit.created),
                    None => (None, None),
                };
                GiteaTag::new(tag.name, tag.id, commit_sha, updated_at)
            })
            .collect())
    }

    pub(super) async fn tree_paths(
        &self,
        repository: &str,
        commit_sha: &str,
    ) -> Result<Vec<String>> {
        let tree: GiteaTree = self
            .get_json(
                &format!(
                    "repos/{}/{repository}/git/trees/{commit_sha}",
                    self.config.organization()
                ),
                &[("recursive", "true")],
            )
            .await?;
        Ok(tree
            .tree
            .into_iter()
            .filter(|entry| entry.kind == "blob")
            .map(|entry| entry.path)
            .collect())
    }

    pub(super) async fn read_file(
        &self,
        repository: &str,
        commit_sha: &str,
        path: &str,
    ) -> Result<String> {
        let content: GiteaContent = self
            .get_json(
                &format!(
                    "repos/{}/{repository}/contents/{path}",
                    self.config.organization()
                ),
                &[("ref", commit_sha)],
            )
            .await?;
        if content.encoding != "base64" {
            anyhow::bail!("Gitea returned an unsupported extension file encoding");
        }
        let bytes = BASE64
            .decode(content.content.replace(['\r', '\n'], ""))
            .context("decode Gitea extension file")?;
        String::from_utf8(bytes).context("Gitea extension file is not UTF-8")
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}/api/v1/{path}", self.config.gitea_url());
        let mut request = self
            .client
            .get(url)
            .query(query)
            .header("user-agent", "prelay-server");
        if let Some(token) = self.config.read_token() {
            request = request.header(AUTHORIZATION, format!("token {token}"));
        }
        request
            .send()
            .await
            .context("request Gitea extension catalog")?
            .error_for_status()
            .context("Gitea extension catalog request failed")?
            .json()
            .await
            .context("decode Gitea extension catalog response")
    }
}

#[derive(Deserialize)]
struct GiteaRepository {
    name: String,
}

#[derive(Deserialize)]
struct GiteaTagResponse {
    name: String,
    id: String,
    commit: Option<GiteaCommit>,
}

#[derive(Deserialize)]
struct GiteaCommit {
    sha: String,
    created: Option<String>,
}

#[derive(Deserialize)]
struct GiteaTree {
    tree: Vec<GiteaTreeEntry>,
}

#[derive(Deserialize)]
struct GiteaTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct GiteaContent {
    content: String,
    encoding: String,
}
