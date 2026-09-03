// ActivityTracker — 将代理内实际执行中的请求推送给仪表盘机器人。

use crate::core::Category;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct ActivityStatus {
    pub status: String,
    pub category: String,
    pub active_categories: Vec<String>,
    pub active_count: u64,
    /// 当前活动类别对应的模拟命令，仅用于仪表盘实时展示，不会在本机执行。
    pub command: String,
    /// 同时运行多个请求时，为每个机器人保留各自的展示命令。
    pub commands: BTreeMap<String, String>,
}

struct ActiveTask {
    count: u64,
    command: String,
}

pub struct ActivityTracker {
    app_handle: AppHandle,
    active: Mutex<BTreeMap<String, ActiveTask>>,
}

pub struct ActiveRequest {
    tracker: Arc<ActivityTracker>,
    category: String,
}

impl ActivityTracker {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            active: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn start(self: &Arc<Self>, category: Category) -> ActiveRequest {
        let label = category.to_string();
        self.start_label(&label, default_command(&label))
    }

    /// 记录请求对应的展示命令。命令只会作为 activity-status 事件发送给前端。
    pub fn start_with_command(
        self: &Arc<Self>,
        category: Category,
        command: impl Into<String>,
    ) -> ActiveRequest {
        let label = category.to_string();
        self.start_label(&label, &command.into())
    }

    /// 响应被篡改时单独标记一个短生命周期活动，让“已篡改”机器人也能真实联动。
    pub fn start_tampered(self: &Arc<Self>) -> ActiveRequest {
        self.start_label("tampered", "$ mutate --response --rules active")
    }

    fn start_label(self: &Arc<Self>, category: &str, command: &str) -> ActiveRequest {
        let category = category.to_string();
        let command = normalize_command(command, default_command(&category));
        {
            let mut active = self.active.lock().unwrap();
            let task = active
                .entry(category.clone())
                .or_insert_with(|| ActiveTask {
                    count: 0,
                    command: command.clone(),
                });
            task.count += 1;
            task.command = command;
        }
        self.emit_running(&category);
        ActiveRequest {
            tracker: self.clone(),
            category,
        }
    }

    pub fn reset(&self) {
        self.active.lock().unwrap().clear();
        self.emit("idle", "general");
    }

    fn finish(&self, category: &str) {
        {
            let mut active = self.active.lock().unwrap();
            if let Some(task) = active.get_mut(category) {
                task.count = task.count.saturating_sub(1);
                if task.count == 0 {
                    active.remove(category);
                }
            }
        }

        let has_active = !self.active.lock().unwrap().is_empty();
        if has_active {
            self.emit_running(category);
        } else {
            self.emit("idle", category);
        }
    }

    fn emit_running(&self, category: &str) {
        self.emit("running", category);
    }

    fn emit(&self, status: &str, category: &str) {
        let active = self.active.lock().unwrap();
        let active_categories: Vec<String> = active.keys().cloned().collect();
        let active_count = active.values().map(|task| task.count).sum();
        let commands: BTreeMap<String, String> = active
            .iter()
            .map(|(label, task)| (label.clone(), task.command.clone()))
            .collect();
        let command = commands
            .get(category)
            .cloned()
            .or_else(|| commands.values().next().cloned())
            .unwrap_or_default();
        drop(active);
        let _ = self.app_handle.emit(
            "activity-status",
            ActivityStatus {
                status: status.to_string(),
                category: category.to_string(),
                active_categories,
                active_count,
                command,
                commands,
            },
        );
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.tracker.finish(&self.category);
    }
}

fn default_command(category: &str) -> &'static str {
    match category {
        "crack" => "$ inspect --mode crack --target active",
        "reverse" => "$ trace --mode reverse --target active",
        "pentest" => "$ probe --mode pentest --target active",
        "tampered" => "$ mutate --response --rules active",
        _ => "$ codex --execute --target active",
    }
}

fn normalize_command(command: &str, fallback: &str) -> String {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized.chars().take(120).collect()
    }
}
