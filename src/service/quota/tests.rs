use super::cache_key;

#[test]
fn cache_key_changes_with_the_api_key() {
    assert_ne!(
        cache_key(
            "provider",
            "https://api.example.test",
            "https://example.test",
            "key-a"
        ),
        cache_key(
            "provider",
            "https://api.example.test",
            "https://example.test",
            "key-b"
        )
    );
    assert_eq!(
        cache_key(
            "provider",
            "https://api.example.test",
            "https://example.test",
            "key-a"
        ),
        cache_key(
            "provider",
            "https://api.example.test",
            "https://example.test",
            "key-a"
        )
    );
}
