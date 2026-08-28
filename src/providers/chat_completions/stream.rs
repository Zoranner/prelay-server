#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
pub fn decode_chat_sse_text_deltas(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|value| {
            value
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}
