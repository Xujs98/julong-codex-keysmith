//! Rust port of the pinned upstream activation-gate.js, wired into the shared HTTP pipeline.
//! This is a local feature switch, not a statement about a remote model's capabilities.
use crate::{environments, extensions::inject::inject_system_for_protocol};
use http::HeaderMap;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
pub const WORD: &str = "矩龙";
pub const REPLY: &str = "把每一次交互，变成可控能力";
const TTL: Duration = Duration::from_secs(30 * 60);
struct Session {
    last: Instant,
}
pub struct ActivationGate {
    packs: BTreeMap<String, String>,
    sessions: BTreeMap<String, Session>,
}
impl ActivationGate {
    pub fn new(packs: BTreeMap<String, String>) -> Self {
        Self {
            packs,
            sessions: BTreeMap::new(),
        }
    }
    pub fn deployed() -> Result<Self, String> {
        Ok(Self::new(environments::gate_packs(
            &environments::deployed_settings()?,
        )?))
    }
    fn prune(&mut self) {
        self.sessions.retain(|_, s| s.last.elapsed() < TTL);
    }
    pub fn reset(&mut self) {
        self.sessions.clear();
    }
    fn identity(&self, headers: &HeaderMap, body: &Value, path: &str) -> Option<(String, String)> {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(path)
            .to_ascii_lowercase();
        let seat = headers
            .get("x-julong-environment")
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if model.contains("claude")
                    || (crate::claude::messages_path(path)
                        && headers.contains_key("x-claude-code-session-id"))
                {
                    "claude"
                } else if model.contains("grok") {
                    "grok"
                } else if model.contains("deepseek") {
                    "deepseek"
                } else if model.contains("glm") {
                    "glm53"
                } else if model.contains("gemini") {
                    "gemini"
                } else if model.contains("gpt") || model.contains("codex") {
                    "codex"
                } else {
                    "unknown"
                }
                .into()
            });
        let session = [
            "x-julong-session",
            // Native Codex uses hyphenated names. Prefer the conversation ID:
            // session-id can represent cache affinity shared by several threads.
            "thread-id",
            "session-id",
            "session_id",
            "x-session-id",
            "x-codex-session-id",
            "x-claude-code-session-id",
        ]
        .iter()
        .find_map(|key| headers.get(*key).and_then(|h| h.to_str().ok()))
        .or_else(|| body.pointer("/metadata/session_id").and_then(Value::as_str))
        .or_else(|| body.pointer("/metadata/user_id").and_then(Value::as_str))?;
        if session.is_empty() || session.len() > 1024 {
            return None;
        }
        let credential = headers
            .get("authorization")
            .or_else(|| headers.get("x-api-key"))
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        let mut digest = Sha256::new();
        digest.update(credential);
        digest.update([0]);
        digest.update(session);
        digest.update([0]);
        digest.update(&seat);
        Some((seat, format!("{:x}", digest.finalize())))
    }
    pub fn local_response(
        &mut self,
        headers: &HeaderMap,
        body: &Value,
        path: &str,
    ) -> Option<axum::response::Response> {
        if path == "/julong/activation"
            && activation_text(headers, body, path)
                .as_deref()
                .map(str::trim)
                != Some(WORD)
        {
            self.prune();
            let active = self
                .identity(headers, body, path)
                .map(|(_, key)| self.sessions.contains_key(&key))
                .unwrap_or(false);
            return Some(wire_reply(
                path,
                body,
                if active {
                    "当前会话已启用。"
                } else {
                    "待机中。单独发送“矩龙”后启用。"
                },
            ));
        }
        if !(supported_path(path) || path == "/julong/activation")
            || activation_text(headers, body, path)
                .as_deref()
                .map(str::trim)
                != Some(WORD)
        {
            return None;
        }
        self.prune();
        let reply = match self.identity(headers, body, path) {
            Some((seat, key)) if self.packs.contains_key(&seat) => {
                if self.sessions.len() >= 100 && !self.sessions.contains_key(&key) {
                    if let Some(old) = self
                        .sessions
                        .iter()
                        .min_by_key(|(_, s)| s.last)
                        .map(|(k, _)| k.clone())
                    {
                        self.sessions.remove(&old);
                    }
                }
                self.sessions.insert(
                    key,
                    Session {
                        last: Instant::now(),
                    },
                );
                REPLY
            }
            Some(_) => "当前环境未部署或未选择模型指令，请先在配置管理完成部署。",
            None => "缺少会话标识。请使用矩龙会话验收，或让客户端发送 x-julong-session 请求头。",
        };
        Some(wire_reply(path, body, reply))
    }
    pub fn inject(&mut self, headers: &HeaderMap, body: &mut Value, path: &str) -> bool {
        self.prune();
        let Some((seat, key)) = self.identity(headers, body, path) else {
            return false;
        };
        let Some(session) = self.sessions.get_mut(&key) else {
            return false;
        };
        session.last = Instant::now();
        let Some(pack) = self.packs.get(&seat) else {
            return false;
        };
        inject_system_for_protocol(
            body,
            pack,
            path.split('?')
                .next()
                .unwrap_or(path)
                .ends_with("/messages"),
        )
    }
}
pub fn supported_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p.ends_with("/responses")
        || p.ends_with("/chat/completions")
        || p.ends_with("/messages")
        || p.contains(":generateContent")
        || p.contains(":streamGenerateContent")
}
fn text_parts(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.into());
    }
    let parts = value.as_array()?;
    let mut text = String::new();
    for part in parts {
        if part
            .get("type")
            .and_then(Value::as_str)
            .map(|t| t != "text" && t != "input_text")
            .unwrap_or(false)
        {
            return None;
        }
        // A file, image or tool block disqualifies the entire message from being an exact trigger.
        if part
            .as_object()?
            .keys()
            .any(|k| !["type", "text", "cache_control"].contains(&k.as_str()))
        {
            return None;
        }
        text.push_str(part.get("text")?.as_str()?);
    }
    Some(text)
}
pub fn latest_text(body: &Value) -> Option<String> {
    if let Some(s) = body.get("input").and_then(Value::as_str) {
        return Some(s.into());
    }
    let arr = body
        .get("input")
        .and_then(Value::as_array)
        .or_else(|| body.get("messages").and_then(Value::as_array))
        .or_else(|| body.get("contents").and_then(Value::as_array))?;
    let last = arr.last()?;
    if last.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    text_parts(last.get("content").or_else(|| last.get("parts"))?)
}

