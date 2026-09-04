//! 供应商管理：持久化多供应商列表，并同步 Codex config.toml/auth.json。
use crate::deploy::{
    backup_auth_if_needed, backup_provider_config_if_needed, find_relay_url_at, DeployManager,
    AUTH_BACKUP_FILE, DEPLOYMENT_MANIFEST, PROVIDER_CONFIG_BACKUP_FILE,
};
use crate::transaction::FileTransaction;
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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

/// 判断地址是否可以作为代理上游，排除空值和代理自身地址，防止形成自环。
pub fn valid_relay_url(url: &str) -> bool {
    let trimmed = url.trim();
    !trimmed.is_empty() && !trimmed.contains("127.0.0.1:8080")
}

/// 解析启动时可用的上游地址。
///
/// 已保存的供应商优先于旧版 relay_url/config 迁移数据；这样首装后只要供应商
/// 已保存，即使 relay_url.txt 尚未生成，也能正常进入启动流程。
pub fn configured_relay_url(home: &Path) -> Option<String> {
    if let Ok(list) = load_or_migrate(home) {
        for provider in list {
            let url = provider.normalized_url();
            if valid_relay_url(&url) {
                return Some(url);
            }
        }
    }
    find_relay_url_at(home).filter(|url| valid_relay_url(url))
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
    if let Some(url) = find_relay_url_at(home) {
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
    let transaction = FileTransaction::begin(
        &home,
        "provider-activate",
        &[
            PathBuf::from("config.toml"),
            PathBuf::from("config.toml.super-instruct-bak"),
            PathBuf::from(PROVIDER_CONFIG_BACKUP_FILE),
            PathBuf::from("auth.json"),
            PathBuf::from(AUTH_BACKUP_FILE),
            PathBuf::from("relay_url.txt"),
            PathBuf::from(DEPLOYMENT_MANIFEST),
        ],
    )?;

    let result = (|| -> Result<String, String> {
        let relay = home.join("relay_url.txt");
        let auth_path = home.join("auth.json");
        let cfg = home.join("config.toml");

        if !provider.api_key.trim().is_empty() && auth_path.exists() {
            backup_auth_if_needed(&home)?;
        }
        if !proxy_active && cfg.exists() {
            backup_provider_config_if_needed(&home)?;
        }
        atomic_write(&relay, url.as_bytes())?;

        let auth_exists = auth_path.exists();
        let auth_text = if auth_exists {
            fs::read_to_string(&auth_path).map_err(|e| format!("read auth.json failed: {e}"))?
        } else {
            String::new()
        };
        // 首装或用户清空 auth.json 后，仍然生成合法 JSON 并写入当前供应商的 Key。
        // 没有 Key 时保留已有认证内容；空文件则初始化为空对象，避免下次启动解析失败。
        if !provider.api_key.trim().is_empty() {
            let out = update_auth_content(&auth_text, &provider.api_key)?;
            atomic_write(&auth_path, out.as_bytes())?;
        } else if !auth_exists || auth_text.trim().is_empty() {
            atomic_write(&auth_path, b"{}\n")?;
        }

        let text = if cfg.exists() {
            fs::read_to_string(&cfg).map_err(|e| format!("read config.toml failed: {e}"))?
        } else {
            String::new()
        };
        let base = if proxy_active {
            "http://127.0.0.1:8080"
        } else {
            &url
        };
        let out = render_provider_config(&text, provider, base);
        atomic_write(&cfg, out.as_bytes())?;
        Ok(url)
    })();

    match result {
        Ok(value) => {
            transaction.commit()?;
            if let Some(manager) = DeployManager::new() {
                if let Err(error) = manager.refresh_config_integrity() {
                    tracing::warn!("refresh provider config integrity failed: {}", error);
                }
            }
            Ok(value)
        }
        Err(error) => match transaction.rollback() {
            Ok(()) => Err(format!("{error}; 已回滚到操作前状态")),
            Err(rollback) => Err(format!("{error}; 回滚失败: {rollback}")),
        },
    }
}

fn toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn update_auth_content(existing: &str, api_key: &str) -> Result<String, String> {
    let mut value = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(existing).map_err(|e| format!("parse auth.json failed: {e}"))?
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| "auth.json must contain a JSON object".to_string())?;
    if !api_key.trim().is_empty() {
        object.insert(
            "OPENAI_API_KEY".into(),
            Value::String(api_key.trim().to_string()),
        );
    }
    serde_json::to_string_pretty(&value).map_err(|e| format!("serialize auth.json failed: {e}"))
}

