// M1: SystemPromptInjector — 保留 Codex 原始系统上下文，并追加 bridge 能力块。

use crate::core::{RequestCtx, RequestInterceptor};
use serde_json::Value;

const BRIDGE_START: &str = "<!-- super-instruct-bridge:start -->";
const BRIDGE_END: &str = "<!-- super-instruct-bridge:end -->";

pub struct SystemPromptInjector {
    instructions: String,
}

impl SystemPromptInjector {
    pub fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: instructions.into(),
        }
    }
}

impl RequestInterceptor for SystemPromptInjector {
    fn name(&self) -> &'static str {
        "inject"
    }

    fn intercept(&self, ctx: &mut RequestCtx) {
        tracing::debug!(category = %ctx.meta.category, "inject: merging system prompt bridge");
        let fields_found = inspect_request_fields(&ctx.body);
        tracing::debug!(category = %ctx.meta.category, fields = ?fields_found, "inject: request body field map");
        if !inject_system(&mut ctx.body, &self.instructions) {
            tracing::warn!(category = %ctx.meta.category, "inject: no system prompt field found");
        }
    }
}

fn bridge_block(instructions: &str) -> String {
    format!("{BRIDGE_START}\n{instructions}\n{BRIDGE_END}")
}

fn merge_text(existing: &str, instructions: &str) -> String {
    if existing.contains(BRIDGE_START) {
        return existing.to_string();
    }
    let bridge = bridge_block(instructions);
    if existing.trim().is_empty() {
        bridge
    } else {
        format!("{existing}\n\n{bridge}")
    }
}

fn merge_value(value: &mut Value, instructions: &str) {
    match value {
        Value::String(text) => *text = merge_text(text, instructions),
        Value::Array(items) => {
            if items.iter().any(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map_or(false, |text| text.contains(BRIDGE_START))
            }) {
                return;
            }
            items.push(serde_json::json!({
                "type": "input_text",
                "text": bridge_block(instructions),
            }));
        }
        _ => {
            let text = value.to_string();
            *value = Value::String(merge_text(&text, instructions));
        }
    }
}

/// 诊断: 列出请求 JSON 中存在的关键字段名。
fn inspect_request_fields(data: &Value) -> Vec<&'static str> {
    let Some(obj) = data.as_object() else {
        return Vec::new();
    };
    let known = [
        "instructions",
        "system",
        "system_prompt",
        "personality",
        "messages",
        "input",
        "model",
        "stream",
    ];
    known
        .into_iter()
        .filter(|field| obj.contains_key(*field))
        .collect()
}

/// 追加 bridge 到直接字段、messages[].role=system 和 input[].role=system。
/// 原始 Codex 系统内容始终保留；重复请求不会重复追加 marker 块。
pub fn inject_system(data: &mut Value, instructions: &str) -> bool {
    let Some(obj) = data.as_object_mut() else {
        return false;
    };
    let mut injected = false;

    for field in ["instructions", "system", "system_prompt", "personality"] {
        if let Some(value) = obj.get_mut(field) {
            merge_value(value, instructions);
            injected = true;
        }
    }

    if let Some(messages) = obj.get_mut("messages").and_then(Value::as_array_mut) {
        let mut found = false;
        for msg in messages.iter_mut() {
            if msg.get("role").and_then(Value::as_str) == Some("system") {
                if let Some(content) = msg.get_mut("content") {
                    merge_value(content, instructions);
                } else {
                    msg["content"] = Value::String(bridge_block(instructions));
                }
                found = true;
                injected = true;
            }
        }
        if !found {
            messages.insert(
                0,
                serde_json::json!({"role": "system", "content": bridge_block(instructions)}),
            );
            injected = true;
        }
    }

    if let Some(input) = obj.get_mut("input").and_then(Value::as_array_mut) {
        let mut found = false;
        for item in input.iter_mut() {
            if item.get("role").and_then(Value::as_str) == Some("system") {
                if let Some(content) = item.get_mut("content") {
                    merge_value(content, instructions);
                } else {
                    item["content"] = serde_json::json!([{
                        "type": "input_text",
                        "text": bridge_block(instructions),
                    }]);
                }
                found = true;
                injected = true;
            }
        }
        if !found {
            input.insert(
                0,
                serde_json::json!({
                    "role": "system",
                    "content": [{"type": "input_text", "text": bridge_block(instructions)}],
                }),
            );
            injected = true;
        }
    }

    injected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_system_content_and_is_idempotent() {
        let mut body = serde_json::json!({"instructions": "Codex original"});
        assert!(inject_system(&mut body, "bridge rules"));
        let first = body["instructions"].as_str().unwrap().to_string();
        assert!(first.starts_with("Codex original"));
        assert!(first.contains(BRIDGE_START));
        assert!(inject_system(&mut body, "bridge rules"));
        assert_eq!(
            body["instructions"]
                .as_str()
                .unwrap()
                .matches(BRIDGE_START)
                .count(),
            1
        );
    }

    #[test]
    fn adds_missing_system_message_without_losing_user_messages() {
        let mut body = serde_json::json!({"messages": [{"role": "user", "content": "hello"}]});
        assert!(inject_system(&mut body, "bridge"));
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        assert_eq!(body["messages"][1]["content"], "hello");
    }
}
