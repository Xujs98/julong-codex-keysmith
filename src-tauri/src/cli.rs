//! `julong-codex` 控制台入口。
//!
//! CLI 与桌面端共用 DeployManager、MitmCore、解析器和资源目录。桌面端继续
//! 负责交互式 UI；CLI 负责可重复执行的 start/stop/status 和 MCP stdio 服务。

use crate::core::MitmCore;
use crate::deploy::DeployManager;
use crate::extensions::inject::SystemPromptInjector;
use crate::extensions::memory::MemoryKernel;
use crate::extensions::sse_parser::UniversalSseParser;
use crate::extensions::tamper::TamperEngine;
use crate::mcp_tools::{load_catalog, run_mcp_stdio, ToolRunner, USER_CATALOG_FILE};
use crate::providers;
use crate::runtime;
use futures::StreamExt;
use http::StatusCode;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const BRIDGE_FALLBACK: &str = include_str!("../../bridge.md");

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("start") => command_start(),
        Some("stop") => command_stop(),
        Some("status") => command_status(),
        Some("mcp") => command_mcp(&args[1..]),
        Some("--proxy-daemon") => command_daemon(),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("未知命令: {other}");
            print_help();
            2
        }
    }
}

fn print_help() {
    println!("julong-codex — 本地代理与 MCP 工具控制台");
    println!();
    println!("用法:");
    println!("  julong-codex start");
    println!("  julong-codex stop");
    println!("  julong-codex status");
    println!("  julong-codex mcp list [BACKEND_OPTIONS]");
    println!("  julong-codex mcp doctor [BACKEND_OPTIONS]");
    println!("  julong-codex mcp export");
    println!("  julong-codex mcp enable TOOL");
    println!("  julong-codex mcp disable TOOL");
    println!("  julong-codex mcp allow-commands");
    println!("  julong-codex mcp deny-commands");
    println!("  julong-codex mcp serve [BACKEND_OPTIONS]");
    println!("  julong-codex mcp call TOOL key=value ... [BACKEND_OPTIONS]");
    println!();
    println!("BACKEND_OPTIONS:");
    println!("  --backend auto|local|wsl|docker|ssh");
    println!("  --wsl-distro NAME  --docker-container NAME  --ssh-host HOST");
}

fn command_start() -> i32 {
    if runtime::port_is_listening() {
        if runtime::proxy_is_healthy() {
            let deployment_ok = DeployManager::new()
                .map(|manager| {
                    let status = manager.status();
                    status.bridge_active && status.integrity_ok && !status.transaction_pending
                })
                .unwrap_or(false);
            if !deployment_ok {
                eprintln!("[FAIL] 矩龙代理已监听，但部署状态不一致，请先执行 stop 恢复");
                return 1;
            }
            println!("Proxy: RUNNING on 127.0.0.1:8080");
            println!("[OK] 已有矩龙代理在运行，未重复启动");
            return 0;
        }
        eprintln!("[FAIL] 端口 8080 已被其他进程占用");
        return 1;
    }

    let manager = match DeployManager::new() {
        Some(manager) => manager,
        None => {
            eprintln!("[FAIL] Codex 配置目录未找到");
            return 1;
        }
    };
    let bridge = resource_file("bridge.md")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| BRIDGE_FALLBACK.to_string());
    let skills = resource_dir("codex-skills");
    if let Err(error) = manager.apply_with_optional_skills(&bridge, skills.as_deref()) {
        eprintln!("[FAIL] 部署失败: {error}");
        return 1;
    }

    // 与桌面端保持一致：供应商凭据与自定义 model provider 只在启动时
    // 投影到 Codex 的 config.toml/auth.json。
    if let Ok(runtime) = providers::ProviderRuntime::load() {
        if let Some(provider) = runtime
            .providers
            .iter()
            .find(|provider| providers::valid_relay_url(&provider.normalized_url()))
        {
            if let Err(error) = providers::activate(provider, true) {
                eprintln!("[FAIL] 供应商配置写入失败: {error}");
                let _ = manager.restore();
                return 1;
            }
        }
    }

    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("[FAIL] 获取 CLI 路径失败: {error}");
            let _ = manager.restore();
            return 1;
        }
    };
    let mut child = match Command::new(executable)
        .arg("--proxy-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("[FAIL] 启动代理进程失败: {error}");
            let _ = manager.restore();
            return 1;
        }
    };

    if let Err(error) = runtime::write_pid(manager.codex_home(), child.id()) {
        eprintln!("[FAIL] {error}");
        let _ = child.kill();
        let _ = child.wait();
        let _ = manager.restore();
        return 1;
    }

    if !runtime::wait_for_port(true, Duration::from_secs(5)) {
        eprintln!("[FAIL] 代理未能在 5 秒内监听 127.0.0.1:8080");
        let _ = runtime::terminate_managed_proxy(manager.codex_home());
        let _ = manager.restore();
        return 1;
    }
    println!("[OK] Proxy started on 127.0.0.1:8080 (pid {})", child.id());
    command_status()
}

