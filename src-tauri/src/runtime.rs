//! 桌面端和 CLI 共享的本地代理运行状态。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const PROXY_ADDRESS: &str = "127.0.0.1:8080";
pub const PID_FILE: &str = "julong-codex-proxy.pid";

pub fn pid_path(codex_home: &Path) -> PathBuf {
    codex_home.join(PID_FILE)
}

pub fn read_pid(codex_home: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_path(codex_home))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn managed_proxy_pid(codex_home: &Path) -> Option<u32> {
    let pid = read_pid(codex_home)?;
    if pid_is_managed_proxy(pid) {
        Some(pid)
    } else {
        remove_pid_file(codex_home);
        None
    }
}

pub fn write_pid(codex_home: &Path, pid: u32) -> Result<(), String> {
    std::fs::write(pid_path(codex_home), pid.to_string())
        .map_err(|e| format!("写入代理 PID 失败: {e}"))
}

pub fn remove_pid_file(codex_home: &Path) {
    let _ = std::fs::remove_file(pid_path(codex_home));
}

pub fn port_is_listening() -> bool {
    std::net::TcpStream::connect_timeout(
        &PROXY_ADDRESS.parse().expect("valid loopback address"),
        Duration::from_millis(250),
    )
    .is_ok()
}

pub fn proxy_is_healthy() -> bool {
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &PROXY_ADDRESS.parse().expect("valid loopback address"),
        Duration::from_millis(500),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
    if stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).is_ok() && response.contains("julong-codex ok")
}

pub fn wait_for_port(expected: bool, timeout: Duration) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if port_is_listening() == expected {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    port_is_listening() == expected
}

pub fn terminate_managed_proxy(codex_home: &Path) -> Result<bool, String> {
    let Some(pid) = read_pid(codex_home) else {
        return Ok(false);
    };
    if !pid_exists(pid) {
        remove_pid_file(codex_home);
        return Ok(false);
    }
    if !pid_is_managed_proxy(pid) {
        remove_pid_file(codex_home);
        return Err(format!(
            "PID {pid} 与矩龙代理进程不匹配，已清理过期 PID 文件"
        ));
    }
    terminate_pid(pid)?;
    remove_pid_file(codex_home);
    Ok(true)
}

#[cfg(unix)]
fn pid_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn pid_exists(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            output.status.success() && text.contains(&format!("\",\"{pid}\",\""))
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn pid_is_managed_proxy(pid: u32) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let command = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    command.contains("julong-codex") && command.contains("--proxy-daemon")
}

#[cfg(windows)]
fn pid_is_managed_proxy(pid: u32) -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .to_ascii_lowercase()
            .contains("julong-codex")
}

fn terminate_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|e| format!("停止代理进程失败: {e}"))?;

    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|e| format!("停止代理进程失败: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("停止代理进程返回状态 {status}"))
    }
}
