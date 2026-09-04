// Super-Instruct — Tauri 桌面应用入口
// MITM Core 作为 Tauri 后端进程运行，前端通过事件系统接收实时数据

pub mod cli;
pub mod core;
pub mod deploy;
pub mod extensions;
pub mod log;
pub mod mcp_tools;
pub mod providers;
pub mod runtime;
pub mod skills;
pub mod transaction;

use futures::StreamExt;
use serde::Serialize;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::RwLock;

/// bridge.md 编译期嵌入 — 运行时文件查找全部失败时的最终 fallback
const BRIDGE_MD_FALLBACK: &str = include_str!("../../bridge.md");

use crate::core::MitmCore;
use crate::deploy::DeployManager;
use crate::extensions::activity::{ActivityStatus, ActivityTracker};
use crate::extensions::inject::SystemPromptInjector;
use crate::extensions::memory::MemoryKernel;
use crate::extensions::monitor::{InteractionEvent, MonitorPanel, StatsEvent};
use crate::extensions::sse_parser::UniversalSseParser;
use crate::extensions::tamper::TamperEngine;

fn release_log_dir() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("Super-Instruct");
    }

    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("logs")))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"))
}

// ── AppState ──────────────────────────────────────────────
pub struct AppState {
    core: RwLock<Option<Arc<MitmCore>>>,
    proxy_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    monitor: RwLock<Option<Arc<MonitorPanel>>>,
    memory: RwLock<Option<Arc<MemoryKernel>>>,
    activity: RwLock<Option<Arc<ActivityTracker>>>,
    providers: RwLock<Option<Arc<RwLock<providers::ProviderRuntime>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            core: RwLock::new(None),
            proxy_handle: RwLock::new(None),
            monitor: RwLock::new(None),
            memory: RwLock::new(None),
            activity: RwLock::new(None),
            providers: RwLock::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ── 应用入口 ──────────────────────────────────────────────
pub fn run() {
    // 日志: 控制台 + 文件双输出, guard 保活到应用结束
    let log_dir = if cfg!(debug_assertions) {
        // Dev: 写到项目根 (parent of src-tauri/)
        std::env::current_dir()
            .ok()
            .and_then(|d| d.parent().map(|p| p.join("logs")))
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "../logs".to_string())
    } else {
        // macOS 的 .app 目录通常不可写；其他平台保留 exe 同级日志目录。
        release_log_dir().to_string_lossy().to_string()
    };
    let _log_guard = log::init_logging(&log_dir);

    tracing::info!("Super-Instruct starting up");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState::new())
        .on_window_event(|window, event| {
            // 拦截关闭按钮: 隐藏窗口而非退出，让用户通过托盘退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            // 优先恢复崩溃中断的文件事务，再处理残留部署。
            if let Some(manager) = DeployManager::new() {
                match manager.recover_pending() {
                    Ok(Some(message)) => tracing::warn!("startup: {message}"),
                    Ok(None) => tracing::debug!("startup: no pending deployment transaction"),
                    Err(error) => tracing::error!("startup: transaction recovery failed: {error}"),
                }
                let status = manager.status();
                if status.config_backed_up && !runtime::proxy_is_healthy() {
                    tracing::warn!(
                        "startup: residual deployment detected (bridge_active={}, backup exists), auto-restoring",
                        status.bridge_active
                    );
                    match manager.restore() {
                        Ok(msg) => {
                            tracing::info!("startup: auto-restore succeeded: {}", msg);
                            let _ = app.emit("interaction", InteractionEvent {
                                id: 0,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                category: "system".into(),
                                user_preview: "残留部署检测".into(),
                                ai_preview: format!("检测到上次未正常关闭，已自动恢复 Codex 配置 ({})", msg),
                                thinking_preview: String::new(),
                                tampered: false,
                                bytes: 0,
                                duration_ms: 0,
                            });
                        }
                        Err(e) => tracing::error!("startup: auto-restore failed: {}", e),
                    }
                } else if status.config_backed_up {
                    tracing::info!(
                        "startup: managed CLI proxy is active on {}, keeping deployment",
                        runtime::PROXY_ADDRESS
                    );
                } else {
                    tracing::debug!("startup: no residual deployment detected");
                }
            }

            // 首次运行自动生成外部篡改规则字典 tamper-rules.txt（不覆盖已有文件）
            // 用户可直接编辑该文件 + 重启代理来动态增删规则，无需重新编译
            let rules_path = tamper_rules_path();
            if !rules_path.exists() {
                let content = crate::extensions::tamper::default_tamper_patterns().join("\n");
                match std::fs::write(&rules_path, content) {
                    Ok(_) => tracing::info!(
                        "startup: tamper-rules.txt generated with {} built-in rules: {}",
                        crate::extensions::tamper::default_tamper_patterns().len(),
                        rules_path.display()
                    ),
                    Err(e) => tracing::warn!(
                        "startup: generate tamper-rules.txt failed ({}), will use built-in rules",
                        e
                    ),
                }
            }

            // 系统托盘
            let show_item = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("矩龙破甲")
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            // 退出前只恢复桌面端自己的部署。CLI 托管代理存活时保留配置。
                            if let Some(manager) = DeployManager::new() {
                                let status = manager.status();
                                let cli_managed = runtime::proxy_is_healthy()
                                    && runtime::managed_proxy_pid(manager.codex_home()).is_some();
                                if (status.config_backed_up || status.bridge_active) && !cli_managed {
                                    tracing::info!("quit: restoring codex config before exit");
                                    match manager.restore() {
                                        Ok(msg) => tracing::info!("quit: restore succeeded: {}", msg),
                                        Err(e) => tracing::error!("quit: restore failed: {}", e),
                                    }
                                } else if cli_managed {
                                    tracing::info!(
                                        "quit: CLI managed proxy is active; keeping deployment"
                                    );
                                }
                            }
                            // 先销毁窗口再退出，避免 Chromium "Failed to unregister class" Error 1412
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.destroy();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            preflight_check,
            get_deploy_status,
            preview_deployment,
            recover_deployment,
            start_proxy,
            stop_proxy,
            deploy_bridge,
            restore_codex,
            restore_clean_environment,
            get_stats,
            get_history,
            get_proxy_status,
            get_activity_status,
            get_codex_info,
            set_relay_url,
            get_tamper_rules,
            export_tamper_rules,
            list_skills,
            toggle_skill,
            toggle_all_skills,
            delete_skill_cmd,
            minimize_window,
            toggle_maximize,
            close_window,
            show_window,
            quit_app,
            list_providers,
            save_provider,
            delete_provider,
            use_provider,
            reorder_providers,
            test_provider,
            fetch_provider_models,
            get_provider_runtime_status,
            get_mcp_tools,
            check_mcp_tools,
            export_mcp_tools,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ── Tauri Commands ────────────────────────────────────────

/// 智能探测中转站 API 路径前缀
/// 尝试 GET {url}/v1/models — 非 404 则 /v1 前缀存在，规范化为 {url}/v1
/// 已含 /v1 后缀则直接返回，探测失败时回退到原始 url
async fn probe_api_prefix(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');

    // 已包含 /v1 后缀，无需探测
    if trimmed.ends_with("/v1") {
        return trimmed.to_string();
    }

    let probe_url = format!("{}/v1/models", trimmed);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return trimmed.to_string(),
    };

    match client.get(&probe_url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 404 {
                tracing::info!(
                    "probe: {} -> 404, base url has no /v1 prefix, using as-is: {}",
                    probe_url,
                    trimmed
                );
                trimmed.to_string()
            } else {
                tracing::info!(
                    "probe: {} -> {}, normalizing base url to {}/v1",
                    probe_url,
                    status,
                    trimmed
                );
                format!("{}/v1", trimmed)
            }
        }
        Err(e) => {
            tracing::warn!(
                "probe: {} failed ({}), using base url as-is: {}",
                probe_url,
                e,
                trimmed
            );
            trimmed.to_string()
        }
    }
}

