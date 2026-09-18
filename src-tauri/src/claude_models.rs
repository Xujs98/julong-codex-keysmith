//! Shared Claude role mapping for Code settings, Desktop catalog and upstream requests.
use crate::providers::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ROLES: [&str; 5] = ["sonnet", "opus", "fable", "haiku", "subagent"];
pub const ROUTES: [(&str, &str); 4] = [
    ("sonnet", "claude-sonnet-4-6"),
    ("opus", "claude-opus-4-6"),
    ("fable", "claude-fable-5"),
    ("haiku", "claude-haiku-4-5"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSlot {
    pub model: String,
    pub display_name: String,
    pub supports_1m: bool,
}

impl ModelSlot {
    pub fn client_model(&self) -> String {
        let model = strip_marker(&self.model);
        if !model.is_empty() && (self.supports_1m || has_marker(&self.model)) {
            format!("{model}[1M]")
        } else {
            model.into()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelMapping {
    pub sonnet: ModelSlot,
    pub opus: ModelSlot,
    pub fable: ModelSlot,
    pub haiku: ModelSlot,
    pub subagent: ModelSlot,
    pub default_1m: bool,
}

impl ModelMapping {
    pub fn row(&self, role: &str) -> &ModelSlot {
        match role {
            "sonnet" => &self.sonnet,
            "opus" => &self.opus,
            "fable" => &self.fable,
            "haiku" => &self.haiku,
            "subagent" => &self.subagent,
            _ => unreachable!("unknown internal Claude role"),
        }
    }
}

pub fn env_key(role: &str) -> &'static str {
    match role {
        "sonnet" => "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "opus" => "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "fable" => "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "haiku" => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "subagent" => "CLAUDE_CODE_SUBAGENT_MODEL",
        _ => unreachable!("unknown internal Claude role"),
    }
}

pub fn has_marker(model: &str) -> bool {
    model.trim().as_bytes().ends_with(b"[1M]") || model.trim().as_bytes().ends_with(b"[1m]")
}

pub fn strip_marker(model: &str) -> &str {
    let model = model.trim();
    if has_marker(model) {
        model[..model.len() - 4].trim_end()
    } else {
        model
    }
}

/// Missing mappings are old provider records: infer roles once without restricting brand.
pub fn effective(provider: &Provider) -> ModelMapping {
    if let Some(mapping) = &provider.claude_models {
        return mapping.clone();
    }
    let fallback = std::iter::once(&provider.default_model)
        .chain(provider.models.iter())
        .find(|m| !strip_marker(m).is_empty())
        .map(String::as_str)
        .unwrap_or("");
    let slot = |role: &str| {
        let model = provider
            .models
            .iter()
            .find(|m| strip_marker(m).contains(&format!("claude-{role}-")))
            .map(String::as_str)
            .unwrap_or(fallback);
        ModelSlot {
            model: strip_marker(model).into(),
            supports_1m: has_marker(model),
            display_name: String::new(),
        }
    };
    ModelMapping {
        sonnet: slot("sonnet"),
        opus: slot("opus"),
        fable: slot("fable"),
        haiku: slot("haiku"),
        subagent: ModelSlot::default(),
        default_1m: has_marker(&provider.default_model),
    }
}

fn resolved(provider: &Provider, mapping: &ModelMapping, role: &str) -> ModelSlot {
    let row = mapping.row(role);
    if !strip_marker(&row.model).is_empty() {
        return row.clone();
    }
    if role == "fable" && !strip_marker(&mapping.opus.model).is_empty() {
        return mapping.opus.clone();
    }
    ModelSlot {
        model: provider.default_model.clone(),
        display_name: row.display_name.clone(),
        supports_1m: mapping.default_1m,
    }
}

pub fn desktop_models(provider: &Provider) -> Vec<Value> {
    let mapping = effective(provider);
    ROUTES.iter().filter_map(|(role, route)| {
        let row = resolved(provider, &mapping, role);
        let model = strip_marker(&row.model);
        if model.is_empty() { return None; }
        let label = if row.display_name.trim().is_empty() { model } else { row.display_name.trim() };
        Some(json!({"name":route, "labelOverride":label, "supports1m":row.supports_1m || has_marker(&row.model)}))
    }).collect()
}

pub fn catalog(provider: &Provider) -> Value {
    let data: Vec<_> = desktop_models(provider)
        .iter()
        .map(|m| {
            json!({
                "id":m["name"], "type":"model", "display_name":m["labelOverride"],
                "created_at":"2024-01-01T00:00:00Z", "supports1m":m["supports1m"]
            })
        })
        .collect();
    json!({"first_id":data.first().map(|m| &m["id"]), "last_id":data.last().map(|m| &m["id"]),
        "has_more":false, "data":data})
}

pub fn upstream_models(provider: &Provider) -> Vec<String> {
    let mapping = effective(provider);
    let mut models = Vec::new();
    for model in std::iter::once(provider.default_model.as_str())
        .chain(ROLES.iter().map(|r| mapping.row(r).model.as_str()))
    {
        let model = strip_marker(model);
        if !model.is_empty() && !models.iter().any(|m| m == model) {
            models.push(model.to_string());
        }
    }
    models
}

/// Apply exactly once, after client-side mapping and before both Messages endpoints.
pub fn map_body(body: &mut Value, provider: &Provider, desktop: bool) {
    let Some(requested) = body.get("model").and_then(Value::as_str) else {
        return;
    };
    let requested = strip_marker(requested);
    let mapping = effective(provider);
    // Code may already have substituted a role or subagent target. Never remap it to fallback.
    let desktop_route = desktop
        .then(|| ROUTES.iter().find(|(_, id)| *id == requested))
        .flatten();
    let target = if let Some((role, _)) = desktop_route {
        let row = resolved(provider, &mapping, role);
        if strip_marker(&row.model).is_empty() {
            requested.into()
        } else {
            strip_marker(&row.model).into()
        }
    } else if ROLES
        .iter()
        .any(|r| strip_marker(&mapping.row(r).model) == requested)
    {
        requested.to_string()
    } else {
        let lower = requested.to_ascii_lowercase();
        let role = ROLES.iter().find(|role| {
            lower == **role
                || lower.starts_with(&format!("{}[", role))
                || lower.starts_with(&format!("claude-{role}-"))
                || lower.starts_with(&format!("anthropic/claude-{role}-"))
                || (lower.starts_with("claude-3-") && lower.contains(&format!("-{role}-")))
        });
        let row = role.map(|role| resolved(provider, &mapping, role));
        row.as_ref()
            .map(|r| strip_marker(&r.model))
            .filter(|m| !m.is_empty())
            .or_else(|| {
                (!strip_marker(&provider.default_model).is_empty())
                    .then(|| strip_marker(&provider.default_model))
            })
            .unwrap_or(requested)
            .to_string()
    };
    body["model"] = json!(target);
}
