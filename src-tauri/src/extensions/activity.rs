// ActivityTracker — 将代理内实际执行中的请求推送给仪表盘机器人。

use crate::core::Category;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use tokio::time::{sleep, Duration};

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
    /// 每个机器人当前请求的阶段、里程碑进度和已运行时长。
    pub tasks: BTreeMap<String, ActivityTaskState>,
}

#[derive(Clone, Serialize)]
pub struct ActivityTaskState {
    pub phase: String,
    pub progress: u8,
    pub elapsed_ms: u64,
}

impl ActivityStatus {
    pub fn idle() -> Self {
        Self {
            status: "idle".to_string(),
            category: "general".to_string(),
            active_categories: Vec::new(),
            active_count: 0,
            command: String::new(),
            commands: BTreeMap::new(),
            tasks: BTreeMap::new(),
        }
    }
}

struct ActiveTask {
    count: u64,
    command: String,
    phase: String,
    progress: u8,
    started_at: Instant,
}

pub struct ActivityTracker {
    active: Mutex<BTreeMap<String, ActiveTask>>,
    heartbeat_running: AtomicBool,
    event_tx: watch::Sender<ActivityStatus>,
}

pub struct ActiveRequest {
    tracker: Arc<ActivityTracker>,
    category: String,
}

impl ActivityTracker {
    pub fn new(app_handle: AppHandle) -> Self {
        let (event_tx, mut event_rx) = watch::channel(ActivityStatus::idle());
        tokio::spawn(async move {
            while event_rx.changed().await.is_ok() {
                let snapshot = event_rx.borrow().clone();
                let _ = app_handle.emit("activity-status", snapshot);
            }
        });

        Self {
            active: Mutex::new(BTreeMap::new()),
            heartbeat_running: AtomicBool::new(false),
            event_tx,
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
                    phase: "解析任务".to_string(),
                    progress: 5,
                    started_at: Instant::now(),
                });
            task.count += 1;
            task.command = command;
        }
        self.ensure_heartbeat();
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

    pub fn snapshot(&self) -> ActivityStatus {
        let category = self
            .active
            .lock()
            .unwrap()
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "general".to_string());
        let status = if self.active.lock().unwrap().is_empty() {
            "idle"
        } else {
            "running"
        };
        self.status_snapshot(status, &category)
    }

    /// 更新当前类别的真实代理处理阶段。进度代表代理生命周期里程碑，
    /// 只用于界面状态展示，不会触发额外的本机命令。
    pub fn update_phase(&self, category: &str, phase: impl Into<String>, progress: u8) {
        let mut active = self.active.lock().unwrap();
        if let Some(task) = active.get_mut(category) {
            task.phase = phase.into();
            task.progress = progress.min(99);
        } else {
            return;
        }
        drop(active);
        self.emit_running(category);
    }

    fn ensure_heartbeat(self: &Arc<Self>) {
        if self
            .heartbeat_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(500)).await;
                let Some(tracker) = weak.upgrade() else { break };

                let category = {
                    let mut active = tracker.active.lock().unwrap();
                    if active.is_empty() {
                        None
                    } else {
                        for task in active.values_mut() {
                            let elapsed_seconds = task.started_at.elapsed().as_secs() as u8;
                            let heartbeat_progress = 8u8.saturating_add(elapsed_seconds.min(80));
                            task.progress = task.progress.max(heartbeat_progress).min(90);
                            if task.progress < 25 {
                                task.phase = "连接上游".to_string();
                            } else if task.progress < 55 {
                                task.phase = "读取响应".to_string();
                            } else if task.progress < 88 {
                                task.phase = "流式处理".to_string();
                            }
                        }
                        active.keys().next().cloned()
                    }
                };

                let Some(category) = category else {
                    tracker.heartbeat_running.store(false, Ordering::Release);
                    // start_label 可能刚好在上一个检查后加入新任务，重新接管心跳。
                    if !tracker.active.lock().unwrap().is_empty()
                        && tracker
                            .heartbeat_running
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        continue;
                    }
                    break;
                };
                tracker.emit_running(&category);
            }
        });
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
        let snapshot = self.status_snapshot(status, category);
        // watch 只保留最新状态，UI 事件发送在独立任务中执行，不会阻塞代理请求。
        let _ = self.event_tx.send(snapshot);
    }

    fn status_snapshot(&self, status: &str, category: &str) -> ActivityStatus {
        let active = self.active.lock().unwrap();
        let active_categories: Vec<String> = active.keys().cloned().collect();
        let has_active = !active_categories.is_empty();
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
        let tasks: BTreeMap<String, ActivityTaskState> = active
            .iter()
            .map(|(label, task)| {
                (
                    label.clone(),
                    ActivityTaskState {
                        phase: task.phase.clone(),
                        progress: task.progress,
                        elapsed_ms: task.started_at.elapsed().as_millis() as u64,
                    },
                )
            })
            .collect();
        ActivityStatus {
            status: if has_active { status } else { "idle" }.to_string(),
            category: if has_active { category } else { "general" }.to_string(),
            active_categories,
            active_count,
            command,
            commands,
            tasks,
        }
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
