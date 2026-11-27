mod catalog;
mod config;
mod gitea;
mod mcp;
mod package;

pub use catalog::{CatalogError, ExtensionCatalog};
pub use config::ExtensionCatalogConfig;
pub use package::{
    classify_paths, policy_from_topics, versions_from_tags, ClassifiedArtifact, ClassifiedPackage,
    GiteaTag, TopicPolicy,
};
