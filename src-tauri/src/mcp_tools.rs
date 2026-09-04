//! 配置驱动的 MCP 工具目录与多后端执行器。
//!
//! 该模块吸收了参考项目的工具目录、工具可用性检查和 Local/WSL/Docker/SSH
//! 后端设计，但所有参数均以 argv 传递，默认不启用任意 shell 命令工具。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BUILTIN_CATALOG: &str = include_str!("../../mcp-tools/tools.json");
pub const USER_CATALOG_FILE: &str = "julong-mcp-tools.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub schema_version: u32,
    #[serde(default)]
    pub defaults: ToolDefaults,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefaults {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_wsl_distro")]
    pub wsl_distro: String,
    #[serde(default = "default_docker_container")]
    pub docker_container: String,
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_output_limit")]
    pub output_limit_bytes: usize,
    #[serde(default)]
    pub allow_command_tools: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(alias = "desc")]
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub programs: Vec<String>,
    #[serde(default)]
    pub windows_programs: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    pub arguments: Vec<String>,
    #[serde(default)]
    pub windows_arguments: Vec<String>,
    #[serde(default)]
    pub expand_parameters: Vec<String>,
    #[serde(default)]
    pub command_tool: bool,
    #[serde(default)]
    pub parameters: Vec<ToolParameter>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolParameter {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolAvailability {
    pub name: String,
    pub category: String,
    pub description: String,
    pub backend: String,
    pub available: bool,
    pub executable: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackendStatus {
    pub requested: String,
    pub selected: String,
    pub ready: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolExecutionResult {
    pub tool: String,
    pub backend: String,
    pub executable: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub truncated: bool,
    pub duration_ms: u64,
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct ToolRunner {
    pub catalog: ToolCatalog,
    backend: Backend,
}

#[derive(Clone, Debug)]
enum Backend {
    Local,
    Wsl { distro: String },
    Docker { container: String },
    Ssh { host: String },
}

impl Default for ToolDefaults {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            wsl_distro: default_wsl_distro(),
            docker_container: default_docker_container(),
            ssh_host: String::new(),
            timeout_seconds: default_timeout(),
            output_limit_bytes: default_output_limit(),
            allow_command_tools: false,
        }
    }
}

impl ToolCatalog {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读取 MCP 工具目录失败 {}: {e}", path.display()))?;
        Self::from_str(&text)
    }

    pub fn from_str(text: &str) -> Result<Self, String> {
        let catalog: Self =
            serde_json::from_str(text).map_err(|e| format!("MCP 工具目录格式错误: {e}"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn builtin() -> Result<Self, String> {
        Self::from_str(BUILTIN_CATALOG)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version == 0 {
            return Err("MCP 工具目录 schema_version 必须大于 0".into());
        }
        let mut names = std::collections::BTreeSet::new();
        for tool in &self.tools {
            if tool.name.trim().is_empty() {
                return Err("MCP 工具名称不能为空".into());
            }
            if !tool
                .name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            {
                return Err(format!("MCP 工具名称包含非法字符: {}", tool.name));
            }
            if !names.insert(tool.name.clone()) {
                return Err(format!("MCP 工具名称重复: {}", tool.name));
            }
            if tool.programs.is_empty() && tool.windows_programs.is_empty() {
                return Err(format!("MCP 工具未定义可执行文件: {}", tool.name));
            }
            for program in tool.programs.iter().chain(&tool.windows_programs) {
                if !program.chars().all(|ch| {
                    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | '\\')
                }) {
                    return Err(format!(
                        "MCP 工具 {} 的可执行文件包含非法字符: {}",
                        tool.name, program
                    ));
                }
            }
            let known: std::collections::BTreeSet<&str> =
                tool.parameters.iter().map(|p| p.name.as_str()).collect();
            for param in &tool.expand_parameters {
                if !known.contains(param.as_str()) {
                    return Err(format!(
                        "MCP 工具 {} 的 expand_parameters 包含未知参数 {}",
                        tool.name, param
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    pub fn categories(&self) -> BTreeMap<String, usize> {
        let mut result = BTreeMap::new();
        for tool in &self.tools {
            *result.entry(tool.category.clone()).or_insert(0) += 1;
        }
        result
    }
}

impl ToolRunner {
    pub fn new(catalog: ToolCatalog, backend_override: Option<&str>) -> Result<Self, String> {
        let requested = backend_override.unwrap_or(&catalog.defaults.backend);
        let backend = resolve_backend(requested, &catalog.defaults)?;
        Ok(Self { catalog, backend })
    }

    pub fn backend_name(&self) -> String {
        self.backend.name()
    }

    pub fn backend_status(&self, requested: &str) -> BackendStatus {
        let (selected, ready, detail) = match &self.backend {
            Backend::Local => ("local".to_string(), true, "本机执行后端".to_string()),
            Backend::Wsl { distro } => {
                let ready = command_exists("wsl");
                ("wsl".to_string(), ready, format!("发行版: {distro}"))
            }
            Backend::Docker { container } => {
                let ready = command_exists("docker");
                ("docker".to_string(), ready, format!("容器: {container}"))
            }
            Backend::Ssh { host } => {
                let ready = !host.trim().is_empty() && command_exists("ssh");
                ("ssh".to_string(), ready, format!("主机: {host}"))
            }
        };
        BackendStatus {
            requested: requested.to_string(),
            selected,
            ready,
            detail,
        }
    }

    pub fn availability(&self) -> Vec<ToolAvailability> {
        let all_programs: std::collections::BTreeSet<String> = self
            .catalog
            .tools
            .iter()
            .flat_map(|tool| tool_programs(tool, &self.backend))
            .collect();
        let available_programs = self.backend.available_programs(&all_programs);
        self.catalog
            .tools
            .iter()
            .map(|tool| {
                if tool.command_tool && !self.catalog.defaults.allow_command_tools {
                    return ToolAvailability {
                        name: tool.name.clone(),
                        category: tool.category.clone(),
                        description: tool.description.clone(),
                        backend: self.backend_name(),
                        available: false,
                        executable: None,
                        reason: "高权限命令工具默认关闭".into(),
                    };
                }
                if !platform_allowed(tool, &self.backend) {
                    return ToolAvailability {
                        name: tool.name.clone(),
                        category: tool.category.clone(),
                        description: tool.description.clone(),
                        backend: self.backend_name(),
                        available: false,
                        executable: None,
                        reason: "当前平台不适用".into(),
                    };
                }
                let programs = tool_programs(tool, &self.backend);
                let executable = programs
                    .iter()
                    .find(|program| available_programs.contains(*program))
                    .cloned();
                let available = executable.is_some();
                ToolAvailability {
                    name: tool.name.clone(),
                    category: tool.category.clone(),
                    description: tool.description.clone(),
                    backend: self.backend_name(),
                    available,
                    executable,
                    reason: if available {
                        "可用".into()
                    } else {
                        format!("未找到: {}", programs.join(", "))
                    },
                }
            })
            .collect()
    }

    pub fn execute(
        &self,
        name: &str,
        args: &BTreeMap<String, String>,
    ) -> Result<ToolExecutionResult, String> {
        let tool = self
            .catalog
            .get(name)
            .ok_or_else(|| format!("未知 MCP 工具: {name}"))?;
        if tool.command_tool && !self.catalog.defaults.allow_command_tools {
            return Err(format!(
                "工具 {} 默认关闭，请在工具目录中显式启用",
                tool.name
            ));
        }
        validate_parameters(tool, args)?;
        let program = tool_programs(tool, &self.backend)
            .into_iter()
            .find(|candidate| self.backend.program_exists(candidate))
            .ok_or_else(|| format!("未找到工具可执行文件: {}", tool.programs.join(", ")))?;
        let argument_template = if cfg!(windows)
            && matches!(self.backend, Backend::Local)
            && !tool.windows_arguments.is_empty()
        {
            &tool.windows_arguments
        } else {
            &tool.arguments
        };
        let expanded = expand_arguments(argument_template, tool, args)?;
        let started = Instant::now();
        let process = self.run(&program, &expanded)?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let limit = self.catalog.defaults.output_limit_bytes.max(1024);
        let (output, truncated) = truncate_utf8(&process.text, limit);
        Ok(ToolExecutionResult {
            tool: tool.name.clone(),
            backend: self.backend_name(),
            executable: program,
            exit_code: process.exit_code,
            timed_out: process.timed_out,
            truncated: truncated || process.truncated,
            duration_ms,
            output,
        })
    }

    fn run(&self, program: &str, args: &[String]) -> Result<ProcessOutput, String> {
        let timeout = Duration::from_secs(self.catalog.defaults.timeout_seconds.clamp(1, 3600));
        let (command, command_args) = self.backend.command(program, args);
        let mut child = Command::new(command)
            .args(command_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动工具失败: {e}"))?;

        let stdout = child.stdout.take().ok_or("读取工具 stdout 失败")?;
        let stderr = child.stderr.take().ok_or("读取工具 stderr 失败")?;
        let output_limit = self.catalog.defaults.output_limit_bytes.max(1024);
        let stdout_thread = std::thread::spawn(move || read_limited(stdout, output_limit));
        let stderr_thread = std::thread::spawn(move || read_limited(stderr, output_limit));

        let mut timed_out = false;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| format!("等待工具失败: {e}"))?
            {
                break Some(status);
            }
            if started.elapsed() >= timeout {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(25));
        };

        let stdout = stdout_thread
            .join()
            .map_err(|_| "读取工具 stdout 线程失败")?;
        let stderr = stderr_thread
            .join()
            .map_err(|_| "读取工具 stderr 线程失败")?;
        let mut text = stdout.text;
        if !stderr.text.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&stderr.text);
        }
        Ok(ProcessOutput {
            exit_code: status.and_then(|s| s.code()),
            timed_out,
            truncated: stdout.truncated || stderr.truncated,
            text,
        })
    }
}

impl Backend {
    fn name(&self) -> String {
        match self {
            Backend::Local => "local".into(),
            Backend::Wsl { .. } => "wsl".into(),
            Backend::Docker { .. } => "docker".into(),
            Backend::Ssh { .. } => "ssh".into(),
        }
    }

    fn command(&self, program: &str, args: &[String]) -> (String, Vec<String>) {
        match self {
            Backend::Local => (program.to_string(), args.to_vec()),
            Backend::Wsl { distro } => {
                let mut out = vec!["-d".into(), distro.clone(), "--".into(), program.into()];
                out.extend(args.iter().cloned());
                ("wsl".into(), out)
            }
            Backend::Docker { container } => {
                let mut out = vec!["exec".into(), container.clone(), program.into()];
                out.extend(args.iter().cloned());
                ("docker".into(), out)
            }
            Backend::Ssh { host } => {
                let remote = std::iter::once(program)
                    .chain(args.iter().map(String::as_str))
                    .map(posix_quote)
                    .collect::<Vec<_>>()
                    .join(" ");
                let out = vec!["-o".into(), "BatchMode=yes".into(), host.clone(), remote];
                ("ssh".into(), out)
            }
        }
    }

    fn program_exists(&self, program: &str) -> bool {
        match self {
            Backend::Local => command_exists(program),
            Backend::Wsl { distro } => command_success(
                "wsl",
                &[
                    "-d",
                    distro,
                    "--",
                    "sh",
                    "-lc",
                    &format!("command -v {}", posix_quote(program)),
                ],
                Duration::from_secs(5),
            ),
            Backend::Docker { container } => command_success(
                "docker",
                &[
                    "exec",
                    container,
                    "sh",
                    "-lc",
                    &format!("command -v {}", posix_quote(program)),
                ],
                Duration::from_secs(5),
            ),
            Backend::Ssh { host } => {
                !host.trim().is_empty()
                    && command_success(
                        "ssh",
                        &[
                            "-o",
                            "BatchMode=yes",
                            "-o",
                            "ConnectTimeout=5",
                            host,
                            &format!("command -v {}", posix_quote(program)),
                        ],
                        Duration::from_secs(7),
                    )
            }
        }
    }

    fn available_programs(
        &self,
        programs: &std::collections::BTreeSet<String>,
    ) -> std::collections::BTreeSet<String> {
        if matches!(self, Backend::Local) {
            return programs
                .iter()
                .filter(|program| self.program_exists(program))
                .cloned()
                .collect();
        }
        let probe = programs
            .iter()
            .map(|program| {
                let quoted = posix_quote(program);
                format!("command -v {quoted} >/dev/null 2>&1 && printf '%s\\n' {quoted}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        let output = match self {
            Backend::Wsl { distro } => command_output(
                "wsl",
                &["-d", distro, "--", "sh", "-lc", &probe],
                Duration::from_secs(15),
            ),
            Backend::Docker { container } => command_output(
                "docker",
                &["exec", container, "sh", "-lc", &probe],
                Duration::from_secs(15),
            ),
            Backend::Ssh { host } if !host.trim().is_empty() => command_output(
                "ssh",
                &[
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=5",
                    host,
                    &probe,
                ],
                Duration::from_secs(20),
            ),
            _ => None,
        };
        output
            .unwrap_or_default()
            .lines()
            .map(ToString::to_string)
            .collect()
    }
}

#[derive(Debug)]
struct ProcessOutput {
    exit_code: Option<i32>,
    timed_out: bool,
    truncated: bool,
    text: String,
}

pub fn load_catalog(path: Option<&Path>) -> Result<ToolCatalog, String> {
    match path {
        Some(path) if path.exists() => ToolCatalog::from_path(path),
        _ => ToolCatalog::builtin(),
    }
}

pub fn export_builtin_catalog(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} 已存在，不覆盖", path.display()));
    }
    std::fs::write(path, BUILTIN_CATALOG)
        .map_err(|e| format!("写入 MCP 工具目录失败 {}: {e}", path.display()))
}

pub fn run_mcp_stdio(runner: &ToolRunner) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in BufReader::new(stdin.lock()).lines() {
        let line = line.map_err(|e| format!("读取 MCP 请求失败: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let Some(response) = handle_mcp_request(runner, &request) {
            serde_json::to_writer(&mut stdout, &response).map_err(|e| e.to_string())?;
            stdout.write_all(b"\n").map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn handle_mcp_request(runner: &ToolRunner, request: &Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match method {
        "notifications/initialized" => None,
        "initialize" => Some(json!({
            "jsonrpc":"2.0", "id":id,
            "result": {
                "protocolVersion":"2024-11-05",
                "serverInfo":{"name":"julong-codex-mcp","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"tools":{}}
            }
        })),
        "tools/list" => {
            let tools: Vec<Value> = runner
                .catalog
                .tools
                .iter()
                .filter(|tool| !tool.command_tool || runner.catalog.defaults.allow_command_tools)
                .map(|tool| {
                    let mut properties = serde_json::Map::new();
                    let mut required = Vec::new();
                    for parameter in &tool.parameters {
                        properties.insert(
                            parameter.name.clone(),
                            json!({"type":"string", "description":parameter.description}),
                        );
                        if parameter.required {
                            required.push(Value::String(parameter.name.clone()));
                        }
                    }
                    json!({
                        "name":tool.name,
                        "description":tool.description,
                        "inputSchema":{"type":"object", "properties":properties, "required":required}
                    })
                })
                .collect();
            Some(json!({"jsonrpc":"2.0", "id":id, "result":{"tools":tools}}))
        }
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = params
                .and_then(|p| p.get("arguments"))
                .and_then(Value::as_object);
            let args = arguments
                .map(|map| {
                    map.iter()
                        .map(|(key, value)| (key.clone(), value.as_str().unwrap_or("").to_string()))
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            match runner.execute(name, &args) {
                Ok(result) => Some(json!({
                    "jsonrpc":"2.0", "id":id,
                    "result":{"content":[{"type":"text","text":result.output}],"isError":result.exit_code != Some(0) || result.timed_out}
                })),
                Err(error) => Some(json!({
                    "jsonrpc":"2.0", "id":id,
                    "result":{"content":[{"type":"text","text":error}],"isError":true}
                })),
            }
        }
        _ => Some(json!({
            "jsonrpc":"2.0", "id":id,
            "error":{"code":-32601,"message":format!("未知 MCP 方法: {method}")}
        })),
    }
}

fn resolve_backend(requested: &str, defaults: &ToolDefaults) -> Result<Backend, String> {
    match requested.to_ascii_lowercase().as_str() {
        "local" => Ok(Backend::Local),
        "wsl" => Ok(Backend::Wsl {
            distro: defaults.wsl_distro.clone(),
        }),
        "docker" => Ok(Backend::Docker {
            container: defaults.docker_container.clone(),
        }),
        "ssh" => Ok(Backend::Ssh {
            host: defaults.ssh_host.clone(),
        }),
        "auto" => {
            if cfg!(windows)
                && command_success(
                    "wsl",
                    &["-d", &defaults.wsl_distro, "--", "true"],
                    Duration::from_secs(5),
                )
            {
                Ok(Backend::Wsl {
                    distro: defaults.wsl_distro.clone(),
                })
            } else if command_output(
                "docker",
                &[
                    "inspect",
                    "-f",
                    "{{.State.Running}}",
                    &defaults.docker_container,
                ],
                Duration::from_secs(5),
            )
            .map(|output| output.trim() == "true")
            .unwrap_or(false)
            {
                Ok(Backend::Docker {
                    container: defaults.docker_container.clone(),
                })
            } else {
                Ok(Backend::Local)
            }
        }
        other => Err(format!("未知 MCP 后端: {other}")),
    }
}

fn validate_parameters(
    tool: &ToolDefinition,
    args: &BTreeMap<String, String>,
) -> Result<(), String> {
    let known: std::collections::BTreeSet<&str> = tool
        .parameters
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    if let Some(unknown) = args.keys().find(|key| !known.contains(key.as_str())) {
        return Err(format!("工具 {} 包含未知参数 {}", tool.name, unknown));
    }
    for parameter in &tool.parameters {
        if parameter.required
            && args
                .get(&parameter.name)
                .map_or(true, |value| value.trim().is_empty())
        {
            return Err(format!("工具 {} 缺少参数 {}", tool.name, parameter.name));
        }
    }
    Ok(())
}

fn expand_arguments(
    templates: &[String],
    tool: &ToolDefinition,
    args: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for template in templates {
        let mut value = template.clone();
        for parameter in &tool.parameters {
            let marker = format!("{{{}}}", parameter.name);
            value = value.replace(
                &marker,
                args.get(&parameter.name).map(String::as_str).unwrap_or(""),
            );
        }
        if value.trim().is_empty() {
            continue;
        }
        let should_expand = tool
            .expand_parameters
            .iter()
            .any(|name| template.contains(&format!("{{{name}}}")));
        if should_expand {
            result.extend(value.split_whitespace().map(ToString::to_string));
        } else {
            result.push(value);
        }
    }
    Ok(result)
}

fn tool_programs(tool: &ToolDefinition, backend: &Backend) -> Vec<String> {
    if cfg!(windows) && matches!(backend, Backend::Local) && !tool.windows_programs.is_empty() {
        tool.windows_programs.clone()
    } else {
        tool.programs.clone()
    }
}

fn platform_allowed(tool: &ToolDefinition, backend: &Backend) -> bool {
    if tool.platforms.is_empty() {
        return true;
    }
    let supported: &[&str] = match backend {
        Backend::Wsl { .. } => &["wsl", "linux"],
        Backend::Docker { .. } | Backend::Ssh { .. } => &["linux"],
        Backend::Local if cfg!(windows) => &["windows"],
        Backend::Local if cfg!(target_os = "macos") => &["macos"],
        Backend::Local => &["linux"],
    };
    tool.platforms
        .iter()
        .any(|value| supported.contains(&value.as_str()))
}

fn command_exists(program: &str) -> bool {
    let mut names = vec![program.to_string()];
    if cfg!(windows) && Path::new(program).extension().is_none() {
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        names.extend(
            extensions
                .split(';')
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("{program}{}", value.to_ascii_lowercase())),
        );
        names.extend(
            extensions
                .split(';')
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("{program}{}", value.to_ascii_uppercase())),
        );
    }
    if program.contains('/') || program.contains('\\') {
        return names.iter().any(|name| Path::new(name).is_file());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .any(|dir| names.iter().any(|name| dir.join(name).is_file()))
}

fn command_success(program: &str, args: &[&str], timeout: Duration) -> bool {
    command_output(program, args, timeout).is_some()
}

fn command_output(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return None;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut output = String::new();
                let mut stdout = child.stdout.take()?;
                let _ = std::io::Read::read_to_string(&mut stdout, &mut output);
                return Some(output);
            }
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn read_limited<R: std::io::Read>(reader: R, limit: usize) -> ProcessOutput {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        match std::io::Read::read(&mut reader, &mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if bytes.len() < limit {
                    let keep = count.min(limit - bytes.len());
                    bytes.extend_from_slice(&buffer[..keep]);
                    if keep < count {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    ProcessOutput {
        exit_code: None,
        timed_out: false,
        truncated,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn truncate_utf8(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}\n[输出已截断]", &text[..end]), true)
}

fn default_backend() -> String {
    "auto".into()
}
fn default_wsl_distro() -> String {
    "kali-linux".into()
}
fn default_docker_container() -> String {
    "kali-tools".into()
}
fn default_timeout() -> u64 {
    600
}
fn default_output_limit() -> usize {
    100_000
}
fn default_required() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_has_unique_tools_and_categories() {
        let catalog = ToolCatalog::builtin().unwrap();
        assert_eq!(catalog.tools.len(), 31);
        assert_eq!(catalog.categories().values().sum::<usize>(), 31);
        assert!(catalog.get("nmap_scan").is_some());
        assert!(catalog.get("strings_extract").is_some());
    }

    #[test]
    fn command_tools_are_disabled_by_default() {
        let catalog = ToolCatalog::builtin().unwrap();
        let runner = ToolRunner::new(catalog, Some("local")).unwrap();
        let args = BTreeMap::from([(String::from("command"), String::from("echo hi"))]);
        assert!(runner.execute("shell_exec", &args).is_err());
    }

    #[test]
    fn arguments_are_passed_as_argv_without_shell_interpolation() {
        let tool = ToolDefinition {
            name: "demo".into(),
            description: "demo".into(),
            category: "test".into(),
            programs: vec!["printf".into()],
            windows_programs: Vec::new(),
            platforms: Vec::new(),
            arguments: vec!["{value}".into()],
            windows_arguments: Vec::new(),
            expand_parameters: Vec::new(),
            command_tool: false,
            parameters: vec![ToolParameter {
                name: "value".into(),
                description: String::new(),
                required: true,
            }],
        };
        let args = BTreeMap::from([(String::from("value"), String::from("a; echo injected"))]);
        let expanded = expand_arguments(&tool.arguments, &tool, &args).unwrap();
        assert_eq!(expanded, vec!["a; echo injected"]);
    }

    #[test]
    fn tools_list_hides_disabled_command_tools() {
        let catalog = ToolCatalog::builtin().unwrap();
        let runner = ToolRunner::new(catalog, Some("local")).unwrap();
        let response = handle_mcp_request(
            &runner,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 28);
        assert!(!tools.iter().any(|tool| tool["name"] == "shell_exec"));
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let catalog = ToolCatalog::builtin().unwrap();
        let tool = catalog.get("strings_extract").unwrap();
        let args = BTreeMap::from([
            (String::from("file"), String::from("sample.bin")),
            (String::from("min_len"), String::from("4")),
            (String::from("typo"), String::from("value")),
        ]);
        assert!(validate_parameters(tool, &args)
            .unwrap_err()
            .contains("未知参数 typo"));
    }
}
