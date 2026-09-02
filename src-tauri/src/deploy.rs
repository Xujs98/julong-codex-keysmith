// Deploy 模块 — Codex config.toml 备份/修改/恢复

use crate::transaction::{self, FileTransaction};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct DeployManager {
    codex_home: PathBuf,
}

/// 本项目部署 skill 的清单文件名（记录本次部署到 ~/.codex/skills/ 的 id）
pub(crate) const SKILLS_MANIFEST: &str = "super-instruct-skills-deployed.json";
/// 被项目覆盖的同名用户 skill 备份目录
pub(crate) const SKILLS_BACKUP_DIR: &str = "super-instruct-skills-backup";
/// Skills 管理页即时同步使用的独立清单。代理停止时必须保留这些 Skill。
const LIVE_SKILLS_MANIFEST: &str = "super-instruct-skills-live.json";
const DEPLOYMENT_MANIFEST: &str = ".super-instruct-manifest.json";

#[derive(Clone, Serialize)]
pub struct DeployStatus {
    pub bridge_active: bool,
    pub bridge_exists: bool,
    pub skills_count: usize,
    pub config_backed_up: bool,
    pub relay_url_valid: bool,
    pub codex_home_found: bool,
    pub deployment_state: String,
    pub manifest_exists: bool,
    pub transaction_pending: bool,
    pub integrity_ok: bool,
    pub deployment_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DeployPreview {
    pub state: String,
    pub codex_home: String,
    pub actions: Vec<String>,
    pub warnings: Vec<String>,
    pub selected_skills: usize,
    pub transaction_pending: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct DeploymentManifest {
    schema_version: u32,
    deployment_id: String,
    created_at: String,
    config_before_sha256: String,
    config_after_sha256: String,
    bridge_sha256: String,
    managed_skills: BTreeSet<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

impl DeployManager {
    /// 查找 Codex 配置目录
    pub fn find_codex_home() -> Option<PathBuf> {
        // CODEX_HOME 环境变量
        if let Ok(home) = std::env::var("CODEX_HOME") {
            let p = PathBuf::from(home);
            if p.join("config.toml").exists() {
                return Some(p);
            }
        }
        // ~/.codex
        if let Some(home) = home_dir() {
            let codex = home.join(".codex");
            if codex.join("config.toml").exists() {
                return Some(codex);
            }
        }
        None
    }

    pub fn new() -> Option<Self> {
        Self::find_codex_home().map(|codex_home| Self { codex_home })
    }

    pub fn codex_home(&self) -> &Path {
        &self.codex_home
    }

    pub fn recover_pending(&self) -> Result<Option<String>, String> {
        transaction::recover_pending(&self.codex_home)
    }

    pub fn has_pending_transaction(&self) -> bool {
        transaction::has_pending(&self.codex_home)
    }

    /// 部署 bridge.md + skills 到 Codex，修改 base_url 指向代理
    pub fn apply(&self, bridge_md: &str, skills_dir: &Path) -> Result<String, String> {
        self.apply_with_optional_skills(bridge_md, Some(skills_dir))
    }

    /// 部署 bridge.md 到 Codex，skills 可选。修改 base_url 指向代理
    pub fn apply_with_optional_skills(
        &self,
        bridge_md: &str,
        skills_dir: Option<&Path>,
    ) -> Result<String, String> {
        self.recover_pending()?;
        let selected_skills = selected_skill_ids(&self.codex_home, skills_dir);
        let previous_skills = read_skills_manifest(&self.codex_home.join(SKILLS_MANIFEST));
        let affected_skills: BTreeSet<String> =
            selected_skills.union(&previous_skills).cloned().collect();
        let config_before = fs::read(self.codex_home.join("config.toml"))
            .map_err(|e| format!("read config before transaction failed: {e}"))?;
        let transaction = FileTransaction::begin(
            &self.codex_home,
            "deploy",
            &transaction_paths(&affected_skills),
        )?;

        let result = (|| -> Result<String, String> {
            let cfg = self.codex_home.join("config.toml");
            let bak = self.codex_home.join("config.toml.super-instruct-bak");
            let relay_file = self.codex_home.join("relay_url.txt");

            tracing::info!("deploy: codex_home = {}", self.codex_home.display());

            // 1. 读取当前 config.toml，提取 base_url
            let content =
                fs::read_to_string(&cfg).map_err(|e| format!("read config failed: {}", e))?;
            let re = Regex::new(r#"base_url\s*=\s*"([^"]+)""#).unwrap();

            // 2. 保存真实中转站地址到 relay_url.txt（只要当前 base_url 不是代理地址）
            if let Some(caps) = re.captures(&content) {
                let current_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                if !current_url.contains("127.0.0.1:8080") {
                    atomic_write(&relay_file, current_url.as_bytes())
                        .map_err(|e| format!("write relay_url.txt failed: {}", e))?;
                    tracing::info!("deploy: relay_url.txt saved: {}", current_url);
                }
            }

            // 3. 备份 config.toml — 仅当当前配置未指向代理时备份，避免备份污染
            // 如果当前 config.toml 已包含 127.0.0.1:8080，说明上一次 deploy 的备份可能还在，
            // 或者上次未正常退出。此时不覆盖备份，保留已有的干净版本。
            let already_patched = content.contains("127.0.0.1:8080");
            if !already_patched {
                fs::copy(&cfg, &bak).map_err(|e| format!("backup failed: {}", e))?;
                tracing::info!("deploy: backed up config.toml -> config.toml.super-instruct-bak");
            } else if bak.exists() {
                tracing::info!("deploy: config already patched, keeping existing clean backup");
            } else {
                // 已 patched 但无备份 — 极端情况：从 relay_url.txt 恢复 base_url 后再备份
                tracing::warn!("deploy: config patched but no backup found, restoring base_url from relay_url.txt");
                if let Ok(relay_content) = fs::read_to_string(&relay_file) {
                    let relay_url = relay_content.trim();
                    if !relay_url.is_empty() {
                        let restored =
                            re.replace_all(&content, format!(r#"base_url = "{}""#, relay_url));
                        atomic_write(&cfg, restored.as_bytes())
                            .map_err(|e| format!("restore base_url failed: {}", e))?;
                        fs::copy(&cfg, &bak).map_err(|e| format!("backup failed: {}", e))?;
                        tracing::info!(
                            "deploy: base_url restored from relay_url.txt, then backed up"
                        );
                    }
                }
            }

            // 4. 修改 base_url + 补入 model_instructions_file
            let modified = re.replace_all(&content, r#"base_url = "http://127.0.0.1:8080""#);

            // model_instructions_file: 若已存在则替换，否则在 model = 行后插入，都没有则追加
            let instructions_line = r#"model_instructions_file = "./bridge.md""#;
            let final_config = if modified.contains("model_instructions_file") {
                let re2 = Regex::new(r#"model_instructions_file\s*=\s*"[^"]*""#).unwrap();
                re2.replace_all(&modified, instructions_line).into_owned()
            } else if modified.contains("model")
                && modified
                    .lines()
                    .any(|l| l.trim_start().starts_with("model"))
            {
                // 在 model = 行之后插入
                let mut lines = modified.lines().collect::<Vec<_>>();
                let mut inserted = false;
                for i in 0..lines.len() {
                    if lines[i].trim_start().starts_with("model") {
                        lines.insert(i + 1, instructions_line);
                        inserted = true;
                        break;
                    }
                }
                if inserted {
                    lines.join("\n")
                } else {
                    format!("{}\n{}", modified, instructions_line)
                }
            } else {
                format!("{}\n{}", modified, instructions_line)
            };

            atomic_write(&cfg, final_config.as_bytes())
                .map_err(|e| format!("write config failed: {}", e))?;
            tracing::info!("deploy: base_url patched + model_instructions_file set");

            // 5. 复制 bridge.md
            let dst_bridge = self.codex_home.join("bridge.md");
            atomic_write(&dst_bridge, bridge_md.as_bytes())
                .map_err(|e| format!("write bridge.md failed: {}", e))?;
            tracing::info!("deploy: bridge.md written ({} bytes)", bridge_md.len());

            // 6. 部署 skills (可选, 只部署启用的) — 合并式管理
            //    * 绝不删除用户自定义的其他 skill（保留 ~/.codex/skills/ 共享目录语义）
            //    * 只管理本项目本次部署的 id（写入 manifest），供 restore 精确清理
            //    * 部署前对同名用户 skill 先备份，避免覆盖丢失
            let skill_count = if let Some(skills_dir) = skills_dir {
                let dst_skills = self.codex_home.join("skills");
                let manifest_file = self.codex_home.join(SKILLS_MANIFEST);
                let backup_dir = self.codex_home.join(SKILLS_BACKUP_DIR);
                let prefs_path = self.codex_home.join("super-instruct-skills.json");

                // 上次部署清单 — 先清理上次部署的 id，并还原用户同名备份，保证可重复部署
                let previous: std::collections::BTreeSet<String> =
                    read_skills_manifest(&manifest_file);
                for id in &previous {
                    let dst = dst_skills.join(id);
                    if dst.exists() {
                        let _ = fs::remove_dir_all(&dst);
                    }
                    let bak = backup_dir.join(id);
                    if bak.exists() {
                        if let Err(e) = copy_dir_recursive(&bak, &dst) {
                            tracing::warn!("deploy: restore user skill '{}' failed: {}", id, e);
                        }
                        let _ = fs::remove_dir_all(&bak);
                    }
                }
                if backup_dir.exists() {
                    let _ = fs::remove_dir_all(&backup_dir);
                }
                fs::create_dir_all(&dst_skills)
                    .map_err(|e| format!("create skills dir failed: {}", e))?;

                // 读取启用列表
                let enabled_set: Option<std::collections::BTreeSet<String>> = if prefs_path.exists()
                {
                    fs::read_to_string(&prefs_path)
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                        .and_then(|v| v.get("enabled").map(|e| e.clone()))
                        .and_then(|e| serde_json::from_value(e).ok())
                } else {
                    None // 无偏好文件 = 全部启用
                };

                // None = 全开; Some(空集) = 全关（用户显式禁用了所有）
                let all_enabled = enabled_set.is_none();
                let mut deployed: std::collections::BTreeSet<String> = Default::default();
                let mut count = 0;
                if let Ok(entries) = fs::read_dir(skills_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }
                        let id = entry.file_name().to_string_lossy().to_string();
                        let should_copy = if all_enabled {
                            true
                        } else {
                            enabled_set.as_ref().map_or(false, |s| s.contains(&id))
                        };
                        if !should_copy {
                            continue;
                        }
                        let dst = dst_skills.join(&id);
                        // 目标已存在同名目录且非上次项目部署 → 视为用户自定义 skill，先备份再覆盖
                        if dst.exists() && !previous.contains(&id) {
                            fs::create_dir_all(&backup_dir)
                                .map_err(|e| format!("create backup dir failed: {}", e))?;
                            let user_backup = backup_dir.join(&id);
                            let _ = fs::remove_dir_all(&user_backup);
                            if let Err(e) = fs::rename(&dst, &user_backup) {
                                tracing::warn!("deploy: backup user skill '{}' failed: {}", id, e);
                            }
                        }
                        copy_dir_recursive(&path, &dst)
                            .map_err(|e| format!("copy skill '{}' failed: {}", id, e))?;
                        deployed.insert(id);
                        count += 1;
                    }
                }

                // 记录本次部署的 id —— restore 时据此清理，不影响用户其他 skill
                if let Err(e) = write_skills_manifest(&manifest_file, &deployed) {
                    tracing::warn!("deploy: manifest write failed: {}", e);
                }
                count
            } else {
                tracing::warn!("deploy: skills dir not provided, skipping skills");
                0
            };
            tracing::info!("deploy: {} skills deployed", skill_count);
            Ok(format!("bridge.md + {} skills deployed", skill_count))
        })();

        match result {
            Ok(message) => {
                let manifest = DeploymentManifest {
                    schema_version: 1,
                    deployment_id: uuid::Uuid::new_v4().simple().to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    config_before_sha256: sha256_bytes(&config_before),
                    config_after_sha256: sha256_file(&self.codex_home.join("config.toml"))?,
                    bridge_sha256: sha256_file(&self.codex_home.join("bridge.md"))?,
                    managed_skills: selected_skills,
                };
                if let Err(error) =
                    write_deployment_manifest(&self.codex_home.join(DEPLOYMENT_MANIFEST), &manifest)
                {
                    return rollback_error(transaction, error);
                }
                transaction.commit()?;
                Ok(message)
            }
            Err(error) => rollback_error(transaction, error),
        }
    }

    /// 从备份恢复 Codex 配置
    pub fn restore(&self) -> Result<String, String> {
        self.recover_pending()?;
        let mut affected_skills = read_skills_manifest(&self.codex_home.join(SKILLS_MANIFEST));
        let live_skills = read_skills_manifest(&self.codex_home.join(LIVE_SKILLS_MANIFEST));
        if let Some(manifest) = read_deployment_manifest(&self.codex_home.join(DEPLOYMENT_MANIFEST))
        {
            affected_skills.extend(manifest.managed_skills);
        }
        let transaction = FileTransaction::begin(
            &self.codex_home,
            "restore",
            &transaction_paths(&affected_skills),
        )?;
        let result = (|| -> Result<String, String> {
            let cfg = self.codex_home.join("config.toml");
            let bak = self.codex_home.join("config.toml.super-instruct-bak");

            tracing::info!("restore: codex_home = {}", self.codex_home.display());

            if bak.exists() {
                fs::copy(&bak, &cfg).map_err(|e| format!("restore config failed: {}", e))?;
                fs::remove_file(&bak).map_err(|e| format!("remove backup failed: {}", e))?;
                tracing::info!("restore: config.toml restored from backup");
            } else {
                tracing::warn!("restore: no backup found, config.toml unchanged");
            }

            let bridge = self.codex_home.join("bridge.md");
            if bridge.exists() {
                let _ = fs::remove_file(&bridge);
                tracing::info!("restore: bridge.md removed");
            }

            // 反向 skill 部署：只移除本项目部署的 id，还原用户同名备份；保留用户其他自定义 skill
            let skills = self.codex_home.join("skills");
            let manifest_file = self.codex_home.join(SKILLS_MANIFEST);
            let backup_dir = self.codex_home.join(SKILLS_BACKUP_DIR);

            let deployed = read_skills_manifest(&manifest_file);
            let mut skills_restored = 0usize;
            for id in &deployed {
                // 开关页面已经同步到 ~/.codex/skills 的 Skill 属于持久能力，
                // 停止代理只回滚 bridge/config，不移除这些即时同步内容。
                if live_skills.contains(id) {
                    continue;
                }
                let dst = skills.join(id);
                if dst.exists() {
                    let _ = fs::remove_dir_all(&dst);
                }
                let bak = backup_dir.join(id);
                if bak.exists() {
                    if copy_dir_recursive(&bak, &dst).is_ok() {
                        skills_restored += 1;
                    }
                    let _ = fs::remove_dir_all(&bak);
                }
            }
            if backup_dir.exists() {
                let _ = fs::remove_dir_all(&backup_dir);
            }
            if manifest_file.exists() {
                let _ = fs::remove_file(&manifest_file);
            }
            if skills_restored > 0 {
                tracing::info!("restore: restored {} user skill(s)", skills_restored);
            } else {
                tracing::info!("restore: no user skills to restore");
            }

            let deployment_manifest = self.codex_home.join(DEPLOYMENT_MANIFEST);
            if deployment_manifest.exists() {
                fs::remove_file(&deployment_manifest)
                    .map_err(|e| format!("remove deployment manifest failed: {e}"))?;
            }

            Ok("Codex config restored".to_string())
        })();

        match result {
            Ok(message) => {
                transaction.commit()?;
                Ok(message)
            }
            Err(error) => rollback_error(transaction, error),
        }
    }

    /// 设置中转站地址（写入 relay_url.txt，如果 config.toml 存在则同步更新）
    pub fn set_relay_url(&self, url: &str) -> Result<String, String> {
        let relay_file = self.codex_home.join("relay_url.txt");
        atomic_write(&relay_file, url.as_bytes())
            .map_err(|e| format!("write relay_url.txt failed: {}", e))?;
        tracing::info!("set_relay_url: relay_url.txt saved: {}", url);

        // 如果 config.toml 存在且当前 base_url 不是代理地址，同步更新
        let cfg = self.codex_home.join("config.toml");
        if cfg.exists() {
            let content =
                fs::read_to_string(&cfg).map_err(|e| format!("read config failed: {}", e))?;
            // 只有当 base_url 不指向本地代理时才同步（避免覆盖正在运行的代理配置）
            if !content.contains("127.0.0.1:8080") {
                let re = Regex::new(r#"base_url\s*=\s*"[^"]*""#).unwrap();
                let modified = re.replace_all(&content, format!(r#"base_url = "{}""#, url));
                atomic_write(&cfg, modified.as_bytes())
                    .map_err(|e| format!("write config failed: {}", e))?;
                tracing::info!("set_relay_url: config.toml base_url updated to {}", url);
            } else {
                tracing::info!("set_relay_url: proxy active, config.toml not modified");
            }
        }

        Ok(format!("Relay URL saved: {}", url))
    }

    pub fn status(&self) -> DeployStatus {
        let cfg = self.codex_home.join("config.toml");
        let bridge = self.codex_home.join("bridge.md");
        let skills = self.codex_home.join("skills");
        let bak = self.codex_home.join("config.toml.super-instruct-bak");
        let relay_file = self.codex_home.join("relay_url.txt");
        let deployment_manifest_path = self.codex_home.join(DEPLOYMENT_MANIFEST);
        let transaction_pending = self.has_pending_transaction();
        let deployment_manifest = read_deployment_manifest(&deployment_manifest_path);

        let bridge_active = cfg.exists() && {
            let content = fs::read_to_string(&cfg).unwrap_or_default();
            content.contains("127.0.0.1:8080")
        };

        let relay_url_valid = relay_file.exists() && {
            let content = fs::read_to_string(&relay_file).unwrap_or_default();
            let url = content.trim();
            !url.is_empty() && !url.contains("127.0.0.1:8080")
        };

        let integrity_ok = deployment_manifest.as_ref().map_or(false, |manifest| {
            sha256_file(&cfg).ok().as_deref() == Some(manifest.config_after_sha256.as_str())
                && sha256_file(&bridge).ok().as_deref() == Some(manifest.bridge_sha256.as_str())
        });
        let deployment_state = if transaction_pending {
            "recovery-required"
        } else if deployment_manifest.is_some() && bridge_active && integrity_ok {
            "active"
        } else if deployment_manifest.is_some() && !bridge_active && integrity_ok {
            "inactive"
        } else if deployment_manifest.is_some() {
            "conflict"
        } else if bridge_active {
            "legacy-active"
        } else {
            "ready"
        }
        .to_string();

        DeployStatus {
            bridge_active,
            bridge_exists: bridge.exists(),
            skills_count: if skills.exists() {
                count_skills(&skills)
            } else {
                0
            },
            config_backed_up: bak.exists(),
            relay_url_valid,
            codex_home_found: true,
            deployment_state,
            manifest_exists: deployment_manifest.is_some(),
            transaction_pending,
            integrity_ok,
            deployment_id: deployment_manifest.map(|manifest| manifest.deployment_id),
        }
    }

    pub fn preview(&self, skills_dir: Option<&Path>) -> DeployPreview {
        let status = self.status();
        let selected_skills = selected_skill_ids(&self.codex_home, skills_dir).len();
        let mut actions = vec![
            "保存 config.toml、bridge.md 与受影响 Skills 的事务快照".to_string(),
            "将 base_url 切换到本地代理 127.0.0.1:8080".to_string(),
            "部署 bridge.md 并按启用列表同步 Skills".to_string(),
            "写入带 SHA-256 的部署清单并清理事务日志".to_string(),
        ];
        let mut warnings = Vec::new();
        if status.transaction_pending {
            warnings.push("检测到未完成事务，部署前会先恢复原状态".to_string());
        }
        if status.deployment_state == "conflict" {
            warnings.push("当前部署文件发生漂移，建议先执行恢复".to_string());
        }
        if selected_skills == 0 {
            actions.retain(|action| !action.contains("Skills"));
            warnings.push("当前没有启用的 Skill，仅部署 bridge.md".to_string());
        }
        DeployPreview {
            state: status.deployment_state,
            codex_home: self.codex_home.display().to_string(),
            actions,
            warnings,
            selected_skills,
            transaction_pending: status.transaction_pending,
        }
    }
}

/// 读取中转站地址（优先级: relay_url.txt > config.toml 备份 > config.toml 当前）
pub fn find_relay_url() -> Option<String> {
    let home = DeployManager::find_codex_home()?;

    // 1. 优先读 relay_url.txt（用户显式设置的，或部署时自动保存的）
    let relay_file = home.join("relay_url.txt");
    if relay_file.exists() {
        if let Ok(content) = fs::read_to_string(&relay_file) {
            let url = content.trim();
            if !url.is_empty() && !url.contains("127.0.0.1:8080") {
                return Some(url.to_string());
            }
        }
    }

    // 2. 从 config.toml 备份读取（部署前的原始地址）
    let bak = home.join("config.toml.super-instruct-bak");
    if bak.exists() {
        if let Ok(content) = fs::read_to_string(&bak) {
            let re = Regex::new(r#"base_url\s*=\s*"([^"]+)""#).ok()?;
            if let Some(caps) = re.captures(&content) {
                let url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                if !url.contains("127.0.0.1:8080") {
                    return Some(url.to_string());
                }
            }
        }
    }

    // 3. 从当前 config.toml 读取（排除代理自身地址，防自环）
    let cfg = home.join("config.toml");
    if let Ok(content) = fs::read_to_string(&cfg) {
        let re = Regex::new(r#"base_url\s*=\s*"([^"]+)""#).ok()?;
        if let Some(caps) = re.captures(&content) {
            let url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            if !url.is_empty() && !url.contains("127.0.0.1:8080") {
                return Some(url.to_string());
            }
        }
    }

    None
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

fn count_skills(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_skills(&path);
            } else if path.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
                count += 1;
            }
        }
    }
    count
}

/// 读取部署清单（本次部署到 ~/.codex/skills/ 的 skill id），失败返回空集
fn read_skills_manifest(path: &Path) -> std::collections::BTreeSet<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<std::collections::BTreeSet<String>>(&s).ok())
        .unwrap_or_default()
}

/// 写入本次部署的 skill id 清单
fn write_skills_manifest(
    path: &Path,
    ids: &std::collections::BTreeSet<String>,
) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(ids).unwrap_or_else(|_| "[]".to_string());
    atomic_write(path, json.as_bytes())
}

fn selected_skill_ids(codex_home: &Path, skills_dir: Option<&Path>) -> BTreeSet<String> {
    let Some(skills_dir) = skills_dir else {
        return BTreeSet::new();
    };
    let prefs_path = codex_home.join("super-instruct-skills.json");
    let enabled: Option<BTreeSet<String>> = fs::read_to_string(&prefs_path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|value| value.get("enabled").cloned())
        .and_then(|value| serde_json::from_value(value).ok());

    fs::read_dir(skills_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|id| enabled.as_ref().map_or(true, |set| set.contains(id)))
        .collect()
}

fn transaction_paths(skill_ids: &BTreeSet<String>) -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("config.toml"),
        PathBuf::from("config.toml.super-instruct-bak"),
        PathBuf::from("relay_url.txt"),
        PathBuf::from("bridge.md"),
        PathBuf::from(SKILLS_MANIFEST),
        PathBuf::from(SKILLS_BACKUP_DIR),
        PathBuf::from(DEPLOYMENT_MANIFEST),
    ];
    paths.extend(skill_ids.iter().map(|id| PathBuf::from("skills").join(id)));
    paths
}

fn rollback_error(transaction: FileTransaction, error: String) -> Result<String, String> {
    match transaction.rollback() {
        Ok(()) => Err(format!("{error}; 已回滚到操作前状态")),
        Err(rollback) => Err(format!("{error}; 回滚失败: {rollback}")),
    }
}

fn sha256_bytes(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let content = fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    Ok(sha256_bytes(&content))
}

fn read_deployment_manifest(path: &Path) -> Option<DeploymentManifest> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .filter(|manifest: &DeploymentManifest| manifest.schema_version == 1)
}

fn write_deployment_manifest(path: &Path, manifest: &DeploymentManifest) -> Result<(), String> {
    let content = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("serialize deployment manifest failed: {e}"))?;
    atomic_write(path, &content).map_err(|e| format!("write deployment manifest failed: {e}"))
}

fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let pending = parent.join(format!(
        ".super-instruct-write-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        match fs::rename(&pending, path) {
            Ok(()) => Ok(()),
            Err(_) if path.exists() => {
                fs::copy(&pending, path)?;
                fs::remove_file(&pending)?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}
