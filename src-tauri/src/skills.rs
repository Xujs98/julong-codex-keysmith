// Skills 管理模块 — 扫描源 skills、解析元数据、管理启用状态
//
// 数据流:
//   codex-skills/ (源) → scan_skills() → Vec<SkillInfo>
//   skills.json (持久化启用状态) → load_prefs() / save_prefs()
//   deploy 时 → get_enabled_skills() → 只复制启用的

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::deploy::{DeployManager, SKILLS_BACKUP_DIR, SKILLS_MANIFEST};
use crate::transaction::FileTransaction;
use tauri::Manager;

const LIVE_MANIFEST: &str = "super-instruct-skills-live.json";
const LIVE_BACKUP_DIR: &str = "super-instruct-skills-live-backup";

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub file_count: usize,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct SkillsPrefs {
    /// 启用的 skill id 集合；不存在 = 全部启用（首次运行默认全开）
    enabled: Option<BTreeSet<String>>,
}

// ── 路径解析 ──────────────────────────────────

fn prefs_path() -> Option<PathBuf> {
    let home = DeployManager::find_codex_home()?;
    Some(home.join("super-instruct-skills.json"))
}

fn source_skills_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    // 复用 lib.rs 的 resolve_resource_dir 逻辑
    if let Ok(base) = app.path().resource_dir() {
        let p = base.join("codex-skills");
        if p.is_dir() {
            return Some(p);
        }
    }
    // Dev fallback: 项目根/codex-skills
    let candidates = [
        std::path::PathBuf::from("codex-skills"),
        std::env::current_dir().ok()?.join("codex-skills"),
        std::env::current_dir().ok()?.parent()?.join("codex-skills"),
    ];
    for p in &candidates {
        if p.is_dir() {
            return Some(p.clone());
        }
    }
    None
}

// ── 元数据解析 ────────────────────────────────

/// 从 SKILL.md frontmatter 提取 name 和 description
fn parse_skill_metadata(skill_dir: &Path) -> (String, String) {
    let skill_md = skill_dir.join("SKILL.md");
    let content = fs::read_to_string(&skill_md).unwrap_or_default();

    let mut name = skill_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut description = String::new();

    // 解析 YAML frontmatter (--- ... ---)
    if content.starts_with("---") {
        let end = content[3..].find("---");
        if let Some(end_pos) = end {
            let frontmatter = &content[3..3 + end_pos];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("name:") {
                    name = rest.trim().trim_matches('"').to_string();
                } else if let Some(rest) = line.strip_prefix("description:") {
                    description = rest.trim().trim_matches('"').to_string();
                }
            }
        }
    }

    (name, description)
}

/// 递归统计目录下文件数
fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
    }
    count
}

// ── 偏好读写 ──────────────────────────────────

fn load_prefs() -> SkillsPrefs {
    let Some(path) = prefs_path() else {
        return SkillsPrefs::default();
    };
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_prefs(prefs: &SkillsPrefs) {
    let Some(path) = prefs_path() else { return };
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(&path, json);
    }
}

fn source_skill_ids(src_dir: &Path) -> BTreeSet<String> {
    fs::read_dir(src_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

fn read_manifest(path: &Path) -> BTreeSet<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_manifest(path: &Path, ids: &BTreeSet<String>) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(ids).map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "manifest parent missing".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path)
        .or_else(|_| {
            fs::copy(&tmp, path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&tmp))
        })
        .map_err(|e| e.to_string())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn valid_skill_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains('/') && !id.contains('\\')
}

// ── 公开 API ──────────────────────────────────

/// 扫描源 skills 目录，返回所有 skill 信息 + 启用状态
/// 首次运行（enabled=None）时自动初始化为全启用并持久化
pub fn scan_skills(app: &tauri::AppHandle) -> Vec<SkillInfo> {
    let Some(src_dir) = source_skills_dir(app) else {
        tracing::warn!("skills: source codex-skills dir not found");
        return Vec::new();
    };

    let prefs = load_prefs();

    // 收集所有 skill id
    let mut all_ids: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                all_ids.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    // 首次运行（enabled=None）：初始化为全启用集合并持久化
    let enabled_set: BTreeSet<String> = match &prefs.enabled {
        None => {
            let set: BTreeSet<String> = all_ids.iter().cloned().collect();
            save_prefs(&SkillsPrefs {
                enabled: Some(set.clone()),
            });
            set
        }
        Some(s) => s.clone(),
    };

    let mut skills: Vec<SkillInfo> = Vec::new();

    for id in &all_ids {
        let path = src_dir.join(id);
        let (name, description) = parse_skill_metadata(&path);
        let file_count = count_files(&path);
        let enabled = enabled_set.contains(id);

        skills.push(SkillInfo {
            id: id.clone(),
            name,
            description,
            enabled,
            file_count,
        });
    }

    // 按名称排序
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    skills
}

