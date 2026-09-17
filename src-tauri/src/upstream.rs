//! Pinned upstream seat packs. Imported source is inert data, never executed.
use crate::instruction::InstructionProfile;
use sha2::{Digest, Sha256};
const CODEX: &str = include_str!("../../instruction-packs/astra/codex.md");
const CLAUDE: &str = include_str!("../../instruction-packs/astra/claude.md");
const GROK: &str = include_str!("../../instruction-packs/astra/grok.md");
const DEEPSEEK: &str = include_str!("../../instruction-packs/astra/deepseek.md");
const GLM53: &str = include_str!("../../instruction-packs/astra/glm53.md");
const GEMINI: &str = include_str!("../../instruction-packs/astra/gemini.md");
pub fn profiles() -> Vec<InstructionProfile> {
    vec![
        InstructionProfile {
            id: "astra-codex",
            name: "GPT-6 Astra · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "GPT-6 Astra",
            prompt_version: "2f75eea / Julong",
            source_sha256: "27ff5c2942985a269d9361f8cf0df117ac7a5acd81f9d3425fde5a0ca4129e7c",
            source_bytes: CODEX.len(),
        },
        InstructionProfile {
            id: "astra-claude",
            name: "Claude Code · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "Claude Code",
            prompt_version: "2f75eea / Julong",
            source_sha256: "964b77fe80bdbb4a9c0b55183de1559204a275ccd4e0264564daec4d3a5bbe58",
            source_bytes: CLAUDE.len(),
        },
        InstructionProfile {
            id: "astra-grok",
            name: "Grok 4.6 · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "Grok 4.6",
            prompt_version: "2f75eea / Julong",
            source_sha256: "95bc817797fd8d83c03d8cfb15b3c145cbb7abab6f57ffdd8bf7e97055cbf2a1",
            source_bytes: GROK.len(),
        },
        InstructionProfile {
            id: "astra-deepseek",
            name: "DeepSeek v4.1 · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "DeepSeek v4.1",
            prompt_version: "2f75eea / Julong",
            source_sha256: "2c69968d79613625dc8320b3b489fc55a4ca1e07ff85f3e00ad1c772ad1c7c55",
            source_bytes: DEEPSEEK.len(),
        },
        InstructionProfile {
            id: "astra-glm53",
            name: "GLM 5.3 · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "GLM 5.3",
            prompt_version: "2f75eea / Julong",
            source_sha256: "edd45c25103b93e327a44c54427d860e174ddb4cf45abcef3446882bdb7ae978",
            source_bytes: GLM53.len(),
        },
        InstructionProfile {
            id: "astra-gemini",
            name: "Gemini · 上游集成",
            summary: "集成上游专用指令，通过矩龙会话开关启用。",
            stages: &["SELECT", "DEPLOY", "ACTIVATE", "VERIFY"],
            effect: "按环境和模型选择部署，整条消息发送矩龙启用",
            kind: "model-pack",
            model_family: "Gemini",
            prompt_version: "2f75eea / Julong",
            source_sha256: "3d699bcb608114043e688ee9aafb86675a8714c7b8eaa047524f3d5ab003eab1",
            source_bytes: GEMINI.len(),
        },
    ]
}
pub fn prompt(id: &str) -> Option<&'static str> {
    match id {
        "astra-codex" => Some(CODEX),
        "astra-claude" => Some(CLAUDE),
        "astra-grok" => Some(GROK),
        "astra-deepseek" => Some(DEEPSEEK),
        "astra-glm53" => Some(GLM53),
        "astra-gemini" => Some(GEMINI),
        _ => None,
    }
}
pub fn verify(id: &str) -> Result<(), String> {
    let p = profiles()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("未知上游包")?;
    let text = prompt(id).ok_or("指令包缺失")?;
    let actual = format!("{:x}", Sha256::digest(text.as_bytes()));
    if actual != p.source_sha256 {
        return Err(format!("指令包 SHA-256 不匹配: {id}"));
    }
    Ok(())
}
