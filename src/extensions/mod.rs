mod catalog;
mod config;
mod gitea;
mod package;

pub use catalog::{CatalogError, ExtensionCatalog};
pub use config::ExtensionCatalogConfig;
pub use package::{classify_paths, versions_from_tags, GiteaTag};
