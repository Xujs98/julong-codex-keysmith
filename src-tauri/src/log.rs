// 日志模块 — 控制台 + 文件双输出，每次运行生成独立日志
// 文件写入 logs/ 目录，文件名格式 super-instruct-YYYY-MM-DD_HH-MM-SS.log
// 返回 WorkerGuard，调用方必须保活以刷新缓冲区

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// 初始化日志系统：控制台 + 每次运行独立文件
/// 返回的 WorkerGuard 必须在应用整个生命周期内保活，否则尾部日志可能丢失
pub fn init_logging(log_dir: &str) -> WorkerGuard {
    std::fs::create_dir_all(log_dir).ok();

    // 每次启动生成独立日志文件，带时间戳避免冲突
    let filename = format!(
        "super-instruct-{}.log",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let log_path = Path::new(log_dir).join(&filename);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .expect("failed to create log file");

    let (file_writer, guard) = tracing_appender::non_blocking(file);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,julong_codex_keysmith=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_ansi(true),
        )
        .with(
            fmt::layer()
                .with_writer(file_writer)
                .with_target(true)
                .with_level(true)
                .with_ansi(false),
        )
        .init();

    tracing::info!(
        "logging initialized: console + file ({}{})",
        log_dir,
        path_suffix(log_dir)
    );
    guard
}

fn path_suffix(dir: &str) -> &'static str {
    if !dir.is_empty() {
        let p = Path::new(dir);
        if p.is_absolute() || dir.ends_with('/') || dir.ends_with('\\') {
            return "";
        }
    }
    "/"
}