/// 切换单个 skill 的启用状态
pub fn toggle_skill(app: &tauri::AppHandle, id: &str, enabled: bool) -> Result<(), String> {
    if !valid_skill_id(id) {
        return Err("invalid skill id".into());
    }
    let mut prefs = load_prefs();
    if prefs.enabled.is_none() {
        let all = source_skills_dir(app)
            .map(|p| source_skill_ids(&p))
            .unwrap_or_default();
        prefs.enabled = Some(all);
    }
    if let Some(ref mut set) = prefs.enabled {
        if enabled {
            set.insert(id.to_string());
        } else {
            set.remove(id);
        }
    }
    save_prefs(&prefs);
    Ok(())
}

/// 批量设置启用状态
pub fn set_enabled(ids: &[String], enabled: bool) {
    let mut prefs = load_prefs();
    if prefs.enabled.is_none() {
        prefs.enabled = Some(BTreeSet::new());
    }
    if let Some(ref mut set) = prefs.enabled {
        for id in ids {
            if enabled {
                set.insert(id.clone());
            } else {
                set.remove(id);
            }
        }
    }
    save_prefs(&prefs);
}

/// 全部启用 / 全部禁用
pub fn set_all(enabled: bool) {
    let prefs = if enabled {
        SkillsPrefs {
            enabled: None, // None = 全开
        }
    } else {
        SkillsPrefs {
            enabled: Some(BTreeSet::new()), // 空集 = 全关
        }
    };
    save_prefs(&prefs);
}

/// 将当前启用的 Skills 即时同步到 ~/.codex/skills/，不触碰 config.toml。
/// 由 Skills 开关调用；独立清单和备份避免与代理部署事务相互覆盖。
pub fn sync_enabled_skills(app: &tauri::AppHandle) -> Result<String, String> {
    let src = source_skills_dir(app).ok_or_else(|| "源 skills 目录未找到".to_string())?;
    let codex_home =
        DeployManager::find_codex_home().ok_or_else(|| "Codex 配置目录未找到".to_string())?;
    // 开关操作只改 ~/.codex/skills 与清单，但仍使用同一事务日志，保证复制、备份、
    // 清单更新任一步失败时都能在本次调用或下次启动恢复到完整快照。
    let enabled: BTreeSet<String> = scan_skills(app)
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.id)
        .collect();
    let previous = read_manifest(&codex_home.join(LIVE_MANIFEST));
    let legacy = read_manifest(&codex_home.join(SKILLS_MANIFEST));
    let mut affected = vec![
        PathBuf::from("super-instruct-skills.json"),
        PathBuf::from(LIVE_MANIFEST),
        PathBuf::from(LIVE_BACKUP_DIR),
        // 兼容早期“随代理部署 Skills”的版本。即时开关接管同名 Skill 时，
        // 需要将旧备份移交给 live 管理器，避免停止代理后误清理用户原始 Skill。
        PathBuf::from(SKILLS_MANIFEST),
        PathBuf::from(SKILLS_BACKUP_DIR),
    ];
    affected.extend(
        previous
            .union(&enabled)
            .map(|id| PathBuf::from("skills").join(id)),
    );
    affected.extend(legacy.iter().map(|id| PathBuf::from("skills").join(id)));
    let transaction = FileTransaction::begin(&codex_home, "skills-sync", &affected)?;

    let result = sync_enabled_skills_inner(&src, &codex_home, &enabled, &previous);
    match result {
        Ok(message) => {
            transaction.commit()?;
            Ok(message)
        }
        Err(error) => match transaction.rollback() {
            Ok(()) => Err(format!("{error}; 已回滚到同步前状态")),
            Err(rollback) => Err(format!("{error}; 回滚失败: {rollback}")),
        },
    }
}

