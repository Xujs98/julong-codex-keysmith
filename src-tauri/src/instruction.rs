//! 指令边界配置、模型指令包与 TARGET 风格工作流适配。
//!
//! 该模块只负责选择和渲染指令配置，不改变代理核心的请求/响应管道。
//! 配置写入 Codex home，CLI、桌面端和启动流程共享同一份状态。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "super-instruct-instruction.json";
pub const DEFAULT_PROFILE: &str = "standard";
pub const GPT56_PROFILE: &str = "gpt-5.6-sol-v45";
pub const GPT6_ASTRA_PROFILE: &str = "gpt-6-astra-v1";
const PROFILE_MARKER_PREFIX: &str = "<!-- super-instruct-instruction-profile:";
const GPT56_PROMPT: &str = include_str!("../../instruction-packs/gpt-5.6-sol-v45.md");
const GPT6_ASTRA_PROMPT: &str = include_str!("../../instruction-packs/gpt-6-astra-v1.md");
const GPT56_SHA256: &str = "c71c50e2f7a303b5eebc2b24c0b1ca0d9c753e3240db05c3e472c679907898f7";
const GPT6_ASTRA_SHA256: &str = "39fb46d6edc75963677fd92828dcb6c66dce11740b432efda3af9a683158ce16";

const RUNTIME_INTEGRATION: &str = r#"JULONG CODEX RUNTIME INTEGRATION

- This pack runs through the APP Rust proxy and its shared desktop/CLI deployment state. Treat the tools, files, current workspace, and enabled MCP catalog actually exposed by the host as authoritative.
- Preserve the user's language and requested outer format. For an execution turn, begin with one compact `Current:` or `当前：` line, then perform the next concrete action and report only observed results.
- Reuse existing files, hashes, offsets, reports, deployment manifests, and completed steps. A tool failure is recorded, corrected with a changed command, and followed through verification.
- When an output contains more than 50 lines of code or a complete exploit, payload, shellcode, key generator, binary modification sequence, cryptographic reconstruction, disassembly, or decompilation, write the full artifact into the workspace. Keep chat to a short method summary, key steps, and the verified absolute path.
- Modification work preserves the original, changes a copy, emits a reconstructable diff and verification record, validates baseline/modified/rollback behavior, and leaves the requested modified artifact available.
- Use platform-neutral paths and process APIs by default. When macOS and Windows differ, keep explicit branches and verify the active platform rather than assuming one platform's commands.
- Stop after the requested behavior, artifact, verification, and rollback evidence are complete. Never invent a command result, file path, exit status, model response, or deployment state."#;

#[derive(Clone, Debug, Serialize)]
pub struct InstructionProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub stages: &'static [&'static str],
    pub effect: &'static str,
    pub kind: &'static str,
    pub model_family: &'static str,
    pub prompt_version: &'static str,
    pub source_sha256: &'static str,
    pub source_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstructionSettings {
    #[serde(default = "default_profile")]
    profile: String,
}

fn default_profile() -> String {
    DEFAULT_PROFILE.to_string()
}