#[tauri::command]
async fn start_proxy(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if state.core.read().await.is_some() {
        return Err("Proxy already running".into());
    }

    tracing::info!("start_proxy: initializing");

    // 1. Codex home 硬检查 — 不存在则阻断启动
    let manager = DeployManager::new().ok_or_else(|| {
        tracing::error!("start_proxy: Codex home not found, aborting");
        "Codex 配置目录未找到，无法启动代理。请确认 Codex CLI 已安装并运行过至少一次。".to_string()
    })?;

    // Skills 开关在本地即时同步，启动代理时再校准一次目标目录。
    let skills_sync_message = skills::sync_enabled_skills(&app)?;

    // 2. 读取 bridge.md — 文件查找失败时用编译期嵌入的 fallback
    let instructions = match resolve_resource_file(&app, "bridge.md") {
        Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            tracing::warn!(
                "start_proxy: read bridge.md failed ({}), using embedded fallback",
                e
            );
            BRIDGE_MD_FALLBACK.to_string()
        }),
        Err(e) => {
            tracing::warn!(
                "start_proxy: bridge.md file lookup failed ({}), using embedded fallback",
                e
            );
            BRIDGE_MD_FALLBACK.to_string()
        }
    };

    // 3. 部署 — bridge_active 不足以判断完整部署，需验证 bridge_exists 和 relay_url_valid
    let status = manager.status();
    let needs_deploy = !status.bridge_active || !status.bridge_exists || !status.relay_url_valid;

    if needs_deploy {
        tracing::info!(
            "start_proxy: deploying (bridge_active={}, bridge_exists={}, relay_valid={})",
            status.bridge_active,
            status.bridge_exists,
            status.relay_url_valid
        );
        match manager.apply_with_optional_skills(&instructions, None) {
            Ok(msg) => tracing::info!("start_proxy: auto-deploy: {}", msg),
            Err(e) => {
                tracing::error!("start_proxy: auto-deploy failed: {}", e);
                return Err(format!("Auto-deploy failed: {}", e));
            }
        }
    } else {
        tracing::info!("start_proxy: already fully deployed, skipping auto-deploy");
    }

    // 4. 验证 relay URL — 无有效地址则阻断启动（防自环）
    let relay_url_raw = match providers::configured_relay_url(manager.codex_home()) {
        Some(url) => url,
        _ => {
            tracing::error!("start_proxy: no valid relay URL found");
            return Err("尚未配置供应商。请先在供应商页面添加中转站地址后再启动代理。".into());
        }
    };

    // 4b. 供应商运行时池：首位供应商作为当前上游
    let provider_runtime = providers::ProviderRuntime::load().unwrap_or_else(|e| {
        tracing::warn!("start_proxy: provider store unavailable: {}", e);
        providers::ProviderRuntime {
            providers: Vec::new(),
            current_index: 0,
            switch_count: 0,
            last_error: String::new(),
        }
    });
    let relay_url = if let Some(provider) = provider_runtime
        .providers
        .iter()
        .find(|provider| providers::valid_relay_url(&provider.normalized_url()))
        .cloned()
    {
        match providers::activate(&provider, true) {
            Ok(url) => url,
            Err(error) => {
                let rollback = manager.restore();
                return Err(match rollback {
                    Ok(_) => format!("供应商配置写入失败: {error}"),
                    Err(rollback_error) => {
                        format!("供应商配置写入失败: {error}; 启动部署回滚失败: {rollback_error}")
                    }
                });
            }
        }
    } else {
        probe_api_prefix(&relay_url_raw).await
    };
    let relay_url_display = relay_url.clone();
    tracing::info!(
        "start_proxy: relay_url = {} (raw: {})",
        relay_url_display,
        relay_url_raw
    );

    // 5. 创建扩展实例
    let monitor = Arc::new(MonitorPanel::new(app.clone()));
    let memory_path = data_dir_path("memory.json");
    let memory = Arc::new(MemoryKernel::new(&memory_path));
    let activity = Arc::new(ActivityTracker::new(app.clone()));

    // 篡改规则: 外部字典 tamper-rules.txt 优先 (每行一条正则, # 开头为注释)
    // 缺失/为空时回退到编译期内置规则，保证开箱即用
    let tamper = match load_tamper_rules() {
        Ok(rules) => {
            tracing::info!(
                "start_proxy: tamper rules = {} (external: {})",
                rules.rule_count(),
                rules.source()
            );
            rules
        }
        Err(e) => {
            tracing::warn!(
                "start_proxy: load external tamper rules failed ({}), using built-in defaults",
                e
            );
            let t = TamperEngine::default_rules();
            tracing::info!("start_proxy: tamper rules = {} (built-in)", t.rule_count());
            t
        }
    };
    let rule_count = tamper.rule_count();

    // 6. 构建 MitmCore
    let core = match MitmCore::builder()
        .target(&relay_url)
        .request_interceptor(SystemPromptInjector::new(instructions))
        .response_parser(UniversalSseParser)
        .response_interceptor(tamper)
        .response_interceptor(memory.clone())
        .response_interceptor(monitor.clone())
        .build()
    {
        Ok(core) => Arc::new(core),
        Err(error) => {
            let rollback = manager.restore();
            return Err(match rollback {
                Ok(_) => format!("创建代理核心失败: {error}"),
                Err(rollback_error) => {
                    format!("创建代理核心失败: {error}; 启动部署回滚失败: {rollback_error}")
                }
            });
        }
    };

    // 7. 同步绑定端口 — 失败则回滚部署，不修改 AppState
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .map_err(|e| {
            tracing::error!(
                "start_proxy: port 8080 bind failed, rolling back deploy: {}",
                e
            );
            if let Some(m) = DeployManager::new() {
                let _ = m.restore();
            }
            format!("端口 8080 绑定失败: {}。可能代理已在运行或端口被占用。", e)
        })?;
    tracing::info!("start_proxy: port 8080 bound successfully");

    // 8. 启动 axum server（listener 已绑定，不会再 panic）
    let core_for_server = core.clone();
    let activity_for_server = activity.clone();
    let providers_for_server = Arc::new(RwLock::new(provider_runtime));
    let providers_for_task = providers_for_server.clone();
    let app_for_server = app.clone();
    let relay_url_for_log = relay_url_display.clone();
    let handle = tokio::spawn(async move {
        let app = axum::Router::new()
            .route("/", axum::routing::get(health_check))
            .route(
                "/{*path}",
                axum::routing::any(move |req| {
                    handle_proxy(
                        req,
                        core_for_server.clone(),
                        activity_for_server.clone(),
                        providers_for_task.clone(),
                        app_for_server.clone(),
                    )
                }),
            );
        tracing::info!("Proxy listening on :8080 -> {}", relay_url_for_log);
        axum::serve(listener, app)
            .await
            .expect("proxy server error");
    });

    // 9. 存入 AppState
    *state.core.write().await = Some(core);
    *state.proxy_handle.write().await = Some(handle);
    *state.monitor.write().await = Some(monitor);
    *state.memory.write().await = Some(memory);
    *state.activity.write().await = Some(activity);
    *state.providers.write().await = Some(providers_for_server);

    let _ = app.emit("proxy-status", "running");

    // 推送系统日志到交互面板
    let _ = app.emit(
        "interaction",
        InteractionEvent {
            id: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
            category: "system".into(),
            user_preview: "启动代理".into(),
            ai_preview: format!("代理已启动 → 127.0.0.1:8080 → {}", relay_url_display),
            thinking_preview: String::new(),
            tampered: false,
            bytes: 0,
            duration_ms: 0,
        },
    );

    tracing::info!(
        "start_proxy: proxy running on :8080 -> {}",
        relay_url_display
    );

    Ok(format!(
        "Proxy running on :8080 -> {} | rules: {} | {}",
        relay_url_display, rule_count, skills_sync_message
    ))
}

