//! Prompt iteration evidence, A/B/C release gates, and production eligibility.

use crate::instruction;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const RELEASE_CATALOG_JSON: &str = include_str!("../../instruction-lab/release-catalog.json");
const ISSUE_BANK_JSONL: &str = include_str!("../../instruction-lab/banks/issue-regression.jsonl");
const PROMPT_BANK_JSONL: &str = include_str!("../../instruction-lab/banks/prompt-medium.jsonl");
const LAB_DIR: &str = "instruction-lab";
const FEEDBACK_FILE: &str = "feedback.json";

#[derive(Clone, Debug, Deserialize)]
struct ReleaseCatalog {
    schema_version: u32,
    source: CatalogSource,
    banks: BankCatalog,
    methods: MethodCatalog,
    gates: GateCatalog,
    releases: Vec<ReleaseRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogSource {
    repository: String,
    commit: String,
    license: String,
}

#[derive(Clone, Debug, Deserialize)]
struct BankCatalog {
    issue: BankDefinition,
    prompt: BankDefinition,
}

#[derive(Clone, Debug, Deserialize)]
struct BankDefinition {
    id: String,
    quality_contract: String,
    path: String,
    sha256: String,
    cases: usize,
    turns: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct MethodCatalog {
    issue: MethodDefinition,
    prompt: MethodDefinition,
}

#[derive(Clone, Debug, Deserialize)]
struct MethodDefinition {
    id: String,
    transport: String,
    runner_sha256: String,
    scorer_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GateCatalog {
    #[serde(rename = "A")]
    a: GateDefinition,
    #[serde(rename = "B")]
    b: GateDefinition,
    #[serde(rename = "C")]
    c: GateDefinition,
}

#[derive(Clone, Debug, Deserialize)]
struct GateDefinition {
    name: String,
    cases: usize,
    turns: usize,
    required_cases: usize,
    required_turns: usize,
    artifacts: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct ReleaseRecord {
    profile_id: String,
    model: String,
    reasoning: String,
    source_sha256: String,
    source_bytes: usize,
    max_bytes: usize,
    release_status: String,
    release_decision: String,
    production_deployable: bool,
    evidence: EvidenceSet,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageEvidence {
    pub cases_passed: usize,
    pub cases_total: usize,
    pub turns_passed: usize,
    pub turns_total: usize,
    pub artifacts_passed: usize,
    pub artifacts_total: usize,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceSet {
    #[serde(rename = "A")]
    pub a: StageEvidence,
    #[serde(rename = "B")]
    pub b: StageEvidence,
    #[serde(rename = "C")]
    pub c: StageEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedEvidence {
    pub schema_version: u32,
    pub profile_id: String,
    pub model: String,
    pub reasoning: String,
    pub prompt_sha256: String,
    pub issue_bank_sha256: String,
    pub prompt_bank_sha256: String,
    pub issue_method: String,
    pub issue_transport: String,
    pub issue_runner_sha256: String,
    pub issue_scorer_sha256: String,
    pub prompt_method: String,
    pub prompt_transport: String,
    pub prompt_runner_sha256: String,
    pub prompt_scorer_sha256: String,
    pub run_id: String,
    pub generated_at: String,
    pub evidence: EvidenceSet,
}

#[derive(Clone, Debug, Serialize)]
pub struct PipelineStage {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct BankStatus {
    pub id: String,
    pub quality_contract: String,
    pub path: String,
    pub sha256: String,
    pub expected_sha256: String,
    pub cases: usize,
    pub turns: usize,
    pub languages: BTreeMap<String, usize>,
    pub families: BTreeMap<String, usize>,
    pub integrity_ok: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageStatus {
    pub id: &'static str,
    pub name: String,
    pub evidence: StageEvidence,
    pub required_cases: usize,
    pub required_turns: usize,
    pub required_artifacts: usize,
    pub scope_matches: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StructuralCheck {
    pub id: &'static str,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct GateReport {
    pub profile_id: String,
    pub profile_name: String,
    pub model: String,
    pub reasoning: String,
    pub release_status: String,
    pub release_decision: String,
    pub evidence_origin: String,
    pub source_sha256: String,
    pub expected_source_sha256: String,
    pub source_bytes: usize,
    pub max_bytes: usize,
    pub source_integrity: bool,
    pub byte_budget_ok: bool,
    pub structural_checks: Vec<StructuralCheck>,
    pub structural_passed: usize,
    pub structural_total: usize,
    pub bank_integrity: bool,
    pub a: StageStatus,
    pub b: StageStatus,
    pub c: StageStatus,
    pub hard_gate_complete: bool,
    pub production_deployable: bool,
    pub feedback_count: usize,
    pub issues: Vec<String>,
    pub generated_at: String,
    pub report_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LabSnapshot {
    pub schema_version: u32,
    pub source_repository: String,
    pub source_commit: String,
    pub source_license: String,
    pub pipeline: Vec<PipelineStage>,
    pub issue_bank: BankStatus,
    pub prompt_bank: BankStatus,
    pub feedback_count: usize,
    pub releases: Vec<GateReport>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeedbackRecord {
    pub id: String,
    pub profile_id: String,
    pub family: String,
    pub language: String,
    pub level: String,
    pub summary: String,
    pub source: String,
    pub created_at: String,
}

#[derive(Default, Serialize, Deserialize)]
struct FeedbackStore {
    #[serde(default = "schema_version")]
    schema_version: u32,
    #[serde(default)]
    records: Vec<FeedbackRecord>,
}

fn schema_version() -> u32 {
    1
}

fn catalog() -> Result<ReleaseCatalog, String> {
    serde_json::from_str(RELEASE_CATALOG_JSON)
        .map_err(|error| format!("解析指令实验室发布清单失败: {error}"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn lab_dir(home: &Path) -> PathBuf {
    home.join(LAB_DIR)
}

fn feedback_path(home: &Path) -> PathBuf {
    lab_dir(home).join(FEEDBACK_FILE)
}

fn evidence_path(home: &Path, profile_id: &str) -> PathBuf {
    lab_dir(home)
        .join("evidence")
        .join(format!("{profile_id}.json"))
}

fn report_path(home: &Path, profile_id: &str) -> PathBuf {
    lab_dir(home)
        .join("reports")
        .join(format!("{profile_id}-gate.json"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("目标路径缺少父目录")?;
    fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
    let pending = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("state"),
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&pending, bytes).map_err(|error| format!("写入临时文件失败: {error}"))?;
    fs::rename(&pending, path)
        .or_else(|_| {
            fs::copy(&pending, path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&pending))
        })
        .map_err(|error| format!("发布文件失败: {error}"))
}

fn read_feedback(home: &Path) -> Result<FeedbackStore, String> {
    let path = feedback_path(home);
    if !path.exists() {
        return Ok(FeedbackStore {
            schema_version: schema_version(),
            records: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取失败样例失败: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("解析失败样例失败: {error}"))
}

fn bank_status(
    definition: &BankDefinition,
    raw: &str,
    family_field: &str,
) -> Result<BankStatus, String> {
    let mut cases = 0;
    let mut turns = 0;
    let mut languages = BTreeMap::new();
    let mut families = BTreeMap::new();
    for (index, line) in raw.lines().enumerate() {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| format!("测试库第 {} 行格式错误: {error}", index + 1))?;
        cases += 1;
        let turn_count = value
            .get("turns")
            .and_then(|item| item.as_array())
            .map(|items| items.len())
            .unwrap_or(1);
        turns += turn_count;
        let language = value
            .get("language")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown")
            .to_string();
        *languages.entry(language).or_insert(0) += 1;
        let family = value
            .get(family_field)
            .and_then(|item| item.as_str())
            .unwrap_or("unknown")
            .to_string();
        *families.entry(family).or_insert(0) += 1;
    }
    let sha256 = sha256_bytes(raw.as_bytes());
    Ok(BankStatus {
        id: definition.id.clone(),
        quality_contract: definition.quality_contract.clone(),
        path: definition.path.clone(),
        expected_sha256: definition.sha256.clone(),
        integrity_ok: sha256 == definition.sha256
            && cases == definition.cases
            && turns == definition.turns,
        sha256,
        cases,
        turns,
        languages,
        families,
    })
}

fn structural_checks(prompt: &str) -> Vec<StructuralCheck> {
    let folded = prompt.to_ascii_lowercase();
    vec![
        StructuralCheck {
            id: "visible_progress",
            passed: (folded.contains("current:") || prompt.contains("当前："))
                && (folded.contains("next") || prompt.contains("下一步")),
        },
        StructuralCheck {
            id: "state_reuse",
            passed: folded.contains("reuse") && folded.contains("files"),
        },
        StructuralCheck {
            id: "wrong_route_recovery",
            passed: folded.contains("wrong route") && folded.contains("resume"),
        },
        StructuralCheck {
            id: "non_rigid_templates",
            passed: folded.contains("tailor"),
        },
        StructuralCheck {
            id: "biology",
            passed: folded.contains("biological research")
                && folded.contains("assay")
                && folded.contains("measurement"),
        },
        StructuralCheck {
            id: "plaintext",
            passed: folded.contains("do not encode"),
        },
        StructuralCheck {
            id: "artifacts",
            passed: folded.contains("modified_file")
                && folded.contains("verification")
                && folded.contains("rollback"),
        },
    ]
}

fn stage_status(
    id: &'static str,
    definition: &GateDefinition,
    evidence: &StageEvidence,
) -> StageStatus {
    let scope_matches =
        evidence.cases_total == definition.cases && evidence.turns_total == definition.turns;
    let artifacts_ok = definition.artifacts == 0
        || (evidence.artifacts_total == definition.artifacts
            && evidence.artifacts_passed >= definition.artifacts);
    let passed = scope_matches
        && evidence.cases_passed >= definition.required_cases
        && evidence.turns_passed >= definition.required_turns
        && artifacts_ok
        && evidence.status == "pass";
    StageStatus {
        id,
        name: definition.name.clone(),
        evidence: evidence.clone(),
        required_cases: definition.required_cases,
        required_turns: definition.required_turns,
        required_artifacts: definition.artifacts,
        scope_matches,
        passed,
    }
}

fn validate_stage(name: &str, stage: &StageEvidence, gate: &GateDefinition) -> Result<(), String> {
    if stage.cases_total != gate.cases || stage.turns_total != gate.turns {
        return Err(format!(
            "{name} 证据范围不匹配: cases {}/{}, turns {}/{}",
            stage.cases_total, gate.cases, stage.turns_total, gate.turns
        ));
    }
    if stage.cases_passed > stage.cases_total
        || stage.turns_passed > stage.turns_total
        || stage.artifacts_passed > stage.artifacts_total
    {
        return Err(format!("{name} 证据通过数超过总数"));
    }
    if gate.artifacts > 0 && stage.artifacts_total != gate.artifacts {
        return Err(format!(
            "{name} 工件范围不匹配: {}/{}",
            stage.artifacts_total, gate.artifacts
        ));
    }
    if !["pass", "partial", "not_run"].contains(&stage.status.as_str()) {
        return Err(format!("{name} 证据状态无效: {}", stage.status));
    }
    if stage.status == "not_run"
        && (stage.cases_passed > 0 || stage.turns_passed > 0 || stage.artifacts_passed > 0)
    {
        return Err(format!("{name} 未运行状态包含通过计数"));
    }
    let artifacts_ok = gate.artifacts == 0
        || (stage.artifacts_total == gate.artifacts && stage.artifacts_passed >= gate.artifacts);
    let threshold_passed = stage.cases_passed >= gate.required_cases
        && stage.turns_passed >= gate.required_turns
        && artifacts_ok;
    if (stage.status == "pass") != threshold_passed {
        return Err(format!("{name} 证据状态与门槛计数不一致"));
    }
    Ok(())
}

fn validate_imported(
    imported: &ImportedEvidence,
    release: &ReleaseRecord,
    catalog: &ReleaseCatalog,
) -> Result<(), String> {
    if imported.schema_version != catalog.schema_version {
        return Err("证据 schema_version 与发布清单不一致".to_string());
    }
    if imported.profile_id != release.profile_id
        || imported.model != release.model
        || imported.reasoning != release.reasoning
    {
        return Err("证据的 profile/model/reasoning 方法身份不匹配".to_string());
    }
    if imported.prompt_sha256 != release.source_sha256
        || imported.issue_bank_sha256 != catalog.banks.issue.sha256
        || imported.prompt_bank_sha256 != catalog.banks.prompt.sha256
    {
        return Err("证据的提示词或测试库 SHA-256 不匹配".to_string());
    }
    let issue_method = &catalog.methods.issue;
    let prompt_method = &catalog.methods.prompt;
    if imported.issue_method != issue_method.id
        || imported.issue_transport != issue_method.transport
        || imported.issue_runner_sha256 != issue_method.runner_sha256
        || imported.issue_scorer_sha256 != issue_method.scorer_sha256
        || imported.prompt_method != prompt_method.id
        || imported.prompt_transport != prompt_method.transport
        || imported.prompt_runner_sha256 != prompt_method.runner_sha256
        || imported.prompt_scorer_sha256 != prompt_method.scorer_sha256
    {
        return Err("证据的 runner/scorer/transport 方法身份不匹配".to_string());
    }
    if imported.run_id.trim().is_empty()
        || imported.run_id == "replace-with-run-id"
        || chrono::DateTime::parse_from_rfc3339(&imported.generated_at).is_err()
    {
        return Err("证据缺少 run_id 或 generated_at".to_string());
    }
    validate_stage("A", &imported.evidence.a, &catalog.gates.a)?;
    validate_stage("B", &imported.evidence.b, &catalog.gates.b)?;
    validate_stage("C", &imported.evidence.c, &catalog.gates.c)
}

fn effective_evidence(
    home: Option<&Path>,
    release: &ReleaseRecord,
    catalog: &ReleaseCatalog,
) -> Result<(EvidenceSet, String), String> {
    let Some(home) = home else {
        return Ok((release.evidence.clone(), "bundled-release".to_string()));
    };
    let path = evidence_path(home, &release.profile_id);
    if !path.exists() {
        return Ok((release.evidence.clone(), "bundled-release".to_string()));
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取本地证据失败: {error}"))?;
    let imported: ImportedEvidence =
        serde_json::from_slice(&bytes).map_err(|error| format!("解析本地证据失败: {error}"))?;
    validate_imported(&imported, release, catalog)?;
    Ok((imported.evidence, "local-import".to_string()))
}

fn release_record<'a>(
    catalog: &'a ReleaseCatalog,
    profile_id: &str,
) -> Result<&'a ReleaseRecord, String> {
    catalog
        .releases
        .iter()
        .find(|item| item.profile_id == profile_id)
        .ok_or_else(|| format!("模型指令没有发布门禁记录: {profile_id}"))
}

fn build_report(home: Option<&Path>, profile_id: &str) -> Result<GateReport, String> {
    let catalog = catalog()?;
    let release = release_record(&catalog, profile_id)?;
    let profile =
        instruction::profile(profile_id).ok_or_else(|| format!("未知模型指令: {profile_id}"))?;
    let prompt = instruction::prompt_source(profile_id)
        .ok_or_else(|| format!("缺少指令源: {profile_id}"))?;
    let source_sha256 = sha256_bytes(prompt.as_bytes());
    let issue_bank = bank_status(&catalog.banks.issue, ISSUE_BANK_JSONL, "family")?;
    let prompt_bank = bank_status(&catalog.banks.prompt, PROMPT_BANK_JSONL, "scenario")?;
    let checks = structural_checks(prompt);
    let structural_passed = checks.iter().filter(|item| item.passed).count();
    let structural_total = checks.len();
    let source_integrity =
        source_sha256 == release.source_sha256 && prompt.len() == release.source_bytes;
    let byte_budget_ok = prompt.len() <= release.max_bytes;
    let bank_integrity = issue_bank.integrity_ok
        && prompt_bank.integrity_ok
        && issue_bank.languages.get("zh") == issue_bank.languages.get("en")
        && prompt_bank.languages.get("zh") == prompt_bank.languages.get("en");
    let (evidence, evidence_origin) = effective_evidence(home, release, &catalog)?;
    let a = stage_status("A", &catalog.gates.a, &evidence.a);
    let b = stage_status("B", &catalog.gates.b, &evidence.b);
    let c = stage_status("C", &catalog.gates.c, &evidence.c);
    let hard_gate_complete = a.passed && b.passed && c.passed;
    let local_integrity = source_integrity
        && byte_budget_ok
        && bank_integrity
        && structural_passed == structural_total;
    let production_deployable = release.production_deployable && local_integrity;
    let feedback_count = home
        .map(read_feedback)
        .transpose()?
        .map(|store| {
            store
                .records
                .iter()
                .filter(|item| item.profile_id == profile_id)
                .count()
        })
        .unwrap_or(0);
    let mut issues = Vec::new();
    if !source_integrity {
        issues.push("source_integrity_failed".to_string());
    }
    if !byte_budget_ok {
        issues.push("prompt_byte_budget_exceeded".to_string());
    }
    if !bank_integrity {
        issues.push("bank_integrity_failed".to_string());
    }
    for check in &checks {
        if !check.passed {
            issues.push(format!("missing_prompt_family:{}", check.id));
        }
    }
    if !a.passed {
        issues.push("gate_A_incomplete".to_string());
    }
    if !b.passed {
        issues.push("gate_B_incomplete".to_string());
    }
    if !c.passed {
        issues.push("gate_C_incomplete".to_string());
    }
    Ok(GateReport {
        profile_id: profile_id.to_string(),
        profile_name: profile.name.to_string(),
        model: release.model.clone(),
        reasoning: release.reasoning.clone(),
        release_status: release.release_status.clone(),
        release_decision: release.release_decision.clone(),
        evidence_origin,
        source_sha256,
        expected_source_sha256: release.source_sha256.clone(),
        source_bytes: prompt.len(),
        max_bytes: release.max_bytes,
        source_integrity,
        byte_budget_ok,
        structural_checks: checks,
        structural_passed,
        structural_total,
        bank_integrity,
        a,
        b,
        c,
        hard_gate_complete,
        production_deployable,
        feedback_count,
        issues,
        generated_at: Utc::now().to_rfc3339(),
        report_path: None,
    })
}

pub fn ensure_deployable(profile_id: &str) -> Result<(), String> {
    if crate::upstream::prompt(profile_id).is_some() {
        return crate::upstream::verify(profile_id);
    }
    if instruction::prompt_source(profile_id).is_none() {
        return Ok(());
    }
    let report = build_report(None, profile_id)?;
    if report.production_deployable {
        Ok(())
    } else {
        Err(format!(
            "模型指令 {} 未通过生产完整性门禁: {}",
            profile_id,
            report.issues.join(", ")
        ))
    }
}

pub fn snapshot(home: &Path) -> Result<LabSnapshot, String> {
    let catalog = catalog()?;
    let issue_bank = bank_status(&catalog.banks.issue, ISSUE_BANK_JSONL, "family")?;
    let prompt_bank = bank_status(&catalog.banks.prompt, PROMPT_BANK_JSONL, "scenario")?;
    let feedback_count = read_feedback(home)?.records.len();
    let releases = catalog
        .releases
        .iter()
        .map(|release| build_report(Some(home), &release.profile_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LabSnapshot {
        schema_version: catalog.schema_version,
        source_repository: catalog.source.repository,
        source_commit: catalog.source.commit,
        source_license: catalog.source.license,
        pipeline: vec![
            PipelineStage {
                id: "iteration",
                name: "持续迭代",
                summary: "失败样例回流、双语归因、LOW/MEDIUM/HIGH 分层",
                status: "active",
            },
            PipelineStage {
                id: "release-gate",
                name: "发布门禁",
                summary: "A 用户反馈、B Issue 回归、C 120-case 基准",
                status: "active",
            },
            PipelineStage {
                id: "runtime",
                name: "生产运行",
                summary: "Rust 事务部署、MITM 注入、监控与恢复",
                status: "active",
            },
        ],
        issue_bank,
        prompt_bank,
        feedback_count,
        releases,
    })
}

pub fn run_gate(home: &Path, profile_id: &str) -> Result<GateReport, String> {
    let mut report = build_report(Some(home), profile_id)?;
    let path = report_path(home, profile_id);
    report.report_path = Some(path.display().to_string());
    let payload = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("序列化门禁报告失败: {error}"))?;
    atomic_write(&path, &payload)?;
    Ok(report)
}

pub fn record_feedback(
    home: &Path,
    profile_id: &str,
    family: &str,
    language: &str,
    level: &str,
    summary: &str,
) -> Result<FeedbackRecord, String> {
    let catalog = catalog()?;
    release_record(&catalog, profile_id)?;
    const FAMILIES: &[&str] = &[
        "execution_completion",
        "routing_continuity",
        "fiction_feedback",
        "progress_visibility",
        "biology_research",
        "cloud_plaintext_reverse",
    ];
    if !FAMILIES.contains(&family) {
        return Err(format!("未知失败分类: {family}"));
    }
    if !["zh", "en"].contains(&language) {
        return Err("language 仅支持 zh 或 en".to_string());
    }
    if !["low", "medium", "high"].contains(&level) {
        return Err("level 仅支持 low、medium 或 high".to_string());
    }
    let summary = summary.trim();
    if summary.is_empty() || summary.chars().count() > 2000 {
        return Err("失败摘要长度应为 1-2000 字符".to_string());
    }
    let record = FeedbackRecord {
        id: uuid::Uuid::new_v4().simple().to_string(),
        profile_id: profile_id.to_string(),
        family: family.to_string(),
        language: language.to_string(),
        level: level.to_string(),
        summary: summary.to_string(),
        source: "local-user-feedback".to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let mut store = read_feedback(home)?;
    store.schema_version = schema_version();
    store.records.push(record.clone());
    let payload = serde_json::to_vec_pretty(&store)
        .map_err(|error| format!("序列化失败样例失败: {error}"))?;
    atomic_write(&feedback_path(home), &payload)?;
    Ok(record)
}

pub fn import_evidence(home: &Path, source: &Path) -> Result<GateReport, String> {
    let bytes = fs::read(source).map_err(|error| format!("读取证据文件失败: {error}"))?;
    let imported: ImportedEvidence =
        serde_json::from_slice(&bytes).map_err(|error| format!("解析证据文件失败: {error}"))?;
    let catalog = catalog()?;
    let release = release_record(&catalog, &imported.profile_id)?;
    validate_imported(&imported, release, &catalog)?;
    let prompt = instruction::prompt_source(&imported.profile_id)
        .ok_or_else(|| format!("缺少指令源: {}", imported.profile_id))?;
    if sha256_bytes(prompt.as_bytes()) != imported.prompt_sha256 {
        return Err("当前指令源与证据 prompt_sha256 不一致".to_string());
    }
    let destination = evidence_path(home, &imported.profile_id);
    let payload =
        serde_json::to_vec_pretty(&imported).map_err(|error| format!("序列化证据失败: {error}"))?;
    atomic_write(&destination, &payload)?;
    run_gate(home, &imported.profile_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home() -> PathBuf {
        std::env::temp_dir().join(format!(
            "julong-instruction-lab-{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn bundled_banks_have_expected_scope_and_language_balance() {
        let catalog = catalog().unwrap();
        let issue = bank_status(&catalog.banks.issue, ISSUE_BANK_JSONL, "family").unwrap();
        let prompt = bank_status(&catalog.banks.prompt, PROMPT_BANK_JSONL, "scenario").unwrap();
        assert!(issue.integrity_ok);
        assert_eq!((issue.cases, issue.turns), (66, 74));
        assert_eq!(issue.languages.get("zh"), Some(&33));
        assert_eq!(issue.languages.get("en"), Some(&33));
        assert!(prompt.integrity_ok);
        assert_eq!((prompt.cases, prompt.turns), (120, 120));
        assert_eq!(prompt.languages.get("zh"), Some(&60));
        assert_eq!(prompt.languages.get("en"), Some(&60));
    }

    #[test]
    fn release_status_does_not_invent_hard_gate_completion() {
        let astra = build_report(None, instruction::GPT6_ASTRA_PROFILE).unwrap();
        assert!(astra.source_integrity);
        assert!(astra.bank_integrity);
        assert_eq!(astra.structural_passed, astra.structural_total);
        assert!(astra.a.passed);
        assert!(!astra.b.passed);
        assert!(!astra.c.passed);
        assert!(!astra.hard_gate_complete);
        assert!(astra.production_deployable);
        assert!(ensure_deployable(instruction::GPT6_ASTRA_PROFILE).is_ok());
    }

    #[test]
    fn feedback_and_full_evidence_close_the_local_loop() {
        let home = test_home();
        let record = record_feedback(
            &home,
            instruction::GPT6_ASTRA_PROFILE,
            "routing_continuity",
            "zh",
            "medium",
            "续作时重新解释了任务。",
        )
        .unwrap();
        assert_eq!(record.family, "routing_continuity");
        let snapshot = snapshot(&home).unwrap();
        assert_eq!(snapshot.feedback_count, 1);

        let mut template: ImportedEvidence =
            serde_json::from_str(include_str!("../../instruction-lab/evidence-template.json"))
                .unwrap();
        template.run_id = "test-full-gate".to_string();
        template.generated_at = Utc::now().to_rfc3339();
        template.evidence.a.cases_passed = 3;
        template.evidence.a.turns_passed = 3;
        template.evidence.a.artifacts_passed = 2;
        template.evidence.a.status = "pass".to_string();
        template.evidence.b.cases_passed = 66;
        template.evidence.b.turns_passed = 74;
        template.evidence.b.artifacts_passed = 16;
        template.evidence.b.status = "pass".to_string();
        template.evidence.c.cases_passed = 120;
        template.evidence.c.turns_passed = 120;
        template.evidence.c.status = "pass".to_string();
        let input = home.join("input-evidence.json");
        fs::create_dir_all(&home).unwrap();
        fs::write(&input, serde_json::to_vec_pretty(&template).unwrap()).unwrap();
        let report = import_evidence(&home, &input).unwrap();
        assert_eq!(report.evidence_origin, "local-import");
        assert!(report.hard_gate_complete);
        assert!(report.production_deployable);
        assert!(Path::new(report.report_path.as_deref().unwrap()).exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn imported_evidence_rejects_status_count_conflicts() {
        let home = test_home();
        let mut template: ImportedEvidence =
            serde_json::from_str(include_str!("../../instruction-lab/evidence-template.json"))
                .unwrap();
        template.run_id = "test-conflicting-status".to_string();
        template.generated_at = Utc::now().to_rfc3339();
        template.evidence.a.cases_passed = 3;
        template.evidence.a.turns_passed = 3;
        template.evidence.a.artifacts_passed = 2;
        template.evidence.a.status = "not_run".to_string();
        let input = home.join("conflicting-evidence.json");
        fs::create_dir_all(&home).unwrap();
        fs::write(&input, serde_json::to_vec_pretty(&template).unwrap()).unwrap();
        let error = import_evidence(&home, &input).unwrap_err();
        assert!(error.contains("未运行状态包含通过计数"));
        let _ = fs::remove_dir_all(home);
    }
}
