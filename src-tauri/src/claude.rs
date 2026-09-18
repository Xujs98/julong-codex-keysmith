//! Claude Code's user settings adapter. No executable installation or shell edits.
use serde_json::{json, Value};

pub const LOCAL_TOKEN: &str = "julong-local-proxy";

pub fn render_settings(original: Option<&[u8]>, base_url: &str) -> Result<Vec<u8>, String> {
    render_provider_settings(original, base_url, None)
}

pub fn render_provider_settings(
    original: Option<&[u8]>,
    base_url: &str,
    provider: Option<&crate::providers::Provider>,
) -> Result<Vec<u8>, String> {
    let mut value: Value = match original {
        None => json!({}),
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|e| format!("Claude settings.json 不是有效 JSON，未覆盖原文件: {e}"))?,
    };
    let object = value
        .as_object_mut()
        .ok_or("Claude settings.json 必须是 JSON 对象")?;
    let env = object
        .entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("Claude settings.json 的 env 必须是 JSON 对象")?;
    for key in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        if env
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|v| v == "1" || v == "true")
        {
            return Err(format!(
                "Claude 当前启用了 {key}，请先改为 Anthropic Messages 接口再部署"
            ));
        }
    }
    let existing_headers = match env.get("ANTHROPIC_CUSTOM_HEADERS") {
        Some(Value::String(s)) => s.as_str(),
        None => "",
        _ => return Err("Claude ANTHROPIC_CUSTOM_HEADERS 必须是字符串".into()),
    };
    let mut headers: Vec<_> = existing_headers
        .lines()
        .filter(|line| {
            !line
                .split_once(':')
                .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("x-julong-environment"))
        })
        .filter(|line| !line.trim().is_empty())
        .collect();
    headers.push("x-julong-environment: claude");
    let headers = headers.join("\n");
    env.insert("ANTHROPIC_CUSTOM_HEADERS".into(), json!(headers));
    env.insert("ANTHROPIC_BASE_URL".into(), json!(base_url));
    // The actual relay key stays in the proxy, so switching providers also works
    // for already-running Claude processes. Keep any user's original auth/model.
    if !["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .iter()
        .any(|key| {
            env.get(*key)
                .and_then(Value::as_str)
                .is_some_and(|v| !v.trim().is_empty())
        })
    {
        env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(LOCAL_TOKEN));
    }
    if let Some(provider) = provider {
        if provider.api_key.trim().is_empty() {
            return Err("Claude 供应商 API Key 不能为空".into());
        }
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".into(),
            json!(provider.api_key.trim()),
        );
        env.remove("ANTHROPIC_API_KEY");
        let mapping = crate::claude_models::effective(provider);
        for role in crate::claude_models::ROLES {
            let key = crate::claude_models::env_key(role);
            let name_key = format!("{key}_NAME");
            env.remove(key);
            env.remove(&name_key);
            let row = mapping.row(role);
            if !row.model.trim().is_empty() {
                env.insert(key.into(), json!(row.client_model()));
                if role != "subagent" && !row.display_name.trim().is_empty() {
                    env.insert(name_key, json!(row.display_name.trim()));
                }
            }
        }
        env.remove("ANTHROPIC_SMALL_FAST_MODEL");
        env.remove("ANTHROPIC_MODEL");
        if !provider.default_model.trim().is_empty() {
            let fallback = crate::claude_models::ModelSlot {
                model: provider.default_model.clone(),
                supports_1m: mapping.default_1m,
                ..Default::default()
            }
            .client_model();
            env.insert("ANTHROPIC_MODEL".into(), json!(fallback));
            object.insert("model".into(), json!(fallback));
        } else {
            object.remove("model");
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn messages_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path.ends_with("/messages") || path.ends_with("/messages/count_tokens")
}

/// Code may emit separate system-role messages for non-Claude model IDs.
/// Anthropic Messages requires these blocks in the top-level system field.
pub fn normalize_native_system_messages(body: &mut Value) -> Result<(), String> {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    if !messages.iter().any(|m| m["role"] == "system") {
        return Ok(());
    }
    let mut system = Vec::new();
    let mut append = |content: &Value| -> Result<(), String> {
        match content {
            Value::String(text) => system.push(json!({"type":"text","text":text})),
            Value::Array(blocks)
                if blocks
                    .iter()
                    .all(|b| b["type"] == "text" && b["text"].is_string()) =>
            {
                system.extend(blocks.iter().cloned())
            }
            _ => return Err("Claude system 消息必须为文本或文本块".into()),
        }
        Ok(())
    };
    if let Some(original) = body.get("system") {
        append(original)?;
    }
    let mut retained = Vec::new();
    for message in messages {
        if message["role"] == "system" {
            append(&message["content"])?;
        } else {
            retained.push(message.clone());
        }
    }
    body["system"] = json!(system);
    body["messages"] = json!(retained);
    Ok(())
}