#[tauri::command]
async fn stop_proxy(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let handle = state.proxy_handle.write().await.take();
    if let Some(h) = handle {
        h.abort();
    }
    *state.core.write().await = None;
    *state.monitor.write().await = None;
    *state.memory.write().await = None;
    if let Some(activity) = state.activity.write().await.take() {
        activity.reset();
    }
    *state.providers.write().await = None;

    // 桌面端也可停止 CLI 启动的托管代理；仅处理带 PID 文件的本项目进程。
    if let Some(manager) = DeployManager::new() {
        match runtime::terminate_managed_proxy(manager.codex_home()) {
            Ok(true) => {
                let _ = runtime::wait_for_port(false, std::time::Duration::from_secs(5));
            }
            Ok(false) => {}
            Err(error) => tracing::warn!("stop_proxy: managed process cleanup failed: {error}"),
        }
    }

    // 自动恢复 Codex 配置：停止代理后 base_url 指向死端口会导致 Codex CLI 不可用
    let restore_msg = if let Some(manager) = DeployManager::new() {
        match manager.restore() {
            Ok(msg) => {
                tracing::info!("stop_proxy: auto-restore: {}", msg);
                msg
            }
            Err(e) => {
                tracing::warn!("stop_proxy: auto-restore failed: {}", e);
                format!("auto-restore failed: {}", e)
            }
        }
    } else {
        "Codex home not found, skipping restore".to_string()
    };

    let _ = app.emit("proxy-status", "stopped");

    // 推送系统日志到交互面板
    let _ = app.emit(
        "interaction",
        InteractionEvent {
            id: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
            category: "system".into(),
            user_preview: "停止代理".into(),
            ai_preview: format!("代理已停止，Codex 配置已自动还原 ({})", restore_msg),
            thinking_preview: String::new(),
            tampered: false,
            bytes: 0,
            duration_ms: 0,
        },
    );

    tracing::info!("stop_proxy: proxy stopped, codex config restored");

    Ok(format!(
        "Proxy stopped, codex config restored ({})",
        restore_msg
    ))
}

#[tauri::command]
async fn deploy_bridge(app: tauri::AppHandle) -> Result<String, String> {
    tracing::info!("deploy_bridge: starting");
    let manager = DeployManager::new().ok_or("Codex home not found")?;
    let skills_sync_message = skills::sync_enabled_skills(&app)?;
    // bridge.md: 文件查找优先，fallback 用编译期嵌入版本
    let bridge_md = match resolve_resource_file(&app, "bridge.md") {
        Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            tracing::warn!(
                "deploy_bridge: read bridge.md failed ({}), using embedded fallback",
                e
            );
            BRIDGE_MD_FALLBACK.to_string()
        }),
        Err(e) => {
            tracing::warn!(
                "deploy_bridge: bridge.md file lookup failed ({}), using embedded fallback",
                e
            );
            BRIDGE_MD_FALLBACK.to_string()
        }
    };
    // Skills 由管理页独立同步，部署事务只处理 bridge/config。
    let result = manager
        .apply_with_optional_skills(&bridge_md, None)
        .map(|message| format!("{}；{}", message, skills_sync_message));
    match &result {
        Ok(msg) => tracing::info!("deploy_bridge: {}", msg),
        Err(e) => tracing::error!("deploy_bridge: failed: {}", e),
    }
    result
}

