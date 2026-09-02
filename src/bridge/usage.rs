use serde_json::Value;

use super::internal::InternalUsage;

pub fn decode_usage(value: Option<&Value>) -> Option<InternalUsage> {
    let usage = value?;
    Some(InternalUsage {
        input_tokens: usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_i64),
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_i64),
        reasoning_tokens: usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .or_else(|| usage.pointer("/completion_tokens_details/reasoning_tokens"))
            .and_then(Value::as_i64),
        cache_read_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_i64),
        cache_write_tokens: usage
            .pointer("/input_tokens_details/cache_write_tokens")
            .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
            .or_else(|| usage.get("cache_creation_input_tokens"))
            .and_then(Value::as_i64),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_usage;

    #[test]
    fn decodes_nested_responses_cache_write_tokens() {
        let usage = decode_usage(Some(&json!({
            "input_tokens": 10,
            "output_tokens": 4,
            "input_tokens_details": {
                "cached_tokens": 3,
                "cache_write_tokens": 2
            }
        })))
        .expect("usage");

        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(4));
        assert_eq!(usage.cache_read_tokens, Some(3));
        assert_eq!(usage.cache_write_tokens, Some(2));
    }
}