fn sync_enabled_skills_inner(
    src: &Path,
    codex_home: &Path,
    enabled: &BTreeSet<String>,
    previous: &BTreeSet<String>,
) -> Result<String, String> {
    let dst_root = codex_home.join("skills");
    let manifest_path = codex_home.join(LIVE_MANIFEST);
    let backup_root = codex_home.join(LIVE_BACKUP_DIR);
    fs::create_dir_all(&dst_root).map_err(|e| format!("create skills dir failed: {e}"))?;

    migrate_legacy_deployed_backups(codex_home, enabled, &backup_root)?;

    // Remove stale live copies and restore any user skill saved by this live synchronizer.
    for id in previous.difference(enabled) {
        let dst = dst_root.join(id);
        if dst.exists() {
            let _ = fs::remove_dir_all(&dst);
        }
        let backup = backup_root.join(id);
        if backup.exists() {
            copy_dir_recursive(&backup, &dst)?;
            let _ = fs::remove_dir_all(&backup);
        }
    }

    let mut synced = BTreeSet::new();
    for id in enabled {
        if !valid_skill_id(id) {
            continue;
        }
        let source = src.join(id);
        if !source.is_dir() {
            continue;
        }
        let dst = dst_root.join(id);
        if dst.exists() && !previous.contains(id) {
            fs::create_dir_all(&backup_root).map_err(|e| e.to_string())?;
            let backup = backup_root.join(id);
            if !backup.exists() {
                fs::rename(&dst, &backup)
                    .map_err(|e| format!("backup skill '{id}' failed: {e}"))?;
            }
        }
        if dst.exists() {
            let _ = fs::remove_dir_all(&dst);
        }
        copy_dir_recursive(&source, &dst)?;
        synced.insert(id.clone());
    }
    if synced.is_empty() {
        if manifest_path.exists() {
            let _ = fs::remove_file(&manifest_path);
        }
    } else {
        write_manifest(&manifest_path, &synced)?;
    }
    if backup_root.exists()
        && fs::read_dir(&backup_root)
            .map(|mut i| i.next().is_none())
            .unwrap_or(false)
    {
        let _ = fs::remove_dir_all(&backup_root);
    }
    Ok(format!("已同步 {} 个 Skill", synced.len()))
}