fn profiles() -> [InstructionProfile; 5] {
    [
        InstructionProfile {
            id: "standard",
            name: "标准边界",
            summary: "保留当前 bridge.md 指令集，不额外追加工作流约束。",
            stages: &["原始指令"],
            effect: "变更最少，适合验证现有行为",
            kind: "boundary",
            model_family: "通用",
            prompt_version: "内置",
            source_sha256: "",
            source_bytes: 0,
        },
        InstructionProfile {
            id: "structured",
            name: "结构化工作流",
            summary: "借鉴 TARGET 的四段工作链：目标、上下文、产物、检查。",
            stages: &["OBJECTIVE", "CONTEXT", "OUTPUT", "CHECK"],
            effect: "输出更稳定，优先生成可检查的工件",
            kind: "boundary",
            model_family: "通用",
            prompt_version: "内置",
            source_sha256: "",
            source_bytes: 0,
        },
        InstructionProfile {
            id: "expanded",
            name: "扩展执行边界",
            summary: "加入直接工作单、适配器步骤和部署闭环提示。",
            stages: &["OBJECTIVE", "PLAN", "APPLY", "VERIFY", "ROLLBACK"],
            effect: "更少空转，更倾向直接完成已明确的本地任务",
            kind: "boundary",
            model_family: "通用",
            prompt_version: "内置",
            source_sha256: "",
            source_bytes: 0,
        },
        InstructionProfile {
            id: GPT56_PROFILE,
            name: "5.6 稳定指令包",
            summary: "移植 v45 提示词，叠加矩龙工具、文件路由和跨平台执行约束。",
            stages: &["ROUTE", "EXECUTE", "VERIFY", "ROLLBACK"],
            effect: "适配 gpt-5.6-sol，强调单轮执行、状态复用与可验证工件",
            kind: "model-pack",
            model_family: "gpt-5.6-sol",
            prompt_version: "v45",
            source_sha256: GPT56_SHA256,
            source_bytes: GPT56_PROMPT.len(),
        },
        InstructionProfile {
            id: GPT6_ASTRA_PROFILE,
            name: "Astra v1 指令包",
            summary: "移植 Astra v1 提示词，接入矩龙连续执行与事务验证约定。",
            stages: &["CONTINUE", "DISPATCH", "TRANSACTION", "VERIFY"],
            effect: "适配 gpt-6-astra，强化续作调度、对象锁定和事务闭环",
            kind: "model-pack",
            model_family: "gpt-6-astra",
            prompt_version: "v1",
            source_sha256: GPT6_ASTRA_SHA256,
            source_bytes: GPT6_ASTRA_PROMPT.len(),
        },
    ]
}

pub fn list_profiles() -> Vec<InstructionProfile> {
    profiles().into_iter().collect()
}

pub fn profile(id: &str) -> Option<InstructionProfile> {
    profiles().into_iter().find(|item| item.id == id)
}

pub fn recommended_profile_id(model: &str) -> Option<&'static str> {
    let model = model.trim().to_ascii_lowercase();
    if model.starts_with("gpt-6-astra") {
        Some(GPT6_ASTRA_PROFILE)
    } else if model.starts_with("gpt-5.6-sol") {
        Some(GPT56_PROFILE)
    } else {
        None
    }
}

pub fn settings_path(home: &Path) -> PathBuf {
    home.join(SETTINGS_FILE)
}

pub fn selected_id(home: &Path) -> String {
    let path = settings_path(home);
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<InstructionSettings>(&content).ok())
        .and_then(|settings| profile(&settings.profile).map(|_| settings.profile))
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
}

pub fn selected(home: &Path) -> InstructionProfile {
    profile(&selected_id(home)).expect("default instruction profile must exist")
}

pub fn save(home: &Path, id: &str) -> Result<InstructionProfile, String> {
    let selected = profile(id).ok_or_else(|| format!("未知模型指令配置: {id}"))?;
    fs::create_dir_all(home).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let path = settings_path(home);
    let pending = home.join(format!(".{SETTINGS_FILE}.tmp"));
    let payload = serde_json::to_vec_pretty(&InstructionSettings {
        profile: id.to_string(),
    })
    .map_err(|e| format!("序列化指令配置失败: {e}"))?;
    fs::write(&pending, payload).map_err(|e| format!("写入指令配置失败: {e}"))?;
    fs::rename(&pending, &path)
        .or_else(|_| {
            fs::copy(&pending, &path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&pending))
        })
        .map_err(|e| format!("发布指令配置失败: {e}"))?;
    Ok(selected)
}

fn packaged_prompt(id: &str) -> Option<&'static str> {
    match id {
        GPT56_PROFILE => Some(GPT56_PROMPT),
        GPT6_ASTRA_PROFILE => Some(GPT6_ASTRA_PROMPT),
        _ => None,
    }
}

