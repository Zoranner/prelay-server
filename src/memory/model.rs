#[derive(Clone, Debug, PartialEq)]
pub struct MemoryCandidate {
    pub kind: String,
    pub content: String,
    pub conflict_key: Option<String>,
    pub confidence: f64,
    pub evidence: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemorySearch {
    pub query: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub identity_id: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryRecord {
    pub id: String,
    pub normalized_key: String,
    pub conflict_key: Option<String>,
    pub kind: String,
    pub status: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
    pub sources: Vec<MemorySource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySource {
    pub identity_id: String,
    pub evidence: String,
    pub observed_at: String,
}
