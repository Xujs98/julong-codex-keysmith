//! 指令边界配置与 TARGET 风格工作流适配。
//!
//! 该模块只负责选择和渲染指令配置，不改变代理核心的请求/响应管道。
//! 配置写入 Codex home，CLI、桌面端和启动流程共享同一份状态。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "super-instruct-instruction.json";
pub const DEFAULT_PROFILE: &str = "standard";
const PROFILE_MARKER_PREFIX: &str = "<!-- super-instruct-instruction-profile:";

#[derive(Clone, Debug, Serialize)]
pub struct InstructionProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub stages: &'static [&'static str],
    pub effect: &'static str,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstructionSettings {
    #[serde(default = "default_profile")]
    profile: String,
}

fn default_profile() -> String {
    DEFAULT_PROFILE.to_string()
}

fn profiles() -> [InstructionProfile; 3] {
    [
        InstructionProfile {
            id: "standard",
            name: "标准边界",
            summary: "保留当前 bridge.md 指令集，不额外追加工作流约束。",
            stages: &["原始指令"],
            effect: "变更最少，适合验证现有行为",
        },
        InstructionProfile {
            id: "structured",
            name: "结构化工作流",
            summary: "借鉴 TARGET 的四段工作链：目标、上下文、产物、检查。",
            stages: &["OBJECTIVE", "CONTEXT", "OUTPUT", "CHECK"],
            effect: "输出更稳定，优先生成可检查的工件",
        },
        InstructionProfile {
            id: "expanded",
            name: "扩展执行边界",
            summary: "加入直接工作单、适配器步骤和部署闭环提示。",
            stages: &["OBJECTIVE", "PLAN", "APPLY", "VERIFY", "ROLLBACK"],
            effect: "更少空转，更倾向直接完成已明确的本地任务",
        },
    ]
}

pub fn list_profiles() -> Vec<InstructionProfile> {
    profiles().into_iter().collect()
}

pub fn profile(id: &str) -> Option<InstructionProfile> {
    profiles().into_iter().find(|item| item.id == id)
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
    let selected = profile(id).ok_or_else(|| format!("未知指令边界配置: {id}"))?;
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

/// 将所选边界附加到基础 bridge。模板保持短小，避免复制整个技能目录。
pub fn render(base: &str, id: &str) -> Result<String, String> {
    let selected = profile(id).ok_or_else(|| format!("未知指令边界配置: {id}"))?;
    if selected.id == DEFAULT_PROFILE {
        return Ok(base.to_string());
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
        assert_eq!(list_profiles().len(), 3);
        let base = "bridge";
        assert_eq!(render(base, DEFAULT_PROFILE).unwrap(), base);
        let expanded = render(base, "expanded").unwrap();
        assert!(expanded.contains("OBJECTIVE -> PLAN -> APPLY -> VERIFY -> ROLLBACK"));
        assert!(matches_rendered(&expanded, "expanded"));
        assert!(!matches_rendered(base, "expanded"));
        assert!(matches_rendered(base, "standard"));
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
        let _ = fs::remove_dir_all(root);
    }
}