fn render_provider_config(existing: &str, provider: &Provider, base_url: &str) -> String {
    let model = if provider.default_model.trim().is_empty() {
        "gpt-5.6-sol"
    } else {
        provider.default_model.trim()
    };
    let model_line = format!(r#"model = "{}""#, toml_string(model));
    let provider_line = r#"model_provider = "custom""#;
    let reasoning_line = r#"model_reasoning_effort = "high""#;
    let storage_line = "disable_response_storage = true";
    let instructions_line = r#"model_instructions_file = "./bridge.md""#;
    let mut out = existing.trim_end().to_string();

    if out.trim().is_empty() {
        return format!(
            "{}\n{}\n{}\n{}\n{}\n\n[model_providers.custom]\nname = \"{}\"\nbase_url = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
            model_line,
            provider_line,
            reasoning_line,
            storage_line,
            instructions_line,
            toml_string(&provider.name),
            toml_string(base_url)
        );
    }

    let mut missing_root_lines = Vec::new();
    if Regex::new(r#"(?m)^\s*model\s*=\s*"[^"]*""#)
        .unwrap()
        .is_match(&out)
    {
        out = replace_or_append(&out, r#"(?m)^\s*model\s*=\s*"[^"]*""#, &model_line);
    } else {
        missing_root_lines.push(model_line);
    }
    if Regex::new(r#"(?m)^\s*model_provider\s*=\s*"[^"]*""#)
        .unwrap()
        .is_match(&out)
    {
        out = replace_or_append(
            &out,
            r#"(?m)^\s*model_provider\s*=\s*"[^"]*""#,
            provider_line,
        );
    } else {
        missing_root_lines.push(provider_line.to_string());
    }
    if Regex::new(r#"(?m)^\s*model_reasoning_effort\s*=\s*"[^"]*""#)
        .unwrap()
        .is_match(&out)
    {
        out = replace_or_append(
            &out,
            r#"(?m)^\s*model_reasoning_effort\s*=\s*"[^"]*""#,
            reasoning_line,
        );
    } else {
        missing_root_lines.push(reasoning_line.to_string());
    }
    if Regex::new(r#"(?m)^\s*disable_response_storage\s*=\s*(?:true|false)\s*$"#)
        .unwrap()
        .is_match(&out)
    {
        out = replace_or_append(
            &out,
            r#"(?m)^\s*disable_response_storage\s*=\s*(?:true|false)\s*$"#,
            storage_line,
        );
    } else {
        missing_root_lines.push(storage_line.to_string());
    }
    if Regex::new(r#"(?m)^\s*model_instructions_file\s*=\s*"[^"]*""#)
        .unwrap()
        .is_match(&out)
    {
        out = replace_or_append(
            &out,
            r#"(?m)^\s*model_instructions_file\s*=\s*"[^"]*""#,
            instructions_line,
        );
    } else {
        missing_root_lines.push(instructions_line.to_string());
    }
    if !missing_root_lines.is_empty() {
        out = format!("{}\n{}", missing_root_lines.join("\n"), out.trim_start());
    }

    let section = "[model_providers.custom]";
    if let Some(start) = out.find(section) {
        let end = out[start..]
            .find("\n[")
            .map(|offset| start + offset)
            .unwrap_or(out.len());
        let head = &out[..start];
        let body = &out[start..end];
        let tail = &out[end..];
        let body = replace_or_append(
            body,
            r#"(?m)^\s*name\s*=\s*"[^"]*""#,
            &format!(r#"name = "{}""#, toml_string(&provider.name)),
        );
        let body = replace_or_append(
            &body,
            r#"(?m)^\s*base_url\s*=\s*"[^"]*""#,
            &format!(r#"base_url = "{}""#, toml_string(base_url)),
        );
        let body = replace_or_append(
            &body,
            r#"(?m)^\s*wire_api\s*=\s*"[^"]*""#,
            r#"wire_api = "responses""#,
        );
        let body = replace_or_append(
            &body,
            r#"(?m)^\s*requires_openai_auth\s*=\s*(?:true|false)\s*$"#,
            "requires_openai_auth = true",
        );
        out = format!("{}{}{}", head, body, tail);
    } else {
        out.push_str(&format!(
            "\n\n{}\nname = \"{}\"\nbase_url = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
            section,
            toml_string(&provider.name),
            toml_string(base_url)
        ));
    }
    format!("{}\n", out.trim_end())
}

fn replace_or_append(text: &str, pattern: &str, replacement: &str) -> String {
    let re = Regex::new(pattern).unwrap();
    if re.is_match(text) {
        re.replace(text, regex::NoExpand(replacement)).into_owned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn provider() -> Provider {
        Provider {
            id: "provider-1".into(),
            name: "主力供应商".into(),
            note: String::new(),
            official_url: String::new(),
            api_key: "sk-test".into(),
            request_url: "https://relay.example".into(),
            full_url: false,
            default_model: "gpt-test".into(),
            models: Vec::new(),
            last_latency_ms: None,
            last_status: String::new(),
            last_error: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn empty_auth_is_initialized_with_provider_key() {
        let value = update_auth_content("", "sk-test").unwrap();
        let parsed: Value = serde_json::from_str(&value).unwrap();
        assert_eq!(parsed["OPENAI_API_KEY"], "sk-test");
    }

    #[test]
    fn empty_config_is_initialized_with_provider_section() {
        let value = render_provider_config("", &provider(), "http://127.0.0.1:8080");
        let parsed: toml::Value = toml::from_str(&value).unwrap();
        assert!(value.contains("model = \"gpt-test\""));
        assert!(value.contains("model_provider = \"custom\""));
        assert!(value.contains("model_reasoning_effort = \"high\""));
        assert!(value.contains("disable_response_storage = true"));
        assert!(value.contains("model_instructions_file = \"./bridge.md\""));
        assert!(value.contains("[model_providers.custom]"));
        assert!(value.contains("name = \"主力供应商\""));
        assert!(value.contains("base_url = \"http://127.0.0.1:8080\""));
        assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
    }

    #[test]
    fn existing_config_gets_root_keys_before_provider_table() {
        let value = render_provider_config(
            "sandbox_mode = \"workspace-write\"\n\n[model_providers.custom]\nname = \"old\"\nbase_url = \"https://old.example/v1\"\n",
            &provider(),
            "https://relay.example/v1",
        );
        let _: toml::Value = toml::from_str(&value).unwrap();
        assert!(value.starts_with("model = \"gpt-test\"\nmodel_provider = \"custom\""));
        assert!(value.contains("model_reasoning_effort = \"high\""));
        assert!(value.contains("disable_response_storage = true"));
        assert!(value.contains("sandbox_mode = \"workspace-write\""));
        assert!(value.contains("base_url = \"https://relay.example/v1\""));
    }

    #[test]
    fn incomplete_existing_config_gets_required_root_keys() {
        let value = render_provider_config(
            "model = \"old-model\"\nmodel_provider = \"old\"\n\n[model_providers.custom]\nname = \"old\"\nbase_url = \"https://old.example/v1\"\n",
            &provider(),
            "https://relay.example/v1",
        );
        let parsed: toml::Value = toml::from_str(&value).unwrap();
        assert_eq!(parsed["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(parsed["disable_response_storage"].as_bool(), Some(true));
        assert_eq!(parsed["model"].as_str(), Some("gpt-test"));
        assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
    }

    #[test]
    fn activate_initializes_empty_codex_files() {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "julong-provider-activate-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("config.toml"), "").unwrap();
        fs::write(root.join("auth.json"), "").unwrap();

        let previous = std::env::var_os("CODEX_HOME");
        std::env::set_var("CODEX_HOME", &root);
        let result = activate(&provider(), true);
        if let Some(value) = previous {
            std::env::set_var("CODEX_HOME", value);
        } else {
            std::env::remove_var("CODEX_HOME");
        }

        result.unwrap();
        let config = fs::read_to_string(root.join("config.toml")).unwrap();
        let auth: Value =
            serde_json::from_str(&fs::read_to_string(root.join("auth.json")).unwrap()).unwrap();
        assert!(config.contains("model_provider = \"custom\""));
        assert!(config.contains("base_url = \"http://127.0.0.1:8080\""));
        assert_eq!(auth["OPENAI_API_KEY"], "sk-test");
        fs::remove_dir_all(root).unwrap();
    }
}
