use julong_codex_keysmith::{
    claude,
    claude_desktop::{self, FileKind},
    claude_models,
    cli::proxy_router,
    core::MitmCore,
    extensions::sse_parser::UniversalSseParser,
    providers::{self, Provider},
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn provider() -> Provider {
    serde_json::from_value(json!({
        "id":"mapped", "category":"claude", "name":"Mapped", "request_url":"http://127.0.0.1:19099",
        "api_key":"dummy-key", "default_model":"fallback-model",
        "claude_models":{
            "sonnet":{"model":"gpt-6-astra", "display_name":"主力 Astra", "supports_1m":true},
            "opus":{"model":"deepseek-v4-pro"},
            "fable":{}, "haiku":{"model":"fast-model"},
            "subagent":{"model":"subagent-model", "supports_1m":true}, "default_1m":true
        }
    }))
    .unwrap()
}

#[test]
fn code_and_desktop_use_role_names_but_accept_arbitrary_upstream_models() {
    let p = provider();
    let original = json!({"permissions":{"allow":["Read"]}, "env":{
        "ANTHROPIC_DEFAULT_FABLE_MODEL":"stale", "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME":"stale name",
        "CLAUDE_CODE_SUBAGENT_MODEL_NAME":"stale name", "ANTHROPIC_SMALL_FAST_MODEL":"stale"
    }});
    let code: Value = serde_json::from_slice(
        &claude::render_provider_settings(
            Some(&serde_json::to_vec(&original).unwrap()),
            "http://127.0.0.1:8080",
            Some(&p),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        code["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "gpt-6-astra[1M]"
    );
    assert_eq!(
        code["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"],
        "主力 Astra"
    );
    assert_eq!(
        code["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
        "subagent-model[1M]"
    );
    assert_eq!(code["env"]["ANTHROPIC_MODEL"], "fallback-model[1M]");
    assert_eq!(code["permissions"], original["permissions"]);
    for stale in [
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        "CLAUDE_CODE_SUBAGENT_MODEL_NAME",
        "ANTHROPIC_SMALL_FAST_MODEL",
    ] {
        assert!(code["env"].get(stale).is_none());
    }
    let profile: Value =
        serde_json::from_slice(&claude_desktop::render(FileKind::Profile, None, &p).unwrap())
            .unwrap();
    assert_eq!(
        profile["inferenceModels"][0],
        json!({"name":"claude-sonnet-4-6","labelOverride":"主力 Astra","supports1m":true})
    );
    assert_eq!(
        profile["inferenceModels"][2]["labelOverride"],
        "deepseek-v4-pro"
    );
    assert_eq!(profile["inferenceModels"].as_array().unwrap().len(), 4);
    assert!(!profile["inferenceModels"].to_string().contains("[1M]"));
    let roundtrip: Provider = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(roundtrip.claude_models, p.claude_models);
}

#[test]
fn legacy_defaults_and_explicit_empty_roles_have_distinct_meanings() {
    let mut p: Provider = serde_json::from_value(json!({"id":"old","name":"Old","category":"claude","request_url":"http://localhost","api_key":"dummy","default_model":"gpt-6-astra [1m]"})).unwrap();
    assert_eq!(
        claude_models::desktop_models(&p)[0]["labelOverride"],
        "gpt-6-astra"
    );
    assert_eq!(claude_models::upstream_models(&p), ["gpt-6-astra"]);
    p.claude_models = Some(Default::default());
    p.default_model.clear();
    p.models = vec!["catalog-only-model".into()];
    assert!(claude_desktop::render(FileKind::Profile, None, &p).is_err());
    let mut body = json!({"model":"custom-model [1M]","messages":[]});
    claude_models::map_body(&mut body, &p, false);
    assert_eq!(body["model"], "custom-model");
    let settings: Value = serde_json::from_slice(&claude::render_provider_settings(Some(br#"{"model":"stale","env":{"ANTHROPIC_MODEL":"stale","ANTHROPIC_DEFAULT_SONNET_MODEL":"stale"}}"#), "http://localhost", Some(&p)).unwrap()).unwrap();
    assert!(settings.get("model").is_none());
    assert!(settings["env"]
        .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .is_none());
}

#[test]
fn native_system_normalization_retains_order_cache_control_and_tool_results() {
    let reminder = json!({"type":"text","text":"Environment","cache_control":{"type":"ephemeral"}});
    let user = json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"fixture","content":"tool output"}]});
    let mut body = json!({"system":"initial", "messages":[{"role":"system","content":"first"},user,{"role":"system","content":[reminder]}]});
    claude::normalize_native_system_messages(&mut body).unwrap();
    assert_eq!(
        body["system"],
        json!([{"type":"text","text":"initial"},{"type":"text","text":"first"},reminder])
    );
    assert_eq!(body["messages"], json!([user]));
    let mut malformed =
        json!({"messages":[{"role":"system","content":[{"type":"image","source":{}}]}]});
    assert!(claude::normalize_native_system_messages(&mut malformed).is_err());
}

#[tokio::test]
async fn wire_mapping_covers_roles_fallback_subagent_catalog_tools_stream_and_hot_switch() {
    use axum::{
        body::Body,
        http::{HeaderMap, Uri},
        response::Response,
    };
    let seen = Arc::new(Mutex::new(Vec::<(String, Value, String)>::new()));
    let record = seen.clone();
    let upstream = axum::Router::new().fallback(axum::routing::any(move |uri: Uri, headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
        let record = record.clone();
        async move {
            record.lock().unwrap().push((uri.to_string(), body.clone(), headers["x-api-key"].to_str().unwrap().into()));
            if body["stream"] == true {
                Response::builder().header("content-type","text/event-stream").body(Body::from("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"gpt-6-astra\",\"content\":[]}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")).unwrap()
            } else {
                Response::builder().header("content-type","application/json").body(Body::from(json!({"type":"message","model":body["model"],"content":[{"type":"text","text":"OK"}]}).to_string())).unwrap()
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut p = provider();
    p.request_url = format!("http://{}", listener.local_addr().unwrap());
    let up = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let core = Arc::new(
        MitmCore::builder()
            .target("http://unused.invalid")
            .openai_provider(None)
            .claude_provider(Some(&p))
            .response_parser(UniversalSseParser)
            .build()
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = proxy_router(core.clone());
    let proxy = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    let catalog: Value = client
        .get(format!("{base}/claude-desktop/v1/models?limit=100"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(catalog["data"][0]["id"], "claude-sonnet-4-6");
    assert_eq!(catalog["data"][0]["display_name"], "主力 Astra");
    assert!(
        seen.lock().unwrap().is_empty(),
        "Desktop catalog must not leak raw upstream IDs"
    );
    for (path, model, expected) in [
        (
            "/claude-desktop/v1/messages",
            "claude-sonnet-4-6[1M]",
            "gpt-6-astra",
        ),
        ("/v1/messages", "gpt-6-astra[1M]", "gpt-6-astra"),
        ("/v1/messages", "claude-opus-4-8", "deepseek-v4-pro"),
        (
            "/claude-desktop/v1/messages",
            "claude-fable-5",
            "deepseek-v4-pro",
        ),
        (
            "/claude-desktop/v1/messages",
            "claude-haiku-4-5-20251001",
            "fast-model",
        ),
        (
            "/v1/messages/count_tokens",
            "claude-3-haiku-20240307",
            "fast-model",
        ),
        ("/v1/messages", "subagent-model [1m]", "subagent-model"),
        ("/v1/messages", "claude-unknown", "fallback-model"),
    ] {
        let body = json!({"model":model,"stream":false,"max_tokens":32,"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"fixture"}]}],"tools":[{"name":"fixture","input_schema":{"type":"object"}}],"thinking":{"type":"enabled","budget_tokens":16}});
        let response = client
            .post(format!("{base}{path}"))
            .bearer_auth("stale-client-key")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let (_, actual, key) = seen.lock().unwrap().last().unwrap().clone();
        let mut expected_body = body;
        expected_body["model"] = json!(expected);
        assert_eq!(actual, expected_body);
        assert_eq!(key, "dummy-key");
    }
    let stream = client
        .post(format!("{base}/claude-desktop/v1/messages"))
        .json(&json!({"model":"claude-sonnet-4-6","stream":true,"messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.headers()["content-type"], "text/event-stream");
    assert!(stream.text().await.unwrap().contains("message_stop"));
    p.claude_models.as_mut().unwrap().sonnet.model = "new-model".into();
    p.api_key = "new-dummy-key".into();
    core.update_category(&p).await;
    client
        .post(format!("{base}/claude-desktop/v1/messages"))
        .json(&json!({"model":"claude-sonnet-4-6","messages":[]}))
        .send()
        .await
        .unwrap();
    let (_, body, key) = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(body["model"], "new-model");
    assert_eq!(key, "new-dummy-key");
    assert!(providers::test_claude(&p)
        .await
        .unwrap()
        .contains("fallback-model"));
    core.clear_claude_provider().await;
    assert_eq!(
        client
            .get(format!("{base}/claude-desktop/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    proxy.abort();
    up.abort();
}

#[test]
fn desktop_route_wins_over_a_different_roles_literal_target() {
    let mut p = provider();
    p.claude_models.as_mut().unwrap().haiku.model = "claude-sonnet-4-6".into();
    let mut body = json!({"model":"claude-sonnet-4-6"});
    claude_models::map_body(&mut body, &p, true);
    assert_eq!(body["model"], "gpt-6-astra");
}
