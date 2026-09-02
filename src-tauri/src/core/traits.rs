// MITM Core traits — three extension roles

use crate::core::context::{ParsedResponse, RequestCtx, ResponseCtx};
use bytes::Bytes;

/// 请求拦截器：转发前修改请求体（注入系统提示词等）
pub trait RequestInterceptor: Send + Sync {
    fn name(&self) -> &'static str;
    fn intercept(&self, ctx: &mut RequestCtx);
}

/// 响应解析器：将原始响应字节解码为结构化文本
/// Core 持有单一解析器，可替换以支持不同上游格式
pub trait ResponseParser: Send + Sync {
    fn parse(&self, body: &Bytes) -> ParsedResponse;
}

/// 响应拦截器：检查/修改已解析的响应
/// 全量执行，每个扩展自判断是否介入
pub trait ResponseInterceptor: Send + Sync {
    fn name(&self) -> &'static str;
    fn intercept(&self, ctx: &mut ResponseCtx);
}

// Arc blanket impls — 允许共享扩展实例 between Core 和 Tauri commands
impl<T: RequestInterceptor + ?Sized> RequestInterceptor for std::sync::Arc<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }
    fn intercept(&self, ctx: &mut RequestCtx) {
        (**self).intercept(ctx)
    }
}

impl<T: ResponseParser + ?Sized> ResponseParser for std::sync::Arc<T> {
    fn parse(&self, body: &Bytes) -> ParsedResponse {
        (**self).parse(body)
    }
}

impl<T: ResponseInterceptor + ?Sized> ResponseInterceptor for std::sync::Arc<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }
    fn intercept(&self, ctx: &mut ResponseCtx) {
        (**self).intercept(ctx)
    }
}