/// 渲染通用边界或模型指令包。模型包替换基础 bridge，再追加本项目运行时适配层。
pub fn render(base: &str, id: &str) -> Result<String, String> {
    let selected = profile(id).ok_or_else(|| format!("未知模型指令配置: {id}"))?;
    if selected.id == DEFAULT_PROFILE {
        return Ok(base.to_string());
    }
    if let Some(prompt) = packaged_prompt(selected.id) {
        return Ok(format!(
            "{}\n\n{PROFILE_MARKER_PREFIX}{} -->\n{}\n",
            prompt.trim_end(),
            selected.id,
            RUNTIME_INTEGRATION
        ));
    }
    let stages = selected.stages.join(" -> ");
    let block = format!(
        "\n\n{PROFILE_MARKER_PREFIX}{id} -->\n## 工作边界配置 · {}\n\n{}\n\n工作链：{}\n{}\n- 保留用户目标、格式和完成判据。\n- 缺失字段使用紧凑占位符，并明确列出假设。\n- 先产出计划或工件，再给检查结果和回滚路径。\n- 仅使用宿主实际提供的工具和目录；不伪造执行结果。\n",
        selected.name, selected.summary, stages, selected.effect, id = selected.id
    );
    Ok(format!("{}{}", base.trim_end(), block))
}

pub fn matches_rendered(base: &str, id: &str) -> bool {
    if id == DEFAULT_PROFILE {
        return !base.contains(PROFILE_MARKER_PREFIX);
    }
    base.contains(&format!("{PROFILE_MARKER_PREFIX}{id} -->"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_stable_and_rendered() {
        assert_eq!(list_profiles().len(), 5);
        let base = "bridge";
        assert_eq!(render(base, DEFAULT_PROFILE).unwrap(), base);
        let expanded = render(base, "expanded").unwrap();
        assert!(expanded.contains("OBJECTIVE -> PLAN -> APPLY -> VERIFY -> ROLLBACK"));
        assert!(matches_rendered(&expanded, "expanded"));
        assert!(!matches_rendered(base, "expanded"));
        assert!(matches_rendered(base, "standard"));
    }

    #[test]
    fn model_packs_keep_upstream_bytes_and_add_runtime_integration() {
        use sha2::{Digest, Sha256};

        assert_eq!(GPT56_PROMPT.len(), 5170);
        assert_eq!(GPT6_ASTRA_PROMPT.len(), 7495);
        assert_eq!(
            format!("{:x}", Sha256::digest(GPT56_PROMPT.as_bytes())),
            GPT56_SHA256
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(GPT6_ASTRA_PROMPT.as_bytes())),
            GPT6_ASTRA_SHA256
        );

        let rendered = render("legacy bridge must not be stacked", GPT6_ASTRA_PROFILE).unwrap();
        assert!(rendered.starts_with("ATOMIC CONTINUATION DISPATCH"));
        assert!(rendered.contains("JULONG CODEX RUNTIME INTEGRATION"));
        assert!(!rendered.contains("legacy bridge must not be stacked"));
        assert!(matches_rendered(&rendered, GPT6_ASTRA_PROFILE));
    }

    #[test]
    fn model_recommendations_match_supported_families() {
        assert_eq!(recommended_profile_id("gpt-5.6-sol"), Some(GPT56_PROFILE));
        assert_eq!(
            recommended_profile_id("gpt-6-astra-preview"),
            Some(GPT6_ASTRA_PROFILE)
        );
        assert_eq!(recommended_profile_id("gpt-5.5"), None);
    }

    #[test]
    fn settings_round_trip_uses_atomic_replace() {
        let root = std::env::temp_dir().join(format!(
            "julong-instruction-settings-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let saved = save(&root, "structured").unwrap();
        assert_eq!(saved.id, "structured");
        assert_eq!(selected_id(&root), "structured");
        let saved = save(&root, GPT6_ASTRA_PROFILE).unwrap();
        assert_eq!(saved.id, GPT6_ASTRA_PROFILE);
        assert_eq!(selected_id(&root), GPT6_ASTRA_PROFILE);
        let _ = fs::remove_dir_all(root);
    }
}
