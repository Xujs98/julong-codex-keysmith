// MitmCore — 格式无关、功能无关的 HTTP 反向代理管道
// 两阶段流式架构:
//   1. handle_request  — 请求拦截器 → 转发上游 → 返回 reqwest::Response (流式)
//   2. finalize_response — 解析 → 响应拦截器 → 返回最终 body (后处理)
// axum handler 负责流式透传 + 背景累积 + 后处理调用

pub mod context;
pub mod extract;
pub mod traits;

pub use context::{Category, ParsedResponse, RequestCtx, RequestMeta, ResponseCtx};
pub use extract::{categorize, extract_user};
pub use traits::{RequestInterceptor, ResponseInterceptor, ResponseParser};

use bytes::Bytes;
use http::{HeaderMap, Method};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MitmCore {
    activation: Option<std::sync::Mutex<crate::activation::ActivationGate>>,
    target: Arc<RwLock<UpstreamTarget>>,
    client: Client,
    request_interceptors: Vec<Box<dyn RequestInterceptor>>,
    response_parser: Box<dyn ResponseParser>,
    response_interceptors: Vec<Box<dyn ResponseInterceptor>>,
}

struct UpstreamTarget {
    url: String,
    anthropic_api_key: Option<String>,
    claude: Option<(String, String)>,
    split_claude: bool,
    openai_available: bool,
}

/// 阶段 1 产物: 请求拦截后的元数据 + 上游响应
pub struct UpstreamResult {
    pub meta: RequestMeta,
    pub status: u16,
    pub content_type: Option<String>,
    pub headers: HeaderMap,
    pub response: reqwest::Response,
}

impl MitmCore {
    pub fn builder() -> MitmCoreBuilder {
        MitmCoreBuilder::new()
    }

    pub fn local_response(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
    ) -> Option<axum::response::Response> {
        let (headers, path) = crate::claude_desktop::normalize_request(headers, path);
        let data = serde_json::from_slice(body).ok()?;
        self.activation
            .as_ref()?
            .lock()
            .ok()?
            .local_response(&headers, &data, &path)
    }

    pub async fn set_target(&self, target: impl Into<String>) {
        self.target.write().await.url = target.into();
    }

    pub async fn set_provider(&self, target: impl Into<String>, api_key: &str) {
        *self.target.write().await = UpstreamTarget {
            url: target.into(),
            anthropic_api_key: Some(api_key.into()),
            claude: None,
            split_claude: false,
            openai_available: true,
        };
    }

    pub async fn update_category(&self, provider: &crate::providers::Provider) {
        let mut target = self.target.write().await;
        if provider.is_claude() {
            target.claude = Some((provider.normalized_url(), provider.api_key.clone()));
            target.split_claude = true;
        } else {
            target.url = provider.normalized_url();
            target.openai_available = true;
        }
    }
    pub async fn clear_openai_provider(&self) {
        self.target.write().await.openai_available = false;
    }
    pub async fn clear_claude_provider(&self) {
        let mut target = self.target.write().await;
        target.claude = None;
        target.split_claude = true;
    }

    /// 阶段 1: 请求拦截 → 转发上游 → 返回流式响应
    /// 调用方拿到返回的 reqwest::Response 后做流式透传
    pub async fn handle_request(
        &self,
        method: Method,
        path_and_query: String,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<UpstreamResult, Box<dyn std::error::Error + Send + Sync>> {
        let (headers, path_and_query) =
            crate::claude_desktop::normalize_request(&headers, &path_and_query);
        // Model discovery and other GET requests carry no JSON body.
        let data: serde_json::Value = if body.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&body)?
        };
        let user_msg = extract_user(&data);
        let category = categorize(&user_msg);

        tracing::debug!(
            category = %category,
            path = %path_and_query,
            method = %method,
            user_msg_len = user_msg.len(),
            "request received"
        );

        let mut req_ctx = RequestCtx {
            meta: RequestMeta {
                user_msg,
                category,
                path: path_and_query.clone(),
                timestamp: chrono::Utc::now(),
            },
            headers: headers.clone(),
            body: data,
        };

        let active = match &self.activation {
            Some(gate) => gate.lock().map_err(|e| e.to_string())?.inject(
                &req_ctx.headers,
                &mut req_ctx.body,
                &path_and_query,
            ),
            None => true,
        };

