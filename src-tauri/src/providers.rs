//! 供应商管理：持久化多供应商列表，并同步 Codex config.toml/auth.json。
use crate::deploy::{find_relay_url, DeployManager};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const STORE_FILE: &str = "julong-providers.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub official_url: String,
    #[serde(default)]
    pub api_key: String,
    pub request_url: String,
    #[serde(default)]
    pub full_url: bool,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub last_latency_ms: Option<u64>,
    #[serde(default)]
    pub last_status: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Provider {
    pub fn normalized_url(&self) -> String {
        let mut url = self.request_url.trim().trim_end_matches('/').to_string();
        if !self.full_url && !url.ends_with("/v1") {
            url.push_str("/v1");
        }
        url
    }
}

#[derive(Clone, Serialize)]
pub struct ProviderRuntimeStatus {
    pub current: Option<Provider>,
    pub switched: bool,
    pub switch_count: usize,
    pub last_error: String,
}

#[derive(Clone)]
pub struct ProviderRuntime {
    pub providers: Vec<Provider>,
    pub current_index: usize,
    pub switch_count: usize,
    pub last_error: String,
}

impl ProviderRuntime {
    pub fn load() -> Result<Self, String> {
        let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
        let providers = load_or_migrate(&home)?;
        Ok(Self {
            providers,
            current_index: 0,
            switch_count: 0,
            last_error: String::new(),
        })
    }
    pub fn current(&self) -> Option<&Provider> {
        self.providers.get(self.current_index)
    }
    pub fn next(&mut self) -> Option<Provider> {
        if self.providers.len() < 2 {
            return None;
        }
        self.current_index = (self.current_index + 1) % self.providers.len();
        self.switch_count += 1;
        self.providers.get(self.current_index).cloned()
    }
    pub fn status(&self) -> ProviderRuntimeStatus {
        ProviderRuntimeStatus {
            current: self.current().cloned(),
            switched: self.switch_count > 0,
            switch_count: self.switch_count,
            last_error: self.last_error.clone(),
        }
    }
}

fn store_path(home: &Path) -> PathBuf {
    home.join(STORE_FILE)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn load_or_migrate(home: &Path) -> Result<Vec<Provider>, String> {
    let path = store_path(home);
    if path.exists() {
        let text = fs::read_to_string(&path).map_err(|e| format!("read providers failed: {e}"))?;
        return serde_json::from_str(&text).map_err(|e| format!("parse providers failed: {e}"));
    }
    let mut list = Vec::new();
    if let Some(url) = find_relay_url() {
        let now = Utc::now().to_rfc3339();
        let p = Provider {
            id: Uuid::new_v4().to_string(),
            name: "默认供应商".into(),
            note: "从现有中转站配置迁移".into(),
            official_url: String::new(),
            api_key: read_api_key(home).unwrap_or_default(),
            request_url: url,
            full_url: false,
            default_model: read_model(home).unwrap_or_else(|| "gpt-5.6-sol".into()),
            models: Vec::new(),
            last_latency_ms: None,
            last_status: "未测速".into(),
            last_error: String::new(),
            created_at: now.clone(),
            updated_at: now,
        };
        list.push(p);
        save_list(home, &list)?;
    }
    Ok(list)
}

pub fn save_list(home: &Path, list: &[Provider]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    atomic_write(&store_path(home), text.as_bytes())
}

pub fn list() -> Result<Vec<Provider>, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    load_or_migrate(&home)
}