#[tauri::command]
async fn restore_codex() -> Result<String, String> {
    tracing::info!("restore_codex: starting");
    let manager = DeployManager::new().ok_or("Codex home not found")?;
    let result = manager.restore();
    match &result {
        Ok(msg) => tracing::info!("restore_codex: {}", msg),
        Err(e) => tracing::error!("restore_codex: failed: {}", e),
    }
    result
}

/// 停止桌面/CLI 托管代理并恢复 Codex 的干净环境。
/// 该命令供供应商页使用，统一处理运行时、配置、认证和部署文件，避免
/// 只恢复文件而留下 8080 代理或本地 relay_url.txt 的半残状态。
#[tauri::command]
async fn restore_clean_environment(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    tracing::info!("restore_clean_environment: stopping proxy and restoring files");
    let manager = DeployManager::new().ok_or("Codex home not found")?;

    let handle = state.proxy_handle.write().await.take();
    let had_local_proxy = handle.is_some();
    if let Some(handle) = handle {
        handle.abort();
    }
    *state.core.write().await = None;
    *state.monitor.write().await = None;
    *state.memory.write().await = None;
    if let Some(activity) = state.activity.write().await.take() {
        activity.reset();
    }
    *state.providers.write().await = None;

    let mut managed_proxy_stopped = false;
    match runtime::terminate_managed_proxy(manager.codex_home()) {
        Ok(true) => {
            managed_proxy_stopped = true;
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!("restore_clean_environment: managed process cleanup failed: {error}")
        }
    }

    if (had_local_proxy || managed_proxy_stopped)
        && !runtime::wait_for_port(false, std::time::Duration::from_secs(5))
    {
        return Err("代理端口仍在监听，已保留 Codex 配置未执行清理".into());
    }

    let message = manager.restore_clean()?;
    let _ = app.emit("proxy-status", "stopped");
    let _ = app.emit(
        "interaction",
        InteractionEvent {
            id: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
            category: "system".into(),
            user_preview: "还原干净环境".into(),
            ai_preview: message.clone(),
            thinking_preview: String::new(),
            tampered: false,
            bytes: 0,
            duration_ms: 0,
        },
    );
    tracing::info!("restore_clean_environment: {}", message);
    Ok(message)
}

#[tauri::command]
async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsEvent, String> {
    let monitor = state.monitor.read().await;
    let memory = state.memory.read().await;
    let mut stats = monitor.as_ref().ok_or("Proxy not running")?.get_stats();
    if let Some(mem) = memory.as_ref() {
        stats.memory_count = mem.success_count();
    }
    Ok(stats)
}

#[tauri::command]
async fn get_history(state: tauri::State<'_, AppState>) -> Result<Vec<InteractionEvent>, String> {
    let monitor = state.monitor.read().await;
    let monitor = monitor.as_ref().ok_or("Proxy not running")?;
    Ok(monitor.get_history())
}

#[tauri::command]
async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let core = state.core.read().await;
    if core.is_some() || runtime::proxy_is_healthy() {
        Ok("running".into())
    } else {
        Ok("stopped".into())
    }
}

#[tauri::command]
async fn get_activity_status(state: tauri::State<'_, AppState>) -> Result<ActivityStatus, String> {
    let activity = state.activity.read().await;
    Ok(activity
        .as_ref()
        .map(|tracker| tracker.snapshot())
        .unwrap_or_else(ActivityStatus::idle))
}

#[tauri::command]
async fn get_codex_info() -> Result<serde_json::Value, String> {
    let home = DeployManager::find_codex_home();
    let relay = home
        .as_ref()
        .and_then(|path| providers::configured_relay_url(path));
    Ok(serde_json::json!({
        "codex_home": home.map(|p| p.display().to_string()),
        "relay_url": relay,
    }))
}

fn mcp_catalog_path(app: &tauri::AppHandle) -> Result<(std::path::PathBuf, bool), String> {
    if let Some(home) = DeployManager::find_codex_home() {
        let user = home.join(crate::mcp_tools::USER_CATALOG_FILE);
        if user.exists() {
            return Ok((user, true));
        }
    }
    resolve_resource_file(app, "mcp-tools/tools.json").map(|path| (path, false))
}

#[tauri::command]
fn get_mcp_tools(
    app: tauri::AppHandle,
    backend: Option<String>,
) -> Result<serde_json::Value, String> {
    let (path, customized) = mcp_catalog_path(&app)?;
    let catalog = crate::mcp_tools::load_catalog(Some(&path))?;
    let runner = crate::mcp_tools::ToolRunner::new(catalog, backend.as_deref())?;
    let requested = backend.unwrap_or_else(|| runner.catalog.defaults.backend.clone());
    let backend_status = runner.backend_status(&requested);
    Ok(serde_json::json!({
        "source": path.display().to_string(),
        "customized": customized,
        "tool_count": runner.catalog.tools.len(),
        "categories": runner.catalog.categories(),
        "defaults": runner.catalog.defaults,
        "backend": backend_status,
        "tools": runner.catalog.tools,
    }))
}

#[tauri::command]
async fn check_mcp_tools(
    app: tauri::AppHandle,
    backend: Option<String>,
) -> Result<Vec<crate::mcp_tools::ToolAvailability>, String> {
    let (path, _) = mcp_catalog_path(&app)?;
    let catalog = crate::mcp_tools::load_catalog(Some(&path))?;
    let runner = crate::mcp_tools::ToolRunner::new(catalog, backend.as_deref())?;
    tokio::task::spawn_blocking(move || runner.availability())
        .await
        .map_err(|e| format!("MCP 工具检查任务失败: {e}"))
}

#[tauri::command]
fn export_mcp_tools() -> Result<String, String> {
    let home = DeployManager::find_codex_home().ok_or("Codex home not found")?;
    let path = home.join(crate::mcp_tools::USER_CATALOG_FILE);
    crate::mcp_tools::export_builtin_catalog(&path)?;
    Ok(path.display().to_string())
}

// ── Pre-flight 检查 ─────────────────────────────────────

#[derive(Clone, Serialize)]
struct PreflightResult {
    codex_home_found: bool,
    codex_home_path: Option<String>,
    provider_count: usize,
    relay_url: Option<String>,
    relay_url_valid: bool,
    port_available: bool,
    bridge_md_readable: bool,
    skills_found: bool,
    errors: Vec<String>,
}