/// 早期版本由 deploy 管理 Skills，现由 Skills 页即时同步管理。若用户此前已有
/// 同名 Skill，旧 deploy 备份就是唯一的用户原始副本；在 live 接管时将它转交给
/// live 备份目录，并从旧清单剔除，保证代理启停不会删除或覆盖该副本。
fn migrate_legacy_deployed_backups(
    codex_home: &Path,
    enabled: &BTreeSet<String>,
    live_backup_root: &Path,
) -> Result<(), String> {
    let legacy_manifest = codex_home.join(SKILLS_MANIFEST);
    let legacy_backup_root = codex_home.join(SKILLS_BACKUP_DIR);
    let mut legacy = read_manifest(&legacy_manifest);
    let legacy_ids: Vec<String> = legacy.iter().cloned().collect();

    for id in &legacy_ids {
        let legacy_backup = legacy_backup_root.join(id);
        if enabled.contains(id) {
            let live_backup = live_backup_root.join(id);
            if legacy_backup.exists() && !live_backup.exists() {
                fs::create_dir_all(live_backup_root).map_err(|e| e.to_string())?;
                fs::rename(&legacy_backup, &live_backup)
                    .map_err(|e| format!("migrate legacy backup '{id}' failed: {e}"))?;
            }
        } else {
            // The old deployment is no longer enabled. Restore its user copy now,
            // rather than leaving a stale project copy in ~/.codex/skills/.
            let dst = codex_home.join("skills").join(id);
            if dst.exists() {
                fs::remove_dir_all(&dst)
                    .map_err(|e| format!("remove stale skill '{id}' failed: {e}"))?;
            }
            if legacy_backup.exists() {
                copy_dir_recursive(&legacy_backup, &dst)?;
                fs::remove_dir_all(&legacy_backup).map_err(|e| e.to_string())?;
            }
        }
        legacy.remove(id);
    }

    if legacy.is_empty() {
        if legacy_manifest.exists() {
            fs::remove_file(&legacy_manifest).map_err(|e| e.to_string())?;
        }
    } else {
        write_manifest(&legacy_manifest, &legacy)?;
    }
    if legacy_backup_root.exists()
        && fs::read_dir(&legacy_backup_root)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    {
        fs::remove_dir_all(&legacy_backup_root).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 返回当前启用的 skill id 列表（供 deploy 使用）
/// 需要传入 app 来扫描源目录
pub fn get_enabled_skill_ids(app: &tauri::AppHandle) -> BTreeSet<String> {
    let skills = scan_skills(app);
    skills
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.id)
        .collect()
}

/// 删除源 skills 目录中的某个 skill
pub fn delete_skill(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let Some(src_dir) = source_skills_dir(app) else {
        return Err("源 skills 目录未找到".into());
    };
    let skill_dir = src_dir.join(id);
    if !skill_dir.exists() {
        return Err(format!("Skill '{}' 不存在", id));
    }
    fs::remove_dir_all(&skill_dir).map_err(|e| format!("删除失败: {}", e))?;

    // 从偏好中移除
    let mut prefs = load_prefs();
    if let Some(ref mut set) = prefs.enabled {
        set.remove(id);
    }
    save_prefs(&prefs);
    sync_enabled_skills(app)?;

    tracing::info!("skills: deleted skill '{}'", id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sync_replaces_then_restores_user_skill() {
        let root = std::env::temp_dir().join(format!(
            "super-instruct-live-skills-test-{}",
            uuid::Uuid::new_v4()
        ));
        let src = root.join("source");
        let codex_home = root.join("codex");
        fs::create_dir_all(src.join("novel-agent")).unwrap();
        fs::create_dir_all(codex_home.join("skills/novel-agent")).unwrap();
        fs::write(src.join("novel-agent/SKILL.md"), "project skill").unwrap();
        fs::write(codex_home.join("skills/novel-agent/SKILL.md"), "user skill").unwrap();

        let enabled = BTreeSet::from(["novel-agent".to_string()]);
        let previous = BTreeSet::new();
        sync_enabled_skills_inner(&src, &codex_home, &enabled, &previous).unwrap();
        assert_eq!(
            fs::read_to_string(codex_home.join("skills/novel-agent/SKILL.md")).unwrap(),
            "project skill"
        );
        assert!(codex_home
            .join(LIVE_BACKUP_DIR)
            .join("novel-agent/SKILL.md")
            .exists());

        sync_enabled_skills_inner(&src, &codex_home, &BTreeSet::new(), &enabled).unwrap();
        assert_eq!(
            fs::read_to_string(codex_home.join("skills/novel-agent/SKILL.md")).unwrap(),
            "user skill"
        );
        assert!(!codex_home.join(LIVE_MANIFEST).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_sync_takes_over_legacy_deployment_backup() {
        let root = std::env::temp_dir().join(format!(
            "super-instruct-legacy-skill-test-{}",
            uuid::Uuid::new_v4()
        ));
        let src = root.join("source");
        let codex_home = root.join("codex");
        let id = "novel-agent";
        fs::create_dir_all(src.join(id)).unwrap();
        fs::create_dir_all(codex_home.join("skills").join(id)).unwrap();
        fs::create_dir_all(codex_home.join(SKILLS_BACKUP_DIR).join(id)).unwrap();
        fs::write(src.join(id).join("SKILL.md"), "new project skill").unwrap();
        fs::write(
            codex_home.join("skills").join(id).join("SKILL.md"),
            "old deployed skill",
        )
        .unwrap();
        fs::write(
            codex_home.join(SKILLS_BACKUP_DIR).join(id).join("SKILL.md"),
            "user original skill",
        )
        .unwrap();
        write_manifest(
            &codex_home.join(SKILLS_MANIFEST),
            &BTreeSet::from([id.to_string()]),
        )
        .unwrap();

        let enabled = BTreeSet::from([id.to_string()]);
        sync_enabled_skills_inner(&src, &codex_home, &enabled, &BTreeSet::new()).unwrap();
        assert_eq!(
            fs::read_to_string(codex_home.join("skills").join(id).join("SKILL.md")).unwrap(),
            "new project skill"
        );
        assert_eq!(
            fs::read_to_string(codex_home.join(LIVE_BACKUP_DIR).join(id).join("SKILL.md")).unwrap(),
            "user original skill"
        );
        assert!(!codex_home.join(SKILLS_MANIFEST).exists());

        sync_enabled_skills_inner(&src, &codex_home, &BTreeSet::new(), &enabled).unwrap();
        assert_eq!(
            fs::read_to_string(codex_home.join("skills").join(id).join("SKILL.md")).unwrap(),
            "user original skill"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
