use std::sync::OnceLock;

use super::DEFAULT_ACTIVITY_CONTENT_MAX_BYTES;

static ACTIVITY_CONTENT_POLICY: OnceLock<ActivityContentPolicy> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityContentPolicy {
    pub max_bytes: usize,
}

impl Default for ActivityContentPolicy {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_ACTIVITY_CONTENT_MAX_BYTES,
        }
    }
}

impl ActivityContentPolicy {
    pub fn from_environment() -> Result<Self, String> {
        Self::from_values(std::env::var("ACTIVITY_CONTENT_MAX_BYTES").ok().as_deref())
    }

    pub fn from_values(max_bytes: Option<&str>) -> Result<Self, String> {
        let max_bytes = match max_bytes {
            Some(value) => value
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    "ACTIVITY_CONTENT_MAX_BYTES must be a positive integer".to_string()
                })?,
            None => DEFAULT_ACTIVITY_CONTENT_MAX_BYTES,
        };
        Ok(Self { max_bytes })
    }
}

pub fn initialize_from_environment() -> Result<&'static ActivityContentPolicy, String> {
    let configured_policy = ActivityContentPolicy::from_environment()?;
    ACTIVITY_CONTENT_POLICY
        .set(configured_policy)
        .map_err(|_| "activity content policy was already initialized".to_owned())?;
    Ok(policy())
}

pub fn policy() -> &'static ActivityContentPolicy {
    ACTIVITY_CONTENT_POLICY.get_or_init(ActivityContentPolicy::default)
}