fn command_stop() -> i32 {
    let manager = match DeployManager::new() {
        Some(manager) => manager,
        None => {
            eprintln!("[FAIL] Codex 配置目录未找到");
            return 1;
        }
    };
    if runtime::proxy_is_healthy() && runtime::managed_proxy_pid(manager.codex_home()).is_none() {
        eprintln!("[FAIL] 检测到桌面端内置代理，请从桌面端停止以保持状态一致");
        return 1;
    }
    match runtime::terminate_managed_proxy(manager.codex_home()) {
        Ok(true) => {
            if !runtime::wait_for_port(false, Duration::from_secs(5)) {
                eprintln!("[WARN] 代理端口仍在监听，继续执行配置恢复");
            }
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("[FAIL] {error}");
            return 1;
        }
    }
    match manager.restore() {
        Ok(message) => {
            println!("[OK] Proxy stopped");
            println!("[OK] {message}");
            0
        }
        Err(error) => {
            eprintln!("[FAIL] 恢复 Codex 配置失败: {error}");
            1
        }
    }
}

fn command_status() -> i32 {
    let manager = DeployManager::new();
    let (home, deployed, backup, pending, integrity, relay) = if let Some(manager) = &manager {
        let status = manager.status();
        (
            manager.codex_home().display().to_string(),
            status.bridge_active,
            status.config_backed_up,
            status.transaction_pending,
            status.integrity_ok,
            providers::configured_relay_url(manager.codex_home())
                .unwrap_or_else(|| "未检测到".into()),
        )
    } else {
        (
            "未检测到".into(),
            false,
            false,
            false,
            false,
            "未检测到".into(),
        )
    };
    let pid = manager
        .as_ref()
        .and_then(|m| runtime::managed_proxy_pid(m.codex_home()));
    let occupied = runtime::port_is_listening();
    let running = runtime::proxy_is_healthy();
    println!("Codex home: {home}");
    println!(
        "Proxy: {}",
        if running {
            "RUNNING on :8080"
        } else if occupied {
            "PORT OCCUPIED"
        } else {
            "STOPPED"
        }
    );
    println!(
        "Proxy PID: {}",
        pid.map(|p| p.to_string()).unwrap_or_else(|| "--".into())
    );
    println!("bridge.md: {}", if deployed { "ACTIVE" } else { "not set" });
    println!("config backup: {}", if backup { "present" } else { "none" });
    println!(
        "deployment integrity: {}",
        if integrity { "OK" } else { "CHECK" }
    );
    println!("transaction: {}", if pending { "PENDING" } else { "clean" });
    println!("relay: {relay}");
    let consistent = manager.is_some()
        && if running {
            deployed && integrity && !pending
        } else {
            !occupied && !deployed && !pending
        };
    if consistent {
        0
    } else {
        1
    }
}

