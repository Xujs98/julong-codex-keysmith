//! Claude Code's user settings adapter. No executable installation or shell edits.
use serde_json::{json, Value};

pub const LOCAL_TOKEN: &str = "julong-local-proxy";

pub fn render_settings(original: Option<&[u8]>, base_url: &str) -> Result<Vec<u8>, String> {
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
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn messages_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path.ends_with("/messages") || path.ends_with("/messages/count_tokens")
}
