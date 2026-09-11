use bytes::Bytes;
use serde_json::json;

/// Wrap replacement text as a complete Responses API SSE stream.
pub fn wrap_replacement_as_sse(text: &str) -> Bytes {
    let created = json!({
        "type": "response.created",
        "sequence_number": 0,
        "response": {
            "id": "resp_tamper", "object": "response",
            "status": "in_progress", "output": []
        }
    });
    let delta = json!({
        "type": "response.output_text.delta", "sequence_number": 1,
        "item_id": "msg_tamper", "output_index": 0, "content_index": 0,
        "delta": text
    });
    let done = json!({
        "type": "response.output_text.done", "sequence_number": 2,
        "item_id": "msg_tamper", "output_index": 0, "content_index": 0,
        "text": text
    });
    let completed = json!({
        "type": "response.completed",
        "sequence_number": 3,
        "response": {
            "id": "resp_tamper", "object": "response", "status": "completed",
            "output": [{
                "id": "msg_tamper", "type": "message", "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text", "annotations": [], "logprobs": [], "text": text
                }]
            }],
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens": 0,
                "output_tokens_details": { "reasoning_tokens": 0 },
                "total_tokens": 0
            }
        }
    });

    Bytes::from(format!(
        "event: response.created\ndata: {created}\n\nevent: response.output_text.delta\ndata: {delta}\n\nevent: response.output_text.done\ndata: {done}\n\nevent: response.completed\ndata: {completed}\n\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(body: &Bytes) -> Vec<serde_json::Value> {
        std::str::from_utf8(body)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str(data).unwrap())
            .collect()
    }

    #[test]
    fn replacement_stream_has_complete_usage_and_sequence_numbers() {
        let parsed = events(&wrap_replacement_as_sse("replacement"));
        assert_eq!(parsed.len(), 4);
        for (index, event) in parsed.iter().enumerate() {
            assert_eq!(event["sequence_number"], index as u64);
        }

        let usage = &parsed[3]["response"]["usage"];
        assert_eq!(usage["input_tokens"], 0);
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 0);
        assert_eq!(usage["output_tokens"], 0);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 0);
        assert_eq!(usage["total_tokens"], 0);
    }

    #[test]
    fn replacement_text_is_json_escaped_and_preserved() {
        let parsed = events(&wrap_replacement_as_sse("quoted \"text\"\nnext line"));
        assert_eq!(
            parsed[3]["response"]["output"][0]["content"][0]["text"],
            "quoted \"text\"\nnext line"
        );
    }
}