#[tauri::command]
async fn preflight_check(app: tauri::AppHandle) -> Result<PreflightResult, String> {
    let mut errors = Vec::new();

    // 1. Codex home
    let codex_home = DeployManager::find_codex_home();
    let codex_home_found = codex_home.is_some();
    let codex_home_path = codex_home.as_ref().map(|p| p.display().to_string());
    if !codex_home_found {
        errors.push("Codex 配置目录未找到，请确认 Codex CLI 已安装并运行过至少一次".into());
    }

    // 2. 供应商/Relay URL — 首装优先读取已保存的供应商，兼容旧版配置迁移
    let provider_count = codex_home
        .as_ref()
        .and_then(|path| providers::load_or_migrate(path).ok())
        .map(|list| list.len())
        .unwrap_or(0);
    let relay_url_raw = codex_home
        .as_ref()
        .and_then(|path| providers::configured_relay_url(path));
    let relay_url_valid = relay_url_raw
        .as_ref()
        .map(|url| providers::valid_relay_url(url))
        .unwrap_or(false);
    if !relay_url_valid {
        if provider_count == 0 {
            errors.push("尚未配置供应商，请先在供应商页面添加中转站 URL".into());
        } else {
            errors.push("已保存的供应商没有有效中转站 URL，请检查供应商配置".into());
        }
    }
    // 智能探测 /v1 前缀 — 返回规范化后的地址给前端展示
    let relay_url = if let Some(raw) = &relay_url_raw {
        if relay_url_valid {
            Some(probe_api_prefix(raw).await)
        } else {
            relay_url_raw.clone()
        }
    } else {
        None
    };

    // 3. 端口可用性 — bind 成功后立即 drop
    let port_available = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .is_ok();
    if !port_available {
        errors.push("端口 8080 被占用，可能代理已在运行".into());
    }

    // 4. bridge.md 可读性
    let bridge_md_readable = match resolve_resource_file(&app, "bridge.md") {
        Ok(path) => std::fs::read_to_string(&path).is_ok(),
        Err(_) => true, // fallback 到编译期嵌入版本
    };
    if !bridge_md_readable {
        errors.push("bridge.md 文件不可读".into());
    }

    // 5. codex-skills 目录
    let skills_found = resolve_resource_dir(&app, "codex-skills").is_ok();
    if !skills_found {
        errors.push("codex-skills 目录未找到，将仅部署 bridge.md".into());
    }

    Ok(PreflightResult {
        codex_home_found,
        codex_home_path,
        provider_count,
        relay_url,
        relay_url_valid,
        port_available,
        bridge_md_readable,
        skills_found,
        errors,
    })
}

// ── 部署状态查询 ─────────────────────────────────────────

#[tauri::command]
async fn get_deploy_status() -> Result<serde_json::Value, String> {
    match DeployManager::new() {
        Some(manager) => {
            let codex_home = manager.codex_home().display().to_string();
            let status = manager.status();
            Ok(serde_json::json!({
                "codex_home": codex_home,
                "bridge_active": status.bridge_active,
                "bridge_exists": status.bridge_exists,
                "skills_count": status.skills_count,
                "config_backed_up": status.config_backed_up,
                "relay_url_valid": status.relay_url_valid,
                "codex_home_found": true,
                "deployment_state": status.deployment_state,
                "manifest_exists": status.manifest_exists,
                "transaction_pending": status.transaction_pending,
                "integrity_ok": status.integrity_ok,
                "deployment_id": status.deployment_id,
            }))
        }
        None => Ok(serde_json::json!({
            "codex_home_found": false,
        })),
    }
}

#[tauri::command]
async fn preview_deployment(app: tauri::AppHandle) -> Result<crate::deploy::DeployPreview, String> {
    let manager = DeployManager::new().ok_or("Codex home not found")?;
    let skills_dir = resolve_resource_dir(&app, "codex-skills").ok();
    Ok(manager.preview(skills_dir.as_deref()))
}

#[tauri::command]
async fn recover_deployment() -> Result<String, String> {
    let manager = DeployManager::new().ok_or("Codex home not found")?;
    match manager.recover_pending()? {
        Some(message) => Ok(message),
        None => Ok("当前没有待恢复事务".to_string()),
    }
}

#[tauri::command]
async fn set_relay_url(url: String) -> Result<String, String> {
    let trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return Err("Relay URL cannot be empty".into());
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("URL must start with http:// or https://".into());
    }
    let manager = DeployManager::new().ok_or_else(|| "Codex home not found".to_string())?;
    manager.set_relay_url(&trimmed)?;
    Ok(format!("Relay URL saved: {}", trimmed))
}

// ── 供应商管理 ──────────────────────────────────────────

#[tauri::command]
fn list_providers() -> Result<Vec<providers::Provider>, String> {
    providers::list()
}

#[tauri::command]
fn save_provider(provider: providers::Provider) -> Result<Vec<providers::Provider>, String> {
    providers::save(provider)
}

