mod catalog;
mod config;
mod gitea;
mod mcp;
mod package;

pub use catalog::{CatalogError, ExtensionCatalog};
pub use config::ExtensionCatalogConfig;
pub use package::{
    classify_manifest, classify_paths, parse_prelay_manifest, versions_from_tags,
    ClassifiedArtifact, ClassifiedPackage, GiteaTag, ManifestError, PrelayManifest,
};