fn activation_text(headers: &HeaderMap, body: &Value, path: &str) -> Option<String> {
    // Claude Code 2.1 adds separate environment/date reminders ahead of the
    // actual user block, even for a one-word prompt. Do not strip inline tags,
    // attachments, tool blocks or reminders from arbitrary client protocols.
    if path.split('?').next()?.ends_with("/messages")
        && headers.contains_key("x-claude-code-session-id")
    {
        // Native Code's non-Claude model adapter can append system context after
        // the user message. Skip only system roles, never assistant/tool history.
        let last = body
            .get("messages")?
            .as_array()?
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(Value::as_str) != Some("system"))?;
        if last.get("role")?.as_str()? != "user" {
            return None;
        }
        if let Some(parts) = last.get("content")?.as_array() {
            if parts.len() > 1
                && parts[..parts.len() - 1].iter().all(|part| {
                    text_parts(&json!([part])).is_some_and(|text| {
                        let text = text.trim();
                        text.starts_with("<system-reminder>")
                            && text.ends_with("</system-reminder>")
                            && text.matches("</system-reminder>").count() == 1
                    })
                })
            {
                return text_parts(&json!([parts.last()?]));
            }
        }
        return text_parts(last.get("content")?);
    }
    latest_text(body)
}
fn wire_reply(path: &str, body: &Value, text: &str) -> axum::response::Response {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("julong-local");
    let id = format!("msg_julong_{}", uuid::Uuid::new_v4().simple());
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false)
        || path.contains(":streamGenerateContent");
    let mut events = Vec::<(Option<&str>, Value)>::new();
    let data = if path.contains("/messages") {
        let msg = json!({"id":id,"type":"message","role":"assistant","model":model,"content":[{"type":"text","text":text}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}});
        if stream {
            events.push((Some("message_start"),json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"usage":{"input_tokens":0,"output_tokens":0}}})));
            events.push((Some("content_block_start"),json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})));
            events.push((Some("content_block_delta"),json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})));
            events.push((
                Some("content_block_stop"),
                json!({"type":"content_block_stop","index":0}),
            ));
            events.push((Some("message_delta"),json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":0}})));
            events.push((Some("message_stop"), json!({"type":"message_stop"})));
        }
        msg
    } else if path.contains("/responses") {
        let rid = format!("resp_{}", uuid::Uuid::new_v4().simple());
        let item = json!({"id":id,"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[],"logprobs":[]}]});
        let response = json!({"id":rid,"object":"response","created_at":chrono::Utc::now().timestamp(),"model":model,"status":"completed","output":[item],"output_text":text,"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}});
        if stream {
            let mut initial = response.clone();
            initial["status"] = json!("in_progress");
            initial["output"] = json!([]);
            events.push((
                Some("response.created"),
                json!({"type":"response.created","response":initial}),
            ));
            let mut added = item.clone();
            added["status"] = json!("in_progress");
            added["content"] = json!([]);
            events.push((
                Some("response.output_item.added"),
                json!({"type":"response.output_item.added","output_index":0,"item":added}),
            ));
            events.push((Some("response.content_part.added"),json!({"type":"response.content_part.added","item_id":id,"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[],"logprobs":[]}})));
            events.push((Some("response.output_text.delta"),json!({"type":"response.output_text.delta","item_id":id,"output_index":0,"content_index":0,"delta":text})));
            events.push((Some("response.output_text.done"),json!({"type":"response.output_text.done","item_id":id,"output_index":0,"content_index":0,"text":text})));
            events.push((Some("response.content_part.done"),json!({"type":"response.content_part.done","item_id":id,"output_index":0,"content_index":0,"part":item["content"][0]})));
            events.push((
                Some("response.output_item.done"),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
            ));
            events.push((
                Some("response.completed"),
                json!({"type":"response.completed","response":response}),
            ));
            for (i, (_, v)) in events.iter_mut().enumerate() {
                v["sequence_number"] = json!(i);
            }
        }
        response
    } else if path.contains("/models/") {
        let value = json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":text}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":0,"candidatesTokenCount":0,"totalTokenCount":0}});
        if stream {
            events.push((None, value.clone()));
        }
        value
    } else {
        let value = json!({"id":id,"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}});
        if stream {
            events.push((None,json!({"id":id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]})));
            events.push((None,json!({"id":id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})));
        }
        value
    };
    let payload = if stream {
        let mut s = String::new();
        for (event, value) in events {
            if let Some(e) = event {
                s.push_str(&format!("event: {e}\n"));
            }
            s.push_str(&format!("data: {value}\n\n"));
        }
        if path.contains("/chat/completions") {
            s.push_str("data: [DONE]\n\n");
        }
        s
    } else {
        data.to_string()
    };
    axum::response::Response::builder()
        .status(200)
        .header(
            "content-type",
            if stream {
                "text/event-stream; charset=utf-8"
            } else {
                "application/json"
            },
        )
        .header(
            "x-julong-activation",
            if text == REPLY { "active" } else { "inactive" },
        )
        .header("cache-control", "no-store")
        .body(axum::body::Body::from(payload))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapped_native_model_trailing_system_preserves_exact_trigger_boundary() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "mapped-native-session".parse().unwrap(),
        );
        let trailing = json!({"role":"system","content":[{"type":"text","text":"# Environment"}]});
        let mut gate = ActivationGate::new(BTreeMap::from([("claude".into(), "PACK".into())]));
        let body = json!({"model":"gpt-6-astra[1M]","messages":[{"role":"user","content":"矩龙"},trailing]});
        assert_eq!(
            activation_text(&headers, &body, "/v1/messages").as_deref(),
            Some(WORD)
        );
        assert_eq!(
            gate.identity(&headers, &body, "/v1/messages").unwrap().0,
            "claude"
        );
        assert!(gate
            .local_response(&headers, &body, "/v1/messages")
            .is_some());
        assert!(activation_text(&HeaderMap::new(), &body, "/v1/messages").is_none());
        for last in [
            json!({"role":"assistant","content":"矩龙"}),
            json!({"role":"tool","content":"矩龙"}),
            json!({"role":"user","content":[{"type":"image","source":{}},{"type":"text","text":"矩龙"}]}),
            json!({"role":"user","content":"历史示例：矩龙"}),
        ] {
            let body = json!({"model":"gpt-6-astra","messages":[{"role":"user","content":"矩龙"},last,trailing]});
            assert!(gate
                .local_response(&headers, &body, "/v1/messages")
                .is_none());
        }
    }
    #[test]
    fn claude_native_reminders_do_not_hide_exact_current_user_word() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "native-session".parse().unwrap(),
        );
        let reminder =
            json!({"type":"text","text":"<system-reminder>\nEnvironment\n</system-reminder>"});
        let word = json!({"type":"text","text":"矩龙","cache_control":{"type":"ephemeral"}});
        let mut body = json!({"model":"claude-sonnet-4-6","messages":[{"role":"user","content":[reminder,word]}]});
        assert_eq!(
            activation_text(&headers, &body, "/v1/messages?beta=true").as_deref(),
            Some(WORD)
        );
        assert_ne!(
            activation_text(&HeaderMap::new(), &body, "/v1/messages").as_deref(),
            Some(WORD)
        );
        let mut gate = ActivationGate::new(BTreeMap::from([("claude".into(), "PACK".into())]));
        assert!(gate
            .local_response(&headers, &body, "/v1/messages")
            .is_some());
        body["messages"][0]["content"] = json!([{"type":"text","text":"next turn"}]);
        assert!(gate.inject(&headers, &mut body, "/v1/messages"));
        headers.insert("x-claude-code-session-id", "other-session".parse().unwrap());
        assert!(!gate.inject(&headers, &mut body, "/v1/messages"));
        for content in [
            json!([{"type":"text","text":"<system-reminder>quoted</system-reminder>矩龙"}]),
            json!([{"type":"text","text":"Quoted document"},word]),
            json!([{"type":"image","source":{}},word]),
            json!([{"type":"tool_result","content":"矩龙"},word]),
            json!([reminder,{"type":"text","text":"Example: 矩龙"}]),
        ] {
            body["messages"][0]["content"] = content;
            assert!(gate
                .local_response(&headers, &body, "/v1/messages")
                .is_none());
        }
    }
    #[test]
    fn expiration_capacity_reset_and_credentials_are_isolated() {
        let mut gate = ActivationGate::new(BTreeMap::from([("codex".into(), "pack".into())]));
        let body = json!({"model":"gpt-6-astra","input":"矩龙"});
        let mut headers = HeaderMap::new();
        headers.insert("session_id", "same".parse().unwrap());
        gate.local_response(&headers, &body, "/v1/responses")
            .unwrap();
        assert!(gate.inject(
            &headers,
            &mut json!({"model":"gpt-6-astra","input":"hello"}),
            "/v1/responses"
        ));
        headers.insert("authorization", "Bearer different".parse().unwrap());
        assert!(!gate.inject(&headers, &mut body.clone(), "/v1/responses"));
        headers.remove("authorization");
        for session in gate.sessions.values_mut() {
            session.last = Instant::now() - TTL - Duration::from_secs(1);
        }
        assert!(!gate.inject(&headers, &mut body.clone(), "/v1/responses"));
        for i in 0..101 {
            headers.insert("session_id", format!("s-{i}").parse().unwrap());
            gate.local_response(&headers, &body, "/v1/responses");
        }
        assert_eq!(gate.sessions.len(), 100);
        gate.reset();
        assert!(gate.sessions.is_empty());
    }
}