#[tauri::command]
async fn delete_provider(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<providers::Provider>, String> {
    let list = providers::delete(&id)?;
    let mut next_provider = None;

    if let Some(runtime) = state.providers.read().await.as_ref() {
        let mut runtime = runtime.write().await;
        let previous_current = runtime.current().map(|provider| provider.id.clone());
        runtime.providers = list.clone();
        runtime.current_index = previous_current
            .as_ref()
            .and_then(|current| list.iter().position(|provider| &provider.id == current))
            .unwrap_or(0);

        if previous_current.as_deref() == Some(id.as_str()) {
            next_provider = runtime.current().cloned();
        }
    }

    if let Some(provider) = next_provider {
        providers::activate(&provider, true)?;
        if let Some(core) = state.core.read().await.as_ref() {
            core.set_target(provider.normalized_url()).await;
        }
    }

    Ok(list)
}

#[tauri::command]
async fn use_provider(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<providers::Provider>, String> {
    let list = providers::list()?;
    let mut ids = vec![id.clone()];
    ids.extend(list.iter().filter(|p| p.id != id).map(|p| p.id.clone()));
    let ordered = providers::reorder(&ids)?;
    if let Some(provider) = ordered.first() {
        let running = state.core.read().await.is_some();
        // 停止状态下只调整供应商优先级；config.toml/auth.json 的落盘统一
        // 延迟到“启动代理”路径，避免打开应用或点“使用”就改写 Codex 文件。
        if running {
            providers::activate(provider, true)?;
        }
        if let Some(runtime) = state.providers.read().await.as_ref() {
            let mut rt = runtime.write().await;
            rt.providers = ordered.clone();
            rt.current_index = 0;
        }
        if let Some(core) = state.core.read().await.as_ref() {
            core.set_target(provider.normalized_url()).await;
        }
    }
    Ok(ordered)
}

#[tauri::command]
fn reorder_providers(ids: Vec<String>) -> Result<Vec<providers::Provider>, String> {
    providers::reorder(&ids)
}

#[tauri::command]
async fn test_provider(provider: providers::Provider) -> Result<providers::Provider, String> {
    providers::test(&provider).await
}

#[tauri::command]
async fn fetch_provider_models(provider: providers::Provider) -> Result<Vec<String>, String> {
    providers::models(&provider).await
}

#[tauri::command]
async fn get_provider_runtime_status(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    if let Some(runtime) = state.providers.read().await.as_ref() {
        return serde_json::to_value(runtime.read().await.status()).map_err(|e| e.to_string());
    }
    Ok(serde_json::json!({"current": null, "switched": false, "switch_count": 0, "last_error": ""}))
}

// ── Skills 管理 ──────────────────────────────────────────

#[tauri::command]
async fn list_skills(app: tauri::AppHandle) -> Result<Vec<skills::SkillInfo>, String> {
    Ok(skills::scan_skills(&app))
}

#[tauri::command]
async fn toggle_skill(app: tauri::AppHandle, id: String, enabled: bool) -> Result<String, String> {
    skills::toggle_skill(&app, &id, enabled)?;
    skills::sync_enabled_skills(&app)
}

#[tauri::command]
async fn toggle_all_skills(app: tauri::AppHandle, enabled: bool) -> Result<String, String> {
    skills::set_all(enabled);
    skills::sync_enabled_skills(&app)
}

#[tauri::command]
async fn delete_skill_cmd(app: tauri::AppHandle, id: String) -> Result<(), String> {
    skills::delete_skill(&app, &id)
}

// ── 窗口控制 ──────────────────────────────────────────────

#[tauri::command]
async fn minimize_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.minimize().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn toggle_maximize(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let is_max = window.is_maximized().unwrap_or(false);
        if is_max {
            window.unmaximize().map_err(|e| e.to_string())?;
        } else {
            window.maximize().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn close_window(app: tauri::AppHandle) -> Result<(), String> {
    // 点 X 不退出，而是隐藏到托盘
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn show_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    // 退出前恢复 Codex 配置
    if let Some(manager) = DeployManager::new() {
        let status = manager.status();
        if status.config_backed_up || status.bridge_active {
            tracing::info!("quit_app: restoring codex config before exit");
            match manager.restore() {
                Ok(msg) => tracing::info!("quit_app: restore succeeded: {}", msg),
                Err(e) => tracing::error!("quit_app: restore failed: {}", e),
            }
        }
    }
    // 先销毁窗口再退出，避免 Chromium Error 1412
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
    app.exit(0);
    Ok(())
}

// ── SSE 包装 ─────────────────────────────────────────────

/// 将 tamper 替换文本包装为合法的 Responses API SSE 格式
/// Codex CLI 期望: response.created → response.output_text.delta → response.completed
/// 纯文本或 data: [DONE] 都会导致 "stream disconnected before response.completed"
fn wrap_tamper_as_sse(text: &str) -> bytes::Bytes {
    let created = serde_json::json!({
        "type": "response.created",
        "response": {
            "id": "resp_tamper",
            "object": "response",
            "status": "in_progress",
            "output": []
        }
    });

    let delta = serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_tamper",
        "output_index": 0,
        "content_index": 0,
        "delta": text
    });

    let done = serde_json::json!({
        "type": "response.output_text.done",
        "item_id": "msg_tamper",
        "output_index": 0,
        "content_index": 0,
        "text": text
    });

    let completed = serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": "resp_tamper",
            "object": "response",
            "status": "completed",
            "output": [{
                "id": "msg_tamper",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": text
                }]
            }],
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0
            }
        }
    });

    let sse = format!(
        "event: response.created\ndata: {}\n\n\
         event: response.output_text.delta\ndata: {}\n\n\
         event: response.output_text.done\ndata: {}\n\n\
         event: response.completed\ndata: {}\n\n",
        created, delta, done, completed
    );

    bytes::Bytes::from(sse)
}

// ── hop-by-hop 头 ────────────────────────────────────────
//
// 请求方向和响应方向各自跳过 hop-by-hop 头，避免代理干预连接级语义。

/// 请求方向跳过的 hop-by-hop 头名（小写）
#[allow(dead_code)]
fn is_request_hop_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "content-type"
            | "connection"
            | "keep-alive"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// 响应方向跳过的 hop-by-hop 头名（小写）
fn is_response_hop_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "content-encoding"
    )
}

async fn switch_provider(
    core: &Arc<MitmCore>,
    providers: &Arc<RwLock<providers::ProviderRuntime>>,
    app: &tauri::AppHandle,
    reason: String,
) -> bool {
    let next = {
        let mut runtime = providers.write().await;
        runtime.last_error = reason.clone();
        runtime.next()
    };
    if let Some(provider) = next {
        let url = provider.normalized_url();
        core.set_target(url.clone()).await;
        let _ = providers::activate(&provider, true);
        let _ = app.emit(
            "provider-switched",
            serde_json::json!({
                "provider_id": provider.id,
                "provider": provider.name,
                "url": url,
                "reason": reason,
                "automatic": true
            }),
        );
        tracing::warn!("provider auto-switched to {}", provider.name);
        true
    } else {
        false
    }
}

// ── Axum Handlers ─────────────────────────────────────────

async fn health_check() -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::OK,
        [("Content-Type", "text/plain; charset=utf-8")],
        "julong-codex ok",
    )
}

/// 为实时活动面板生成与当前请求关联的展示命令。
/// 该字符串只通过 activity-status 事件发送给 UI，不会交给 shell 执行。
fn activity_command(
    category: &crate::core::Category,
    method: &str,
    path_and_query: &str,
) -> String {
    let route: String = path_and_query
        .split('?')
        .next()
        .unwrap_or("/")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || "/._-".contains(*ch))
        .take(42)
        .collect();
    let route = if route.is_empty() { "/" } else { &route };
    let action = match category.as_str() {
        "crack" => "inspect-license",
        "reverse" => "trace-binary",
        "pentest" => "probe-surface",
        _ => "execute-task",
    };
    format!(
        "$ {} --method {} --route {}",
        action,
        method.to_ascii_uppercase(),
        route
    )
}