fn command_mcp(args: &[String]) -> i32 {
    let action = args.first().map(String::as_str).unwrap_or("list");
    let backend = option_value(args, "--backend");
    let catalog_path = DeployManager::find_codex_home()
        .map(|home| home.join(USER_CATALOG_FILE))
        .filter(|path| path.exists())
        .or_else(|| resource_file("mcp-tools/tools.json"));
    let mut catalog = match load_catalog(catalog_path.as_deref()) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("[FAIL] {error}");
            return 1;
        }
    };
    if let Some(value) = option_value(args, "--wsl-distro") {
        catalog.defaults.wsl_distro = value;
    }
    if let Some(value) = option_value(args, "--docker-container") {
        catalog.defaults.docker_container = value;
    }
    if let Some(value) = option_value(args, "--ssh-host") {
        catalog.defaults.ssh_host = value;
    }
    let runner = match ToolRunner::new(catalog, backend.as_deref()) {
        Ok(runner) => runner,
        Err(error) => {
            eprintln!("[FAIL] {error}");
            return 1;
        }
    };

    match action {
        "list" => {
            println!("MCP tools: {}", runner.catalog.tools.len());
            for tool in &runner.catalog.tools {
                println!(
                    "{:<22} {:<14} {}",
                    tool.name, tool.category, tool.description
                );
            }
            0
        }
        "doctor" => {
            let requested = backend.unwrap_or_else(|| runner.catalog.defaults.backend.clone());
            let backend_status = runner.backend_status(&requested);
            println!(
                "Backend: {} ({})",
                backend_status.selected, backend_status.detail
            );
            println!("Ready: {}", backend_status.ready);
            let available = runner.availability();
            let ready_count = available.iter().filter(|item| item.available).count();
            println!("Tools available: {}/{}", ready_count, available.len());
            for item in available {
                println!(
                    "[{}] {:<22} {}",
                    if item.available { "OK" } else { "--" },
                    item.name,
                    item.reason
                );
            }
            if backend_status.ready {
                0
            } else {
                1
            }
        }
        "export" => {
            let Some(home) = DeployManager::find_codex_home() else {
                eprintln!("[FAIL] Codex 配置目录未找到");
                return 1;
            };
            let path = home.join(USER_CATALOG_FILE);
            match crate::mcp_tools::export_builtin_catalog(&path) {
                Ok(()) => {
                    println!("[OK] {}", path.display());
                    0
                }
                Err(error) => {
                    eprintln!("[FAIL] {error}");
                    1
                }
            }
        }
        "enable" | "disable" => {
            let positional = positional_mcp_args(args, 1);
            let Some(name) = positional.first() else {
                eprintln!("用法: julong-codex mcp {} TOOL", action);
                return 2;
            };
            let Some(home) = DeployManager::find_codex_home() else {
                eprintln!("[FAIL] Codex 配置目录未找到");
                return 1;
            };
            let path = home.join(USER_CATALOG_FILE);
            match crate::mcp_tools::set_tool_enabled(&path, name, action == "enable") {
                Ok(()) => {
                    println!(
                        "[OK] {} {}",
                        if action == "enable" {
                            "已启用"
                        } else {
                            "已停用"
                        },
                        name
                    );
                    0
                }
                Err(error) => {
                    eprintln!("[FAIL] {error}");
                    1
                }
            }
        }
        "allow-commands" | "deny-commands" => {
            let Some(home) = DeployManager::find_codex_home() else {
                eprintln!("[FAIL] Codex 配置目录未找到");
                return 1;
            };
            let path = home.join(USER_CATALOG_FILE);
            let enabled = action == "allow-commands";
            match crate::mcp_tools::set_command_tools_enabled(&path, enabled) {
                Ok(()) => {
                    println!(
                        "[OK] 高权限命令工具已{}",
                        if enabled { "允许" } else { "禁止" }
                    );
                    0
                }
                Err(error) => {
                    eprintln!("[FAIL] {error}");
                    1
                }
            }
        }
        "serve" => match run_mcp_stdio(&runner) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("[FAIL] MCP 服务退出: {error}");
                1
            }
        },
        "call" => {
            let positional = positional_mcp_args(args, 1);
            let Some(name) = positional.first() else {
                eprintln!("用法: julong-codex mcp call TOOL key=value ...");
                return 2;
            };
            let mut values = BTreeMap::new();
            for item in positional.iter().skip(1) {
                let Some((key, value)) = item.split_once('=') else {
                    eprintln!("参数必须使用 key=value 格式: {item}");
                    return 2;
                };
                values.insert(key.to_string(), value.to_string());
            }
            match runner.execute(name, &values) {
                Ok(result) => {
                    println!(
                        "tool={} backend={} exit={:?} duration_ms={}",
                        result.tool, result.backend, result.exit_code, result.duration_ms
                    );
                    print!("{}", result.output);
                    if result.timed_out {
                        eprintln!("[WARN] 工具执行超时");
                    }
                    0
                }
                Err(error) => {
                    eprintln!("[FAIL] {error}");
                    1
                }
            }
        }
        _ => {
            eprintln!("未知 MCP 子命令: {action}");
            2
        }
    }
}

fn command_daemon() -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[FAIL] 创建代理运行时失败: {error}");
            return 1;
        }
    };
    match runtime.block_on(run_headless_proxy()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[FAIL] 代理退出: {error}");
            1
        }
    }
}

async fn run_headless_proxy() -> Result<(), String> {
    let manager = DeployManager::new().ok_or("Codex 配置目录未找到")?;
    let relay = providers::configured_relay_url(manager.codex_home())
        .ok_or("尚未配置供应商，请先设置中转站 URL")?;
    let bridge = resource_file("bridge.md")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| BRIDGE_FALLBACK.to_string());
    let memory = Arc::new(MemoryKernel::new(
        manager.codex_home().join("julong-memory.json"),
    ));
    let core = Arc::new(
        MitmCore::builder()
            .target(relay)
            .request_interceptor(SystemPromptInjector::new(bridge))
            .response_parser(UniversalSseParser)
            .response_interceptor(TamperEngine::default_rules())
            .response_interceptor(memory)
            .build()?,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .map_err(|e| format!("绑定 127.0.0.1:8080 失败: {e}"))?;
    let shared = core.clone();
    let app = axum::Router::new()
        .route("/", axum::routing::get(|| async { "julong-codex ok" }))
        .route(
            "/{*path}",
            axum::routing::any(move |request| headless_handler(request, shared.clone())),
        );
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("代理服务异常: {e}"))
}

