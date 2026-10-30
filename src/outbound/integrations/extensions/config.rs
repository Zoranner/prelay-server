use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Url;

const DEFAULT_GITEA_URL: &str = "https://git.kimo.ink";
const DEFAULT_GITEA_ORGANIZATION: &str = "agents";
const DEFAULT_CATALOG_CACHE_TTL_SECONDS: u64 = 300;

#[derive(Clone, Debug)]
pub struct ExtensionCatalogConfig {
    gitea_url: String,
    organization: String,
    read_token: Option<String>,
    cache_ttl: Duration,
}

impl ExtensionCatalogConfig {
    pub fn new(
        gitea_url: impl AsRef<str>,
        organization: impl Into<String>,
        read_token: Option<String>,
        cache_ttl_seconds: u64,
    ) -> Result<Self> {
        let mut url = Url::parse(gitea_url.as_ref())
            .context("EXTENSIONS_GITEA_URL must be an absolute HTTP or HTTPS URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("EXTENSIONS_GITEA_URL must be an absolute HTTP or HTTPS URL");
        }
        let path = url.path().trim_end_matches('/').to_string();
        url.set_path(&path);
        let gitea_url = url.to_string().trim_end_matches('/').to_string();

        let organization = organization.into();
        if !valid_name(&organization) {
            bail!("EXTENSIONS_GITEA_ORGANIZATION contains unsupported characters");
        }
        if cache_ttl_seconds == 0 {
            bail!("EXTENSIONS_CATALOG_CACHE_TTL_SECONDS must be greater than zero");
        }

        Ok(Self {
            gitea_url,
            organization,
            read_token: read_token.filter(|value| !value.trim().is_empty()),
            cache_ttl: Duration::from_secs(cache_ttl_seconds),
        })
    }

    pub fn from_environment() -> Result<Self> {
        let cache_ttl_seconds = std::env::var("EXTENSIONS_CATALOG_CACHE_TTL_SECONDS")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .context("EXTENSIONS_CATALOG_CACHE_TTL_SECONDS must be an integer")
            })
            .transpose()?
            .unwrap_or(DEFAULT_CATALOG_CACHE_TTL_SECONDS);
        Self::new(
            std::env::var("EXTENSIONS_GITEA_URL").unwrap_or_else(|_| DEFAULT_GITEA_URL.to_string()),
            std::env::var("EXTENSIONS_GITEA_ORGANIZATION")
                .unwrap_or_else(|_| DEFAULT_GITEA_ORGANIZATION.to_string()),
            std::env::var("EXTENSIONS_GITEA_READ_TOKEN").ok(),
            cache_ttl_seconds,
        )
    }

    pub fn gitea_url(&self) -> &str {
        &self.gitea_url
    }

    pub fn organization(&self) -> &str {
        &self.organization
    }

    pub fn read_token(&self) -> Option<&str> {
        self.read_token.as_deref()
    }

    pub fn cache_ttl(&self) -> Duration {
        self.cache_ttl
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, b'-' | b'_'))
}
