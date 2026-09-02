// extract_user + categorize — 从请求 JSON 中提取真实用户消息并分类

use crate::core::context::Category;

/// 从请求 JSON 中提取真实用户消息，跳过环境上下文 / system / config
pub fn extract_user(data: &serde_json::Value) -> String {
    let mut texts = Vec::new();

    // Responses API 用 "input"，Chat API 用 "messages"
    let arr = data
        .get("input")
        .and_then(|v| v.as_array())
        .or_else(|| data.get("messages").and_then(|v| v.as_array()));

    if let Some(arr) = arr {
        for item in arr {
            if let Some(obj) = item.as_object() {
                let role = obj.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role != "user" {
                    continue;
                }

                if let Some(content) = obj.get("content") {
                    // content 可以是字符串或对象数组
                    if let Some(s) = content.as_str() {
                        if !is_env_context(s) {
                            texts.push(s.to_string());
                        }
                    } else if let Some(content_arr) = content.as_array() {
                        for c in content_arr {
                            if let Some(c_obj) = c.as_object() {
                                if let Some(t) = c_obj.get("text").and_then(|t| t.as_str()) {
                                    if !t.is_empty() && !is_env_context(t) {
                                        texts.push(t.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    texts.join(" ")
}

/// 过滤环境上下文、配置、工具消息
fn is_env_context(t: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "<environment_context>",
        "<cwd>",
        "<shell>",
        "AGENTS.md",
        "Project Configuration",
        "provide a short title",
        "You are a helpful assistant",
    ];
    for kw in KEYWORDS {
        if t.contains(kw) {
            return true;
        }
    }
    let trimmed = t.trim();
    if trimmed.starts_with('<') || trimmed.starts_with('#') {
        return true;
    }
    false
}

/// 将用户消息分类到四个领域之一
pub fn categorize(user_msg: &str) -> Category {
    let u = user_msg.to_lowercase();
    let crack_words = [
        "crack", "keygen", "license", "serial", "activate", "破解", "注册", "激活", "授权",
    ];
    let reverse_words = ["reverse", "disassemble", "decompile", "逆向", "反汇编"];
    let pentest_words = [
        "pentest", "exploit", "sqli", "xss", "scan", "渗透", "漏洞", "扫描",
    ];

    if crack_words.iter().any(|w| u.contains(w)) {
        return Category::Crack;
    }
    if reverse_words.iter().any(|w| u.contains(w)) {
        return Category::Reverse;
    }
    if pentest_words.iter().any(|w| u.contains(w)) {
        return Category::Pentest;
    }
    Category::General
}
