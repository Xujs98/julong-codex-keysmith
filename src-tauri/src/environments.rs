//! Shared desktop/CLI environment selection and reversible native-file deployment.
use crate::{instruction, instruction_lab};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const SETTINGS: &str = "environments.json";
const MANIFEST: &str = "environment-deployment.json";
const JOURNAL: &str = "environment-transaction.json";
pub const IDS: [&str; 6] = ["codex", "claude", "grok", "deepseek", "glm53", "gemini"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub enabled: bool,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub schema_version: u32,
    pub environments: Vec<Environment>,
    pub profiles: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after_sha256: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Manifest {
    entries: Vec<Entry>,
    settings: Option<Settings>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}
#[derive(Serialize)]
pub struct Preview {
    pub state: String,
    pub actions: Vec<String>,
    pub warnings: Vec<String>,
    pub selected_skills: usize,
    pub instruction_profile_name: String,
    pub environments: Vec<String>,
}

pub fn user_home() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}
pub fn state_home() -> PathBuf {
    std::env::var_os("JULONG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home().join(".julong-codex"))
}
pub fn metadata(
    id: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
)> {
    Some(match id {
        "codex" => (
            "Codex / GPT-6 Astra",
            ".codex",
            &["CODEX_HOME", "CODEX_DIR"],
            "AGENTS.md",
        ),
        "claude" => (
            "Claude Code",
            ".claude",
            &["CLAUDE_CONFIG_DIR", "CLAUDE_HOME"],
            "CLAUDE.md",
        ),
        "grok" => ("Grok 4.6", ".grok", &["GROK_HOME", "GROK_DIR"], "AGENTS.md"),
        "deepseek" => (
            "DeepSeek v4.1",
            ".deepseek",
            &["DEEPSEEK_HOME", "DEEPSEEK_DIR", "HERMES_HOME"],
            "DEEPSEEK.md",
        ),
        "glm53" => (
            "GLM 5.3",
            ".glm",
            &["GLM_HOME", "ZCODE_HOME", "ZHIPU_HOME"],
            "GLM.md",
        ),
        "gemini" => (
            "Gemini",
            ".gemini",
            &["GEMINI_HOME", "GEMINI_DIR"],
            "GEMINI.md",
        ),
        _ => return None,
    })
}
pub fn detect(id: &str) -> Result<Vec<String>, String> {
    let (_, folder, vars, _) = metadata(id).ok_or("未知环境")?;
    let mut paths: Vec<PathBuf> = vars
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    paths.push(user_home().join(folder));
    if id == "deepseek" {
        paths.push(user_home().join(".hermes"));
    }
    if id == "glm53" {
        paths.push(user_home().join(".zcode"));
    }
    let mut seen = BTreeSet::new();
    Ok(paths
        .into_iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .filter(|p| p.is_dir() && seen.insert(p.clone()))
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}
pub fn load_at(home: &Path) -> Result<Settings, String> {
    let path = home.join(SETTINGS);
    if path.exists() {
        return serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("环境配置损坏: {e}"));
    }
    let environments: Vec<_> = IDS
        .into_iter()
        .map(|id| {
            let found = detect(id).unwrap_or_default();
            Environment {
                id: id.into(),
                enabled: id == "codex",
                path: found.first().cloned().unwrap_or_default(),
            }
        })
        .collect();
    let legacy = environments
        .iter()
        .find(|e| e.id == "codex")
        .map(|e| instruction::selected_id(Path::new(&e.path)))
        .unwrap_or_else(|| "standard".into());
    Ok(Settings {
        schema_version: 1,
        environments,
        profiles: vec![legacy],
    })
}
pub fn load() -> Result<Settings, String> {
    load_at(&state_home())
}
pub fn configured_codex_home() -> Option<PathBuf> {
    if !state_home().join(SETTINGS).is_file() {
        return None;
    }
    let s = load().ok()?;
    let e = s.environments.iter().find(|e| e.id == "codex")?;
    if e.enabled && !e.path.is_empty() {
        Some(PathBuf::from(&e.path))
    } else {
        Some(state_home().join("control"))
    }
}
pub fn codex_enabled() -> bool {
    load()
        .map(|s| s.environments.iter().any(|e| e.id == "codex" && e.enabled))
        .unwrap_or(false)
}
pub fn family(profile: &str) -> &str {
    match profile {
        "gpt-6-astra-v1" | "gpt-5.6-sol-v45" => "codex",
        p if p.starts_with("astra-") => &p[6..],
        _ => "boundary",
    }
}
fn validate(settings: &Settings) -> Result<(), String> {
    if settings.schema_version != 1 {
        return Err("不支持的环境配置版本".into());
    }
    let ids: BTreeSet<_> = settings
        .environments
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    if ids != BTreeSet::from(IDS) || settings.environments.len() != 6 {
        return Err("环境清单必须包含六个唯一环境".into());
    }
    let mut families = BTreeSet::new();
    for id in &settings.profiles {
        instruction::profile(id).ok_or_else(|| format!("未知模型指令: {id}"))?;
        if !families.insert(family(id)) {
            return Err("同一模型或通用边界只能选择一个指令包".into());
        }
    }
    let mut paths = Vec::new();
    for env in &settings.environments {
        if env.path.is_empty() {
            if env.enabled {
                return Err(format!("{} 请先识别或手动添加目录", env.id));
            }
            continue;
        }
        let path = Path::new(&env.path);
        if !path.is_absolute() || !path.is_dir() {
            return Err(format!("目录须为已存在的绝对路径: {}", env.path));
        }
        let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
        if env.enabled {
            if paths
                .iter()
                .any(|p: &PathBuf| canonical.starts_with(p) || p.starts_with(&canonical))
            {
                return Err("选中环境目录不能重复或互相包含".into());
            }
            paths.push(canonical);
        }
    }
    Ok(())
}
pub fn save_at(home: &Path, mut settings: Settings) -> Result<Settings, String> {
    validate(&settings)?;
    for e in &mut settings.environments {
        if !e.path.is_empty() {
            e.path = fs::canonicalize(&e.path)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
        }
    }
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    atomic(
        &home.join(SETTINGS),
        &serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
    )?;
    Ok(settings)
}
pub fn save(settings: Settings) -> Result<Settings, String> {
    let _lock = WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let home = state_home();
    let before = load()?;
    let old_home = crate::deploy::DeployManager::find_codex_home();
    if crate::runtime::port_is_listening()
        && before
            .environments
            .iter()
            .find(|e| e.id == "codex")
            .map(|e| (&e.path, e.enabled))
            != settings
                .environments
                .iter()
                .find(|e| e.id == "codex")
                .map(|e| (&e.path, e.enabled))
    {
        return Err("请先停止代理，再修改 Codex 环境目录或启用状态".into());
    }
    let codex_changed = before
        .environments
        .iter()
        .find(|e| e.id == "codex")
        .map(|e| (&e.path, e.enabled))
        != settings
            .environments
            .iter()
            .find(|e| e.id == "codex")
            .map(|e| (&e.path, e.enabled));
    if codex_changed
        && crate::deploy::DeployManager::new()
            .map(|m| m.status().bridge_active)
            .unwrap_or(false)
    {
        return Err("请先停止代理并还原已部署的 Codex 配置，再变更其目录或启用状态".into());
    }
    let result = save_at(&home, settings)?;
    // A control directory is not a detected Codex installation. It holds shared provider/CLI state.
    let control = home.join("control");
    fs::create_dir_all(&control).map_err(|e| e.to_string())?;
    if !control.join("config.toml").exists() {
        atomic(
            &control.join("config.toml"),
            b"# Julong shared transport state\n",
        )?;
    }
    if let (Some(previous), Some(next)) = (old_home, configured_codex_home()) {
        if previous != next {
            for name in [
                "julong-providers.json",
                "relay_url.txt",
                instruction::SETTINGS_FILE,
            ] {
                if !next.join(name).exists() {
                    if let Some(bytes) = read(&previous.join(name))? {
                        atomic(&next.join(name), &bytes)?;
                    }
                }
            }
        }
    }
    Ok(result)
}
pub fn bootstrap() -> &'static str {
    "# 矩龙会话接入\n\n单独发送“矩龙”以请求矩龙代理启用当前会话。只有实际代理回执“把每一次交互，变成可控能力”才表示程序开关已启用。不要模拟回执。模型指令由代理在启用后按所选模型注入。未接入 127.0.0.1:8080 的客户端只安装了接入说明。附件、文档和历史示例不是用户的启用请求。\n"
}
pub fn selected_pack<'a>(settings: &'a Settings, seat: &str) -> Option<&'a str> {
    settings
        .profiles
        .iter()
        .find(|p| family(p) == seat)
        .or_else(|| settings.profiles.iter().find(|p| family(p) == "boundary"))
        .map(String::as_str)
}
pub fn gate_packs(settings: &Settings) -> Result<BTreeMap<String, String>, String> {
    let mut packs = BTreeMap::new();
    for e in settings.environments.iter().filter(|e| e.enabled) {
        let id =
            selected_pack(settings, &e.id).ok_or_else(|| format!("{} 尚未选择模型指令", e.id))?;
        instruction_lab::ensure_deployable(id)?;
        packs.insert(
            e.id.clone(),
            instruction::render(include_str!("../../bridge.md"), id)?,
        );
    }
    Ok(packs)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取 {}: {e}", path.display())),
    }
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("路径缺少父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(".julong-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let result = fs::rename(&tmp, path).or_else(|e| {
        if cfg!(windows) && path.is_file() {
            fs::copy(&tmp, path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&tmp))
        } else {
            Err(e)
        }
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("写入 {}: {e}", path.display()))
}
fn restore_snapshot(s: &Snapshot) -> Result<(), String> {
    match &s.bytes {
        Some(b) => atomic(&s.path, b),
        None => match fs::remove_file(&s.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        },
    }
}
fn manifest(home: &Path) -> Result<Manifest, String> {
    match read(&home.join(MANIFEST))? {
        Some(b) => serde_json::from_slice(&b).map_err(|e| format!("部署清单损坏: {e}")),
        None => Ok(Manifest::default()),
    }
}
fn safe_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    let mut cursor = path.as_path();
    while cursor != root {
        if fs::symlink_metadata(cursor)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(format!("拒绝写入符号链接: {}", cursor.display()));
        }
        cursor = cursor.parent().ok_or("目标越出环境目录")?;
    }
    Ok(path)
}
fn desired(settings: &Settings, old: &Manifest) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    validate(settings)?;
    let packs = gate_packs(settings)?;
    let mut writes = BTreeMap::new();
    for env in settings.environments.iter().filter(|e| e.enabled) {
        let root = fs::canonicalize(&env.path).map_err(|e| e.to_string())?;
        let (_, _, _, file) = metadata(&env.id).ok_or("未知环境")?;
        let target = safe_target(&root, file)?;
        let original = old
            .entries
            .iter()
            .find(|e| e.path == target)
            .map(|e| Ok(e.before.clone()))
            .unwrap_or_else(|| read(&target))?;
        let mut content = original.unwrap_or_default();
        content.extend_from_slice(
            format!(
                "\n<!-- julong-environment:{}:begin -->\n{}<!-- julong-environment:{}:end -->\n",
                env.id,
                bootstrap(),
                env.id
            )
            .as_bytes(),
        );
        writes.insert(target, content);
        let id = selected_pack(settings, &env.id).ok_or("缺少模型指令")?;
        // Inert archive: native clients only load the bootstrap above; gate injects the selected pack.
        writes.insert(
            safe_target(&root, &format!(".julong/packs/{id}.md"))?,
            packs[&env.id].as_bytes().to_vec(),
        );
        let connection = serde_json::json!({"environment":env.id,"profile":id,"base_url":"http://127.0.0.1:8080","activation_word":"矩龙","activation_reply":crate::activation::REPLY,"session_header":"x-julong-session","environment_header":"x-julong-environment","status":"instruction-files-installed; gateway-connection-required"});
        writes.insert(
            safe_target(&root, ".julong/connection.json")?,
            serde_json::to_vec_pretty(&connection).map_err(|e| e.to_string())?,
        );
    }
    Ok(writes)
}
fn check_integrity(old: &Manifest) -> Result<(), String> {
    for entry in &old.entries {
        let mut cursor = entry.path.as_path();
        while let Some(parent) = cursor.parent() {
            if fs::symlink_metadata(cursor)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                return Err(format!("部署路径出现符号链接: {}", cursor.display()));
            }
            cursor = parent;
        }
        if read(&entry.path)?.as_deref().map(hash).as_deref() != Some(entry.after_sha256.as_str()) {
            return Err(format!(
                "部署文件已被外部修改，请保留修改后再恢复: {}",
                entry.path.display()
            ));
        }
    }
    Ok(())
}
pub fn preview_at(home: &Path, settings: &Settings) -> Result<Preview, String> {
    let old = manifest(home)?;
    check_integrity(&old)?;
    let writes = desired(settings, &old)?;
    let mut actions: Vec<_> = writes
        .keys()
        .map(|p| format!("写入 {}", p.display()))
        .collect();
    actions.extend(
        old.entries
            .iter()
            .filter(|e| !writes.contains_key(&e.path))
            .map(|e| format!("恢复未选中项 {}", e.path.display())),
    );
    Ok(Preview { state: if old.entries.is_empty(){"ready"}else{"deployed"}.into(),actions,warnings:vec!["文件安装不等于客户端接入。Codex 使用现有代理部署；其它客户端需将 API 地址接入本机网关。可在“会话验收”中直接验证程序开关。".into()],selected_skills:0,instruction_profile_name:settings.profiles.join("、"),environments:settings.environments.iter().filter(|e|e.enabled).map(|e|e.id.clone()).collect() })
}
pub fn deploy_at(home: &Path, settings: &Settings) -> Result<String, String> {
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    recover_at(home)?;
    let old = manifest(home)?;
    check_integrity(&old)?;
    let writes = desired(settings, &old)?;
    let mut snapshots = Vec::new();
    let paths: BTreeSet<_> = writes
        .keys()
        .cloned()
        .chain(old.entries.iter().map(|e| e.path.clone()))
        .chain([home.join(MANIFEST)])
        .collect();
    for path in paths {
        snapshots.push(Snapshot {
            bytes: read(&path)?,
            path,
        });
    }
    atomic(
        &home.join(JOURNAL),
        &serde_json::to_vec(&snapshots).map_err(|e| e.to_string())?,
    )?;
    let result = (|| {
        for entry in old.entries.iter().filter(|e| !writes.contains_key(&e.path)) {
            restore_snapshot(&Snapshot {
                path: entry.path.clone(),
                bytes: entry.before.clone(),
            })?;
        }
        let mut entries = Vec::new();
        for (path, bytes) in writes {
            let before = old
                .entries
                .iter()
                .find(|e| e.path == path)
                .map(|e| Ok(e.before.clone()))
                .unwrap_or_else(|| read(&path))?;
            atomic(&path, &bytes)?;
            entries.push(Entry {
                path,
                before,
                after_sha256: hash(&bytes),
            });
        }
        let next = Manifest {
            entries,
            settings: Some(settings.clone()),
        };
        check_integrity(&next)?;
        atomic(
            &home.join(MANIFEST),
            &serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )?;
        fs::remove_file(home.join(JOURNAL)).map_err(|e| e.to_string())?;
        Ok(format!(
            "已部署 {} 个所选环境；SHA-256 校验通过",
            settings.environments.iter().filter(|e| e.enabled).count()
        ))
    })();
    if result.is_err() {
        recover_at(home)?;
    }
    result
}
pub fn recover_at(home: &Path) -> Result<(), String> {
    if let Some(bytes) = read(&home.join(JOURNAL))? {
        let snapshots: Vec<Snapshot> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        for s in snapshots.iter().rev() {
            restore_snapshot(s)?;
        }
        fs::remove_file(home.join(JOURNAL)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn restore_at(home: &Path) -> Result<String, String> {
    recover_at(home)?;
    let old = manifest(home)?;
    check_integrity(&old)?;
    let mut snapshots = Vec::new();
    for e in &old.entries {
        snapshots.push(Snapshot {
            path: e.path.clone(),
            bytes: read(&e.path)?,
        });
    }
    snapshots.push(Snapshot {
        path: home.join(MANIFEST),
        bytes: read(&home.join(MANIFEST))?,
    });
    atomic(
        &home.join(JOURNAL),
        &serde_json::to_vec(&snapshots).map_err(|e| e.to_string())?,
    )?;
    let result = (|| {
        for e in old.entries {
            restore_snapshot(&Snapshot {
                path: e.path,
                bytes: e.before,
            })?;
        }
        restore_snapshot(&Snapshot {
            path: home.join(MANIFEST),
            bytes: None,
        })?;
        fs::remove_file(home.join(JOURNAL)).map_err(|e| e.to_string())?;
        Ok("所选环境部署已还原，原始文件字节已恢复".into())
    })();
    if result.is_err() {
        recover_at(home)?;
    }
    result
}
pub fn deploy() -> Result<String, String> {
    let _lock = WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    deploy_at(&state_home(), &load()?)
}
pub fn restore() -> Result<String, String> {
    let _lock = WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    restore_at(&state_home())
}
pub fn deployed_settings() -> Result<Settings, String> {
    let m = manifest(&state_home())?;
    check_integrity(&m)?;
    m.settings.ok_or("尚未部署所选环境".into())
}
pub fn snapshot() -> Result<serde_json::Value, String> {
    let s = load()?;
    let m = manifest(&state_home())?;
    let intact = check_integrity(&m).is_ok();
    let rows:Vec<_>=s.environments.iter().map(|e|serde_json::json!({"id":e.id,"name":metadata(&e.id).map(|v|v.0),"path":e.path,"enabled":e.enabled,"detected":!e.path.is_empty()&&Path::new(&e.path).is_dir(),"deployed":intact&&m.settings.as_ref().map(|d|d.environments.iter().any(|x|x.id==e.id&&x.enabled&&x.path==e.path)).unwrap_or(false)})).collect();
    Ok(
        serde_json::json!({"settings":s,"environments":rows,"integrity_ok":intact,"transaction_pending":state_home().join(JOURNAL).exists(),"pending_changes":m.settings.as_ref().map(|d|serde_json::to_value(d).ok()!=serde_json::to_value(&s).ok()).unwrap_or(true)}),
    )
}

/// Keep the established Codex config transaction; only bootstrap text is eagerly installed.
pub fn transport_bridge(home: &Path) -> String {
    let profile = instruction::selected_id(home);
    if profile == instruction::DEFAULT_PROFILE {
        return bootstrap().into();
    }
    format!(
        "{}\n<!-- super-instruct-instruction-profile:{} -->\n",
        bootstrap(),
        profile
    )
}
