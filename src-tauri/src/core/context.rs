// Core context types — 管道各阶段的数据载体

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::HeaderMap;

#[derive(Clone, Debug)]
pub enum Category {
    Crack,
    Reverse,
    Pentest,
    General,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Crack => "crack",
            Category::Reverse => "reverse",
            Category::Pentest => "pentest",
            Category::General => "general",
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 请求阶段元数据 — 从用户消息中提取
#[derive(Clone)]
pub struct RequestMeta {
    pub user_msg: String,
    pub category: Category,
    pub path: String,
    pub timestamp: DateTime<Utc>,
}

/// 请求上下文 — 请求拦截器操作的目标
pub struct RequestCtx {
    pub meta: RequestMeta,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

/// 响应解析结果 — ResponseParser 输出
pub struct ParsedResponse {
    pub thinking: String,
    pub reply: String,
}

/// 响应上下文 — 响应拦截器操作的目标
pub struct ResponseCtx {
    pub meta: RequestMeta,
    pub status: u16,
    pub raw_body: Bytes,
    pub parsed: ParsedResponse,
    /// 设置此项来替换响应体
    pub modified_body: Option<Bytes>,
    pub duration_ms: u64,
}