async fn handle_proxy(
    req: axum::extract::Request,
    core: Arc<MitmCore>,
    activity: Arc<ActivityTracker>,
    providers: Arc<RwLock<providers::ProviderRuntime>>,
    app: tauri::AppHandle,
) -> axum::response::Response {
    // GET 请求 = 健康检查
    if req.method() == axum::http::Method::GET {
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(axum::body::Body::from("julong-codex ok"))
            .unwrap();
    }

    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return axum::response::Response::builder()
                .status(axum::http::StatusCode::BAD_REQUEST)
                .body(axum::body::Body::from(format!("{{\"error\": \"{}\"}}", e)))
                .unwrap();
        }
    };

    // BUG-1 修复: 保留 query string，上游可能依赖 ?stream=true 等参数
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());

    // 阶段 1: 请求拦截 + 转发上游
    let method = parts.method.clone();
    // 请求刚开始转发即通知活动面板；任务 guard 会在错误、断流或正常完成时统一释放。
    let category = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map(|data| crate::core::categorize(&crate::core::extract_user(&data)))
        .unwrap_or(crate::core::Category::General);
    let category_label = category.to_string();
    let command = activity_command(&category, method.as_str(), &path_and_query);
    let activity_request = activity.start_with_command(category, command);
    let headers = parts.headers.clone();
    let upstream = match core
        .handle_request(
            method.clone(),
            path_and_query.clone(),
            headers.clone(),
            bytes.clone(),
        )
        .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Proxy error (request phase): {}", e);
            let switched = switch_provider(&core, &providers, &app, e.to_string()).await;
            if switched {
                match core
                    .handle_request(method, path_and_query, headers, bytes)
                    .await
                {
                    Ok(u) => u,
                    Err(retry_error) => {
                        drop(activity_request);
                        return axum::response::Response::builder()
                            .status(axum::http::StatusCode::BAD_GATEWAY)
                            .header("Content-Type", "application/json")
                            .body(axum::body::Body::from(format!(
                                "{{\"error\": \"{}\"}}",
                                retry_error
                            )))
                            .unwrap();
                    }
                }
            } else {
                drop(activity_request);
                return axum::response::Response::builder()
                    .status(axum::http::StatusCode::BAD_GATEWAY)
                    .header("Content-Type", "application/json")
                    .body(axum::body::Body::from(format!("{{\"error\": \"{}\"}}", e)))
                    .unwrap();
            }
        }
    };
    activity.update_phase(&category_label, "读取响应", 30);

    let status =
        axum::http::StatusCode::from_u16(upstream.status).unwrap_or(axum::http::StatusCode::OK);
    let content_type = upstream.content_type.clone();
    let is_sse = content_type
        .as_deref()
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    // 活动面板只通过 Tauri 事件观察请求；不得向 Codex 响应流注入 keepalive 或其他字节。
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<bytes::Bytes, std::io::Error>>();
    let core_clone = core.clone();
    let activity_for_tamper = activity.clone();
    let activity_for_progress = activity.clone();
    let activity_category = category_label.clone();
    let meta = upstream.meta;
    let upstream_status = upstream.status;
    if upstream_status == 401
        || upstream_status == 403
        || upstream_status == 429
        || upstream_status >= 500
    {
        let _ = switch_provider(&core, &providers, &app, format!("HTTP {}", upstream_status)).await;
    }
    let ct_for_finalize = content_type.clone();
    let upstream_resp = upstream.response;
    // BUG-5 修复: 保留上游响应头，过滤 hop-by-hop 后透传给 CLI
    let upstream_headers = upstream.headers;

    tokio::spawn(async move {
        // ActiveRequest 的 Drop 覆盖成功、上游异常和客户端断开的全部退出路径。
        let _activity_request = activity_request;
        let mut accumulated = Vec::with_capacity(65536);
        let mut stream = upstream_resp.bytes_stream();

        // 先完整读取上游响应，再交给既有响应拦截器。活动监控只复制长度和阶段信息，
        // 不提前结束上游 stream，也不向下游写入任何额外内容。
        let mut stream_error = None;
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    accumulated.extend_from_slice(&chunk);
                    let progress = 36u8.saturating_add((accumulated.len() / 4096).min(50) as u8);
                    let phase = if is_sse {
                        "流式处理"
                    } else {
                        "读取响应"
                    };
                    activity_for_progress.update_phase(&activity_category, phase, progress);
                }
                Err(error) => {
                    stream_error = Some(error.to_string());
                    activity_for_progress.update_phase(&activity_category, "上游读取失败", 82);
                    break;
                }
            }
        }

        // 不要把上游断流时已收到的半截 body 当作正常响应交给 Codex；这正是
        // “incomplete utf-8 byte sequence” 的典型来源。让 HTTP 流自然结束，
        // 同时保留真实错误到日志，避免监控层伪造响应。
        if let Some(error) = stream_error {
            tracing::warn!(error = %error, bytes = accumulated.len(), "upstream response stream aborted");
            drop(tx);
            return;
        }

        let accumulated_bytes = bytes::Bytes::from(accumulated);
        activity_for_progress.update_phase(&activity_category, "解析响应", 92);

        // BUG-6 修复: duration_ms 从请求到达时开始计算（含上游等待 + body streaming）
        let duration_ms = meta
            .timestamp
            .signed_duration_since(chrono::Utc::now())
            .num_milliseconds()
            .unsigned_abs() as u64;

        tracing::debug!(
            category = %meta.category,
            status = upstream_status,
            resp_bytes = accumulated_bytes.len(),
            duration_ms,
            "buffering completed, running finalize"
        );

        // 阶段 2: 解析 + 响应拦截器 (tamper/memory/monitor)
        let (final_body, tampered, _ct) = core_clone.finalize_response(
            meta,
            upstream_status,
            ct_for_finalize,
            accumulated_bytes,
            duration_ms,
        );

        // 阶段 3: 发送最终 body 给 Codex CLI
        // 篡改发生在响应阶段，额外创建一个 tampered guard，使第四个机器人
        // 在真实响应替换期间进入“敲击中”状态；请求结束后 guard 自动释放。
        let _tampered_activity = tampered.then(|| activity_for_tamper.start_tampered());
        activity_for_progress.update_phase(&activity_category, "交付 Codex", 97);
        if tampered {
            if is_sse {
                let replacement_text =
                    std::str::from_utf8(&final_body).unwrap_or("「了解。実行する。」");
                let sse_body = wrap_tamper_as_sse(replacement_text);
                tracing::info!(
                    bytes = sse_body.len(),
                    "tamper: sending SSE-wrapped replacement to CLI"
                );
                let _ = tx.send(Ok(sse_body));
            } else {
                tracing::info!(
                    bytes = final_body.len(),
                    "tamper: sending replaced body to CLI"
                );
                let _ = tx.send(Ok(final_body));
            }
        } else {
            let _ = tx.send(Ok(final_body));
        }

        drop(tx);
    });

    // 构建 axum 响应
    let body_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    let body = axum::body::Body::from_stream(body_stream);

    let mut resp_builder = axum::response::Response::builder().status(status);

    // BUG-5 修复: 转发上游响应头（过滤 hop-by-hop）
    for (name, value) in upstream_headers.iter() {
        let lower = name.as_str().to_lowercase();
        if is_response_hop_header(&lower) {
            continue;
        }
        // content-type 由下方 is_sse 逻辑统一处理
        if lower == "content-type" {
            continue;
        }
        resp_builder = resp_builder.header(name, value);
    }

    if is_sse {
        resp_builder = resp_builder.header("content-type", "text/event-stream");
    } else if let Some(ct) = &content_type {
        resp_builder = resp_builder.header("content-type", ct);
    }
    resp_builder.body(body).unwrap()
}