fn read_api_key(home: &Path) -> Option<String> {
    let text = fs::read_to_string(home.join("auth.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::to_string)
}
fn read_model(home: &Path) -> Option<String> {
    let text = fs::read_to_string(home.join("config.toml")).ok()?;
    Regex::new(r#"(?m)^\s*model\s*=\s*"([^"]+)""#)
        .ok()?
        .captures(&text)
        .map(|c| c[1].to_string())
}

pub fn activate(provider: &Provider, proxy_active: bool) -> Result<String, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    let url = provider.normalized_url();
    let relay = home.join("relay_url.txt");
    atomic_write(&relay, url.as_bytes())?;

    let auth_path = home.join("auth.json");
    if auth_path.exists() {
        let text =
            fs::read_to_string(&auth_path).map_err(|e| format!("read auth.json failed: {e}"))?;
        let mut value: Value =
            serde_json::from_str(&text).map_err(|e| format!("parse auth.json failed: {e}"))?;
        if !provider.api_key.trim().is_empty() {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "OPENAI_API_KEY".into(),
                    Value::String(provider.api_key.clone()),
                );
            }
            let out = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
            atomic_write(&auth_path, out.as_bytes())?;
        }
    }

    let cfg = home.join("config.toml");
    if cfg.exists() {
        let text = fs::read_to_string(&cfg).map_err(|e| format!("read config.toml failed: {e}"))?;
        let mut out = text.clone();
        out = replace_or_append(
            &out,
            r#"(?m)^\s*model\s*=\s*"[^"]*""#,
            &format!(r#"model = "{}""#, provider.default_model),
        );
        if let Some(section) = out.find("[model_providers.custom]") {
            let end = out[section..]
                .find("\n[")
                .map(|n| section + n)
                .unwrap_or(out.len());
            let head = &out[..section];
            let body = &out[section..end];
            let tail = &out[end..];
            let body = replace_or_append(
                body,
                r#"(?m)^\s*name\s*=\s*"[^"]*""#,
                &format!(r#"name = "{}""#, provider.name),
            );
            let base = if proxy_active {
                "http://127.0.0.1:8080"
            } else {
                &url
            };
            let body = replace_or_append(
                &body,
                r#"(?m)^\s*base_url\s*=\s*"[^"]*""#,
                &format!(r#"base_url = "{}""#, base),
            );
            out = format!("{}{}{}", head, body, tail);
        }
        atomic_write(&cfg, out.as_bytes())?;
        if let Some(manager) = DeployManager::new() {
            if let Err(error) = manager.refresh_config_integrity() {
                tracing::warn!("refresh provider config integrity failed: {}", error);
            }
        }
    }
    Ok(url)
}

fn replace_or_append(text: &str, pattern: &str, replacement: &str) -> String {
    let re = Regex::new(pattern).unwrap();
    if re.is_match(text) {
        re.replace(text, replacement).into_owned()
    } else {
        format!("{}\n{}", text.trim_end(), replacement)
    }
}

pub fn save(provider: Provider) -> Result<Vec<Provider>, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    let mut list = load_or_migrate(&home)?;
    let mut p = provider;
    if p.id.trim().is_empty() {
        p.id = Uuid::new_v4().to_string();
        p.created_at = Utc::now().to_rfc3339();
    }
    p.updated_at = Utc::now().to_rfc3339();
    if p.name.trim().is_empty() {
        p.name = "未命名供应商".into();
    }
    if let Some(i) = list.iter().position(|x| x.id == p.id) {
        list[i] = p;
    } else {
        list.push(p);
    }
    save_list(&home, &list)?;
    Ok(list)
}

pub fn delete(id: &str) -> Result<Vec<Provider>, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    let mut list = load_or_migrate(&home)?;
    list.retain(|p| p.id != id);
    save_list(&home, &list)?;
    Ok(list)
}

pub fn reorder(ids: &[String]) -> Result<Vec<Provider>, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    let list = load_or_migrate(&home)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(p) = list.iter().find(|p| &p.id == id) {
            out.push(p.clone());
        }
    }
    for p in list {
        if !out.iter().any(|x| x.id == p.id) {
            out.push(p);
        }
    }
    save_list(&home, &out)?;
    Ok(out)
}

pub async fn test(provider: &Provider) -> Result<Provider, String> {
    let started = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(format!("{}/models", provider.normalized_url()));
    if !provider.api_key.trim().is_empty() {
        req = req.bearer_auth(&provider.api_key);
    }
    let response = req.send().await.map_err(|e| e.to_string())?;
    let mut p = provider.clone();
    p.last_latency_ms = Some(started.elapsed().as_millis() as u64);
    p.last_status = response.status().to_string();
    if !response.status().is_success() {
        p.last_error = format!("HTTP {}", response.status());
    }
    Ok(p)
}

pub async fn models(provider: &Provider) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(format!("{}/models", provider.normalized_url()));
    if !provider.api_key.trim().is_empty() {
        req = req.bearer_auth(&provider.api_key);
    }
    let value: Value = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}
