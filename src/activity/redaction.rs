use std::sync::LazyLock;

use regex::Regex;

static BEARER_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bbearer\s+[^\s,;"']+"#).expect("valid bearer token redaction pattern")
});
static SECRET_ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        (?P<prefix>
            \b(?:authorization|api[_-]?key|endpoint[_-]?token|device[_-]?credential|
            client[_-]?secret|private[_-]?key|access[_-]?token|refresh[_-]?token|
            id[_-]?token|password|passphrase|secret|endpoint\s+token|device\s+credential|
            client\s+secret|private\s+key|access\s+token|refresh\s+token|id\s+token|
            credential|token)\b
            \s*(?::|=)\s*
        )
        (?:"[^"]*"|'[^']*'|[^\s,;]+)"#,
    )
    .expect("valid secret assignment redaction pattern")
});
static KNOWN_API_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(?:sk|pk|rk|pplx|xai)-[a-z0-9_-]{8,}\b
        |\bAIza[a-z0-9_-]{20,}\b
        |\bghp_[a-z0-9]{20,}\b
        |\bgithub_pat_[a-z0-9_]{20,}\b",
    )
    .expect("valid API key redaction pattern")
});
static JSON_WEB_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")
        .expect("valid JSON web token redaction pattern")
});
static PRIVATE_KEY_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)-----BEGIN(?: [A-Z0-9]+)* PRIVATE KEY-----.*?-----END(?: [A-Z0-9]+)* PRIVATE KEY-----",
    )
    .expect("valid private key block redaction pattern")
});
static PGP_PRIVATE_KEY_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*?-----END PGP PRIVATE KEY BLOCK-----")
        .expect("valid PGP private key block redaction pattern")
});

pub(super) fn redact_sensitive_text(value: &str) -> String {
    let value = PRIVATE_KEY_BLOCK.replace_all(value, "[REDACTED]");
    let value = PGP_PRIVATE_KEY_BLOCK.replace_all(&value, "[REDACTED]");
    let value = BEARER_TOKEN.replace_all(&value, "Bearer [REDACTED]");
    let value = SECRET_ASSIGNMENT
        .replace_all(&value, "${prefix}[REDACTED]")
        .into_owned();
    let value = KNOWN_API_KEY.replace_all(&value, "[REDACTED]");
    let value = JSON_WEB_TOKEN.replace_all(&value, "[REDACTED]");
    redact_url_safe_credentials(&value)
}

fn redact_url_safe_credentials(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let Some((offset, _)) = value[cursor..].char_indices().find(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }) else {
            redacted.push_str(&value[cursor..]);
            break;
        };
        let start = cursor + offset;
        redacted.push_str(&value[cursor..start]);
        let end = value[start..]
            .char_indices()
            .find(|(_, character)| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            })
            .map(|(offset, _)| start + offset)
            .unwrap_or(value.len());
        if end - start == 43 {
            redacted.push_str("[REDACTED]");
        } else {
            redacted.push_str(&value[start..end]);
        }
        cursor = end;
    }
    redacted
}