// ── 资源文件查找 ─────────────────────────────────────────

fn find_resource_file(name: &str) -> Result<std::path::PathBuf, String> {
    let candidates = collect_candidates(name);
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    Err(format!("{} not found (searched: {:?})", name, candidates))
}

fn find_resource_dir(name: &str) -> Result<std::path::PathBuf, String> {
    let candidates = collect_candidates(name);
    for p in &candidates {
        if p.is_dir() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "{} directory not found (searched: {:?})",
        name, candidates
    ))
}

/// 生成所有候选路径：CWD、exe 目录、以及它们的上级（覆盖 dev / cargo run / raw exe 场景）
fn collect_candidates(name: &str) -> Vec<std::path::PathBuf> {
    let mut candidates = vec![
        std::path::PathBuf::from(name),
        std::env::current_dir().unwrap_or_default().join(name),
    ];

    // CWD 上级：cargo run 时 cwd=src-tauri/，资源在项目根目录
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(name));
        }
        // src-tauri/resources/（beforeBuildCommand 复制的资源）
        candidates.push(cwd.join("resources").join(name));
    }

    // exe 目录及其上级：raw exe 从 target/release/ 运行时向上搜索
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join(name));
            if let Some(target_dir) = exe_dir.parent() {
                // target/
                candidates.push(target_dir.join(name));
                if let Some(src_tauri_dir) = target_dir.parent() {
                    // src-tauri/
                    candidates.push(src_tauri_dir.join(name));
                    // src-tauri/resources/
                    candidates.push(src_tauri_dir.join("resources").join(name));
                    // 项目根目录
                    if let Some(project_root) = src_tauri_dir.parent() {
                        candidates.push(project_root.join(name));
                    }
                }
            }
        }
    }

    candidates
}

/// 可写数据目录 (memory.json / tamper-rules.txt 所在处)
/// Dev: 项目根；macOS Release: Application Support；其他 Release: exe 同级目录
fn data_dir() -> std::path::PathBuf {
    let dir = if cfg!(debug_assertions) {
        std::env::current_dir()
            .ok()
            .and_then(|d| d.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    } else {
        #[cfg(target_os = "macos")]
        if let Some(home) = std::env::var_os("HOME") {
            let dir = std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Super-Instruct");
            let _ = std::fs::create_dir_all(&dir);
            return dir;
        }

        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn data_dir_path(file: &str) -> String {
    data_dir().join(file).to_string_lossy().to_string()
}

// ── 外部篡改规则字典 ─────────────────────────────────────

fn tamper_rules_path() -> std::path::PathBuf {
    data_dir().join("tamper-rules.txt")
}

/// 从 tamper-rules.txt 加载规则；每行一条正则，# 开头为注释，空行忽略。
/// 文件不存在或无有效规则时返回 Err（调用方回退到内置规则）。
fn load_tamper_rules() -> Result<TamperEngine, String> {
    let path = tamper_rules_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {} failed: {}", path.display(), e))?;
    let patterns: Vec<String> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    if patterns.is_empty() {
        return Err("no valid rules in file".into());
    }
    let engine = TamperEngine::with_patterns_and_source(patterns, path.display().to_string());
    if engine.rule_count() == 0 {
        return Err("all regexes failed to compile".into());
    }
    Ok(engine)
}

/// 将内置默认规则导出到 tamper-rules.txt（用户可在此基础上增删）
#[tauri::command]
fn export_tamper_rules() -> Result<String, String> {
    let path = tamper_rules_path();
    if path.exists() {
        return Err(format!("{} 已存在，不覆盖", path.display()));
    }
    let content = crate::extensions::tamper::default_tamper_patterns().join("\n");
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    tracing::info!(
        "export_tamper_rules: wrote {} rules to {}",
        crate::extensions::tamper::default_tamper_patterns().len(),
        path.display()
    );
    Ok(path.display().to_string())
}

#[tauri::command]
fn get_tamper_rules() -> Result<serde_json::Value, String> {
    let path = tamper_rules_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let rules: Vec<&str> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            Ok(serde_json::json!({
                "external": true,
                "path": path.display().to_string(),
                "count": rules.len(),
                "content": content,
            }))
        }
        Err(_) => {
            let rules = crate::extensions::tamper::default_tamper_patterns();
            Ok(serde_json::json!({
                "external": false,
                "path": path.display().to_string(),
                "count": rules.len(),
                "content": rules.join("\n"),
            }))
        }
    }
}

/// 通过 Tauri 资源解析查找文件（release），fallback 到手动搜索（dev）
fn resolve_resource_file(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    // Release: Tauri 打包资源目录 (resources/)
    if let Ok(base) = app.path().resource_dir() {
        let p = base.join(name);
        if p.exists() {
            return Ok(p);
        }
    }
    // Dev fallback
    find_resource_file(name)
}

/// 通过 Tauri 资源解析查找目录（release），fallback 到手动搜索（dev）
fn resolve_resource_dir(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    if let Ok(base) = app.path().resource_dir() {
        let p = base.join(name);
        if p.is_dir() {
            return Ok(p);
        }
    }
    find_resource_dir(name)
}
