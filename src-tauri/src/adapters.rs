//! 多目标适配器注册表。
//!
//! 适配器描述公共生命周期；实际文件操作继续由 DeployManager、providers、
//! mcp_tools 和 skills 模块完成。这样新增目标时只需增加注册项和实现，不改代理核心。

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AdapterInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub lifecycle: &'static [&'static str],
    pub status: &'static str,
}

const LIFECYCLE: &[&str] = &["detect", "plan", "preview", "apply", "verify", "restore"];

pub fn list() -> Vec<AdapterInfo> {
    vec![
        AdapterInfo {
            id: "codex",
            name: "Codex 配置适配器",
            description: "bridge、config.toml 与 Skills 的事务部署。",
            lifecycle: LIFECYCLE,
            status: "active",
        },
        AdapterInfo {
            id: "provider",
            name: "供应商适配器",
            description: "auth.json、provider 配置和运行时切换。",
            lifecycle: LIFECYCLE,
            status: "active",
        },
        AdapterInfo {
            id: "mcp",
            name: "MCP 工具适配器",
            description: "工具目录、后端探测、启停和导出。",
            lifecycle: LIFECYCLE,
            status: "active",
        },
        AdapterInfo {
            id: "skills",
            name: "Skills 适配器",
            description: "技能扫描、启用偏好、清单和增量同步。",
            lifecycle: LIFECYCLE,
            status: "active",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_unique_ids_and_common_lifecycle() {
        let items = list();
        let ids: std::collections::BTreeSet<_> = items.iter().map(|item| item.id).collect();
        assert_eq!(ids.len(), items.len());
        assert!(items.iter().all(|item| item.lifecycle == LIFECYCLE));
    }
}
