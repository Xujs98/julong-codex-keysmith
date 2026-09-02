// M5: MemoryKernel — 从成功操作中学习，持久化到 memory.json
// 自门控: 被篡改或回复太短则跳过

use crate::core::{Category, ResponseCtx, ResponseInterceptor};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct MemoryKernel {
    file: PathBuf,
    data: Mutex<MemoryData>,
}

#[derive(Serialize, Deserialize, Default)]
struct MemoryData {
    successes: Vec<SuccessRecord>,
    patterns: std::collections::HashMap<String, u64>,
    techniques: std::collections::HashMap<String, u64>,
    stats: Stats,
}

#[derive(Serialize, Deserialize, Clone)]
struct SuccessRecord {
    ts: String,
    category: String,
    user: String,
    result: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Stats {
    total: u64,
    crack: u64,
    reverse: u64,
    pentest: u64,
    tamper: u64,
}

impl MemoryKernel {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let data = load_memory(&file);
        Self {
            file,
            data: Mutex::new(data),
        }
    }

    pub fn stats(&self) -> Stats {
        self.data.lock().unwrap().stats.clone()
    }

    pub fn success_count(&self) -> u64 {
        self.data.lock().unwrap().stats.total
    }
}

impl ResponseInterceptor for MemoryKernel {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn intercept(&self, ctx: &mut ResponseCtx) {
        // 自门控: 被篡改 → 跳过; 回复太短 → 跳过
        if ctx.modified_body.is_some() {
            tracing::trace!("memory: skipped (response was tampered)");
            return;
        }
        if ctx.parsed.reply.len() <= 50 {
            tracing::trace!(
                "memory: skipped (reply too short: {} chars)",
                ctx.parsed.reply.len()
            );
            return;
        }

        let mut data = self.data.lock().unwrap();

        data.successes.push(SuccessRecord {
            ts: ctx.meta.timestamp.to_rfc3339(),
            category: ctx.meta.category.to_string(),
            user: ctx.meta.user_msg.chars().take(200).collect(),
            result: ctx.parsed.reply.chars().take(300).collect(),
        });

        data.stats.total += 1;
        match ctx.meta.category {
            Category::Crack => data.stats.crack += 1,
            Category::Reverse => data.stats.reverse += 1,
            Category::Pentest => data.stats.pentest += 1,
            Category::General => {}
        }

        tracing::debug!(
            category = %ctx.meta.category,
            total = data.stats.total,
            "memory: recorded successful interaction"
        );

        // 提取词汇频率
        let words: std::collections::HashSet<&str> = ctx.meta.user_msg.split_whitespace().collect();
        for w in words {
            let key = w.to_lowercase();
            *data.patterns.entry(key).or_insert(0) += 1;
        }

        save_memory(&self.file, &data);
    }
}

fn load_memory(file: &PathBuf) -> MemoryData {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_memory(file: &PathBuf, data: &MemoryData) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(file, json);
    }
}
