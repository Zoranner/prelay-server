mod model;
mod normalization;

pub use model::{MemoryCandidate, MemoryRecord, MemorySearch, MemorySource};
pub(crate) use normalization::normalize_candidate;