        // 2. 请求拦截器 — 会话启用后执行
        for ext in self.request_interceptors.iter().filter(|_| active) {
            tracing::trace!(interceptor = ext.name(), "request interceptor running");
            ext.intercept(&mut req_ctx);
        }

        // 3. 转发到上游 — 跳过 hop-by-hop 头
        let target = self.target.read().await;
        let claude_request = crate::claude::messages_path(&path_and_query)
            || headers
                .get("x-julong-environment")
                .and_then(|h| h.to_str().ok())
                == Some("claude-desktop");
        if !claude_request && !target.openai_available {
            return Err("尚未配置 OpenAI / Codex 分类供应商".into());
        }
        let (url, anthropic_key) = if claude_request && target.split_claude {
            let (url, key) = target
                .claude
                .as_ref()
                .ok_or("尚未配置 Claude 分类供应商，请在供应商页面添加后选择使用")?;
            (upstream_url(url, &path_and_query), Some(key.clone()))
        } else {
            (
                upstream_url(&target.url, &path_and_query),
                target.anthropic_api_key.clone(),
            )
        };
        drop(target);
        tracing::debug!(url = %url, "forwarding to upstream");

        let mut forward_headers = HeaderMap::new();
        for (name, value) in headers.iter() {
            let lower = name.as_str().to_lowercase();
            // 跳过 hop-by-hop 和需要重新计算的头部
            // accept-encoding: 强制上游返回未压缩数据，否则 SSE 解析器无法读取压缩字节
            if lower == "host"
                || lower == "content-length"
                || lower == "content-type"
                || lower == "accept-encoding"
                || lower.starts_with("x-julong-")
            {
                continue;
            }
            forward_headers.insert(name.clone(), value.clone());
        }

        if crate::claude::messages_path(&path_and_query)
            || headers
                .get("x-julong-environment")
                .and_then(|h| h.to_str().ok())
                == Some("claude-desktop")
        {
            if let Some(key) = anthropic_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                // Anthropic uses x-api-key; compatible relays may use Bearer.
                // Replace both, never forward a stale client key to a new provider.
                forward_headers.insert("x-api-key", http::HeaderValue::from_str(key)?);
                forward_headers.insert(
                    "authorization",
                    http::HeaderValue::from_str(&format!("Bearer {key}"))?,
                );
            } else if anthropic_key.is_some()
                || headers
                    .get("x-julong-environment")
                    .and_then(|h| h.to_str().ok())
                    == Some("claude-desktop")
                || forward_headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    == Some(&format!("Bearer {}", crate::claude::LOCAL_TOKEN))
            {
                return Err(
                    "Claude Code 接入缺少供应商 API Key，请在供应商页面配置后重启代理".into(),
                );
            }
        }

        let is_get = method == Method::GET || method == Method::HEAD;
        let request = self.client.request(method, &url).headers(forward_headers);
        let request = if is_get {
            request
        } else {
            request.json(&req_ctx.body)
        };
        let resp = request.send().await?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        tracing::debug!(status, "upstream response headers received");

        let headers = resp.headers().clone();

        Ok(UpstreamResult {
            meta: req_ctx.meta,
            status,
            content_type,
            headers,
            response: resp,
        })
    }

    /// 阶段 2: 流结束后 — 解析响应 + 运行响应拦截器
    /// 返回 (最终 body, 是否被篡改, content_type)
    pub fn finalize_response(
        &self,
        meta: RequestMeta,
        status: u16,
        content_type: Option<String>,
        accumulated: Bytes,
        duration_ms: u64,
    ) -> (Bytes, bool, Option<String>) {
        // 4. 响应解析
        let parsed = self.response_parser.parse(&accumulated);

        tracing::debug!(
            thinking_len = parsed.thinking.len(),
            reply_len = parsed.reply.len(),
            "response parsed"
        );

        // 5. 响应拦截器 — 全量执行, 自门控
        let mut resp_ctx = ResponseCtx {
            meta,
            status,
            raw_body: accumulated.clone(),
            parsed,
            modified_body: None,
            duration_ms,
        };

        for ext in &self.response_interceptors {
            tracing::trace!(interceptor = ext.name(), "response interceptor running");
            ext.intercept(&mut resp_ctx);
        }

        // 6. 返回
        let tampered = resp_ctx.modified_body.is_some();
        let final_body = resp_ctx.modified_body.clone().unwrap_or(accumulated);
        tracing::info!(
            category = %resp_ctx.meta.category,
            status,
            tampered,
            duration_ms = resp_ctx.duration_ms,
            resp_bytes = final_body.len(),
            "request completed"
        );

        let ct = if tampered {
            Some("application/json".to_string())
        } else {
            content_type
        };

        (final_body, tampered, ct)
    }
}