async fn headless_handler(
    req: axum::extract::Request,
    core: Arc<MitmCore>,
) -> axum::response::Response {
    if req.method() == http::Method::GET {
        return response(
            StatusCode::OK,
            "text/plain; charset=utf-8",
            bytes::Bytes::from_static(b"julong-codex ok"),
        );
    }
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return response(
                StatusCode::BAD_REQUEST,
                "application/json",
                bytes::Bytes::from(json!({"error": error.to_string()}).to_string()),
            )
        }
    };
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    let upstream = match core
        .handle_request(parts.method, path, parts.headers, bytes)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return response(
                StatusCode::BAD_GATEWAY,
                "application/json",
                bytes::Bytes::from(json!({"error": error.to_string()}).to_string()),
            )
        }
    };
    let status = StatusCode::from_u16(upstream.status).unwrap_or(StatusCode::OK);
    let content_type = upstream.content_type.clone();
    let is_sse = content_type
        .as_deref()
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);
    let mut stream = upstream.response.bytes_stream();
    let mut accumulated = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => accumulated.extend_from_slice(&chunk),
            Err(error) => {
                return response(
                    StatusCode::BAD_GATEWAY,
                    "application/json",
                    bytes::Bytes::from(json!({"error": error.to_string()}).to_string()),
                )
            }
        }
    }
    let (mut final_body, tampered, _) = core.finalize_response(
        upstream.meta,
        upstream.status,
        content_type.clone(),
        bytes::Bytes::from(accumulated),
        0,
    );
    let response_type = if tampered && is_sse {
        final_body = bytes::Bytes::from(wrap_tamper_as_sse(
            std::str::from_utf8(&final_body).unwrap_or("响应已替换"),
        ));
        "text/event-stream"
    } else {
        content_type.as_deref().unwrap_or("application/json")
    };
    let mut builder = axum::response::Response::builder().status(status);
    for (name, value) in upstream.headers.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-connection"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
                | "content-encoding"
                | "content-type"
        ) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
        .header("content-type", response_type)
        .body(axum::body::Body::from(final_body))
        .unwrap_or_else(|_| {
            response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "text/plain",
                bytes::Bytes::from_static(b"response build failed"),
            )
        })
}

fn response(
    status: StatusCode,
    content_type: &str,
    body: bytes::Bytes,
) -> axum::response::Response {
    axum::response::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn wrap_tamper_as_sse(text: &str) -> String {
    let created = json!({"type":"response.created","response":{"id":"resp_tamper","object":"response","status":"in_progress","output":[]}});
    let delta = json!({"type":"response.output_text.delta","item_id":"msg_tamper","output_index":0,"content_index":0,"delta":text});
    let done = json!({"type":"response.output_text.done","item_id":"msg_tamper","output_index":0,"content_index":0,"text":text});
    let completed = json!({"type":"response.completed","response":{"id":"resp_tamper","object":"response","status":"completed","output":[{"id":"msg_tamper","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text}]}]}});
    format!("event: response.created\ndata: {created}\n\nevent: response.output_text.delta\ndata: {delta}\n\nevent: response.output_text.done\ndata: {done}\n\nevent: response.completed\ndata: {completed}\n\n")
}

fn resource_file(name: &str) -> Option<PathBuf> {
    resource_candidates(name)
        .into_iter()
        .find(|path| path.is_file())
}

fn resource_dir(name: &str) -> Option<PathBuf> {
    resource_candidates(name)
        .into_iter()
        .find(|path| path.is_dir())
}

fn resource_candidates(name: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        result.push(cwd.join(name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            result.push(parent.join(name));
            result.push(parent.join("resources").join(name));
            result.push(parent.join("Resources").join(name));
            result.push(parent.join("..").join("Resources").join(name));
            if let Some(root) = parent.parent().and_then(Path::parent) {
                result.push(root.join(name));
            }
        }
    }
    result
}

fn option_value(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn positional_mcp_args(args: &[String], skip: usize) -> Vec<&String> {
    const OPTIONS: [&str; 4] = [
        "--backend",
        "--wsl-distro",
        "--docker-container",
        "--ssh-host",
    ];
    let mut result = Vec::new();
    let mut index = skip;
    while index < args.len() {
        if OPTIONS.contains(&args[index].as_str()) {
            index += 2;
        } else {
            result.push(&args[index]);
            index += 1;
        }
    }
    result
}
