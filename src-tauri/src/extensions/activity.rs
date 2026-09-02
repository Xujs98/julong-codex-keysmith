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
}

pub struct ActivityTracker {
    app_handle: AppHandle,
    active: Mutex<BTreeMap<String, u64>>,
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
        self.start_label(&category.to_string())
    }

    /// 响应被篡改时单独标记一个短生命周期活动，让“已篡改”机器人也能真实联动。
    pub fn start_tampered(self: &Arc<Self>) -> ActiveRequest {
        self.start_label("tampered")
    }

    fn start_label(self: &Arc<Self>, category: &str) -> ActiveRequest {
        let category = category.to_string();
        {
            let mut active = self.active.lock().unwrap();
            *active.entry(category.clone()).or_insert(0) += 1;
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
            if let Some(count) = active.get_mut(category) {
                *count = count.saturating_sub(1);
                if *count == 0 {
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
        let active_count = active.values().sum();
        drop(active);
        let _ = self.app_handle.emit(
            "activity-status",
            ActivityStatus {
                status: status.to_string(),
                category: category.to_string(),
                active_categories,
                active_count,
            },
        );
    }
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.tracker.finish(&self.category);
    }
}