pub struct MitmCoreBuilder {
    openai_available: bool,
    claude: Option<(String, String)>,
    split_claude: bool,
    activation: Option<crate::activation::ActivationGate>,
    target: Option<String>,
    anthropic_api_key: Option<String>,
    client: Option<Client>,
    request_interceptors: Vec<Box<dyn RequestInterceptor>>,
    response_parser: Option<Box<dyn ResponseParser>>,
    response_interceptors: Vec<Box<dyn ResponseInterceptor>>,
}

impl MitmCoreBuilder {
    pub fn new() -> Self {
        Self {
            openai_available: true,
            claude: None,
            split_claude: false,
            activation: None,
            target: None,
            anthropic_api_key: None,
            client: None,
            request_interceptors: Vec::new(),
            response_parser: None,
            response_interceptors: Vec::new(),
        }
    }

    pub fn activation_gate(mut self, gate: crate::activation::ActivationGate) -> Self {
        self.activation = Some(gate);
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn anthropic_api_key(mut self, api_key: Option<String>) -> Self {
        self.anthropic_api_key = api_key;
        self
    }

    pub fn openai_provider(mut self, provider: Option<&crate::providers::Provider>) -> Self {
        self.openai_available = provider.is_some();
        if let Some(provider) = provider {
            self.target = Some(provider.normalized_url());
        }
        self
    }
    pub fn claude_provider(mut self, provider: Option<&crate::providers::Provider>) -> Self {
        self.split_claude = true;
        self.claude = provider
            .filter(|p| p.is_claude())
            .map(|p| (p.normalized_url(), p.api_key.clone()));
        self
    }

    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn request_interceptor(mut self, ext: impl RequestInterceptor + 'static) -> Self {
        self.request_interceptors.push(Box::new(ext));
        self
    }

    pub fn response_parser(mut self, ext: impl ResponseParser + 'static) -> Self {
        self.response_parser = Some(Box::new(ext));
        self
    }

    pub fn response_interceptor(mut self, ext: impl ResponseInterceptor + 'static) -> Self {
        self.response_interceptors.push(Box::new(ext));
        self
    }

    pub fn build(self) -> Result<MitmCore, String> {
        Ok(MitmCore {
            activation: self.activation.map(std::sync::Mutex::new),
            target: Arc::new(RwLock::new(UpstreamTarget {
                url: self.target.ok_or("target not set")?,
                anthropic_api_key: self.anthropic_api_key,
                claude: self.claude,
                split_claude: self.split_claude,
                openai_available: self.openai_available,
            })),
            client: self.client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .build()
                    .expect("failed to build reqwest client")
            }),
            request_interceptors: self.request_interceptors,
            response_parser: self.response_parser.ok_or("response parser not set")?,
            response_interceptors: self.response_interceptors,
        })
    }
}

impl Default for MitmCoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Native clients may include /v1 while a configured relay already ends in /v1.
fn upstream_url(target: &str, path: &str) -> String {
    let base = target.trim_end_matches('/');
    if path.starts_with("/v1/") || path.starts_with("/v1beta/") {
        for suffix in ["/v1", "/v1beta"] {
            if let Some(prefix) = base.strip_suffix(suffix) {
                return format!("{prefix}{path}");
            }
        }
    }
    format!("{base}{path}")
}
#[cfg(test)]
mod url_tests {
    #[test]
    fn version_prefix_is_not_duplicated() {
        assert_eq!(
            super::upstream_url("https://relay.example/api/v1", "/v1/messages"),
            "https://relay.example/api/v1/messages"
        );
        assert_eq!(
            super::upstream_url("https://relay.example/v1", "/responses"),
            "https://relay.example/v1/responses"
        );
        assert_eq!(
            super::upstream_url(
                "https://relay.example/v1",
                "/v1beta/models/gemini:generateContent"
            ),
            "https://relay.example/v1beta/models/gemini:generateContent"
        );
    }
}
