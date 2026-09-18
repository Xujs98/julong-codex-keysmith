//! Claude Desktop 3P configuration, following cc-switch's documented file format.
//! Implemented independently; see docs/verification-0.2.10.md for pinned references.
use crate::providers::Provider;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PROFILE_ID: &str = "8197eec6-032a-4a30-a739-a9e438651710";
pub const PREFIX: &str = "/claude-desktop";
pub const BASE_URL: &str = "http://127.0.0.1:8080/claude-desktop";

pub fn detect_dirs(home: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = home.join(".config");
    candidates(&base)
}

pub fn candidates(base: &Path) -> Vec<PathBuf> {
    let exact = base.join("Claude");
    if exact.is_dir() {
        return vec![exact];
    }
    let mut dirs: Vec<_> = std::fs::read_dir(base)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("Claude") && !n.contains("-3p"))
        })
        .collect();
    dirs.sort();
    dirs
}

pub fn threep_dir(normal: &Path) -> Result<PathBuf, String> {
    let name = normal
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Claude Desktop 配置目录名称无效")?;
    if name.contains("-3p") || name == "configLibrary" {
        return Err("请选择普通 Claude 配置目录，不要选择 Claude-3p 或 configLibrary".into());
    }
    let parent = normal.parent().ok_or("Claude Desktop 配置目录缺少父目录")?;
    // Preserve Windows channel suffixes, e.g. ClaudeDev -> ClaudeDev-3p.
    let exact = parent.join(format!("{name}-3p"));
    if std::fs::symlink_metadata(&exact).is_ok() || name != "Claude" {
        return Ok(exact);
    }
    // Windows distributions can have a distinct 3P channel directory.
    #[cfg(windows)]
    {
        let mut dirs: Vec<_> = std::fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("Claude") && n.contains("-3p"))
            })
            .collect();
        dirs.sort();
        if let Some(path) = dirs.into_iter().next() {
            return Ok(path);
        }
    }
    Ok(exact)
}

#[derive(Clone, Copy)]
pub enum FileKind {
    Mode,
    Profile,
    Meta,
}
pub fn targets(root: &Path) -> Result<Vec<(PathBuf, FileKind)>, String> {
    let threep = threep_dir(root)?;
    Ok(vec![
        (root.join("claude_desktop_config.json"), FileKind::Mode),
        (threep.join("claude_desktop_config.json"), FileKind::Mode),
        (
            threep.join(format!("configLibrary/{PROFILE_ID}.json")),
            FileKind::Profile,
        ),
        (threep.join("configLibrary/_meta.json"), FileKind::Meta),
    ])
}

pub fn models(provider: &Provider) -> Vec<String> {
    crate::claude_models::upstream_models(provider)
}

pub fn render(
    kind: FileKind,
    original: Option<&[u8]>,
    provider: &Provider,
) -> Result<Vec<u8>, String> {
    let mut value: Value = original
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|e| format!("Claude Desktop JSON 无效，未覆盖: {e}"))?
        .unwrap_or_else(|| json!({}));
    let obj = value
        .as_object_mut()
        .ok_or("Claude Desktop 配置必须为 JSON 对象")?;
    match kind {
        FileKind::Mode => {
            obj.insert("deploymentMode".into(), json!("3p"));
        }
        FileKind::Profile => {
            if provider.api_key.trim().is_empty() {
                return Err("Claude Desktop 供应商 API Key 不能为空".into());
            }
            let models = crate::claude_models::desktop_models(provider);
            if models.is_empty() {
                return Err("Claude Desktop 需要至少一个角色映射或默认兜底模型（可使用 gpt-6-astra 等上游模型）；供应商须支持 Anthropic Messages".into());
            }
            for (key, value) in [
                ("inferenceProvider", json!("gateway")),
                ("inferenceGatewayBaseUrl", json!(BASE_URL)),
                ("inferenceGatewayApiKey", json!(provider.api_key.trim())),
                ("inferenceGatewayAuthScheme", json!("bearer")),
                ("disableDeploymentModeChooser", json!(true)),
                ("inferenceModels", json!(models)),
            ] {
                obj.insert(key.into(), value);
            }
        }
        FileKind::Meta => {
            let entries = obj
                .entry("entries")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("Claude Desktop _meta.json 的 entries 必须为数组")?;
            entries.retain(|e| e.get("id").and_then(Value::as_str) != Some(PROFILE_ID));
            entries.push(json!({"id":PROFILE_ID,"name":"矩龙"}));
            obj.insert("appliedId".into(), json!(PROFILE_ID));
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A distinct URL namespace is reliable even when Desktop supplies no custom headers.
pub fn normalize_request(headers: &http::HeaderMap, path: &str) -> (http::HeaderMap, String) {
    let mut headers = headers.clone();
    if let Some(path) = path.strip_prefix(&format!("{PREFIX}/")) {
        headers.insert(
            "x-julong-environment",
            http::HeaderValue::from_static("claude-desktop"),
        );
        (headers, format!("/{path}"))
    } else {
        (headers, path.into())
    }
}
