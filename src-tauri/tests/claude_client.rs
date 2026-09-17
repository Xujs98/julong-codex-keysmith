//! Optional installed-client acceptance: local mock only; no paid API requests.
use axum::{body::Body, http::HeaderMap, response::Response};
use julong_codex_keysmith::{
    activation::{ActivationGate, REPLY},
    claude,
    cli::proxy_router,
    core::MitmCore,
    extensions::sse_parser::UniversalSseParser,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reply(body: &Value) -> Response {
    let message = json!({"id":"msg_local_fixture", "type":"message", "role":"assistant",
        "model":body["model"], "content":[], "stop_reason":null, "stop_sequence":null,
        "usage":{"input_tokens":1,"output_tokens":0}});
    let events = [
        json!({"type":"message_start","message":message}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Local Claude fixture reply."}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(
            events
                .iter()
                .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
                .collect::<String>(),
        ))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires JULONG_CLAUDE_TEST_BIN pointing to an installed native Claude executable"]
async fn native_claude_settings_activation_resume_and_isolation() {
    let binary =
        std::env::var_os("JULONG_CLAUDE_TEST_BIN").expect("JULONG_CLAUDE_TEST_BIN required");
    let f = Fixture(
        std::env::temp_dir().join(format!("julong-native-claude-{}", uuid::Uuid::new_v4())),
    );
    fs::create_dir_all(f.0.join("home/.claude")).unwrap();
    fs::create_dir_all(f.0.join("project")).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<(bool, bool)>::new()));
    let record = seen.clone();
    let upstream = axum::Router::new().fallback(axum::routing::any(
        move |headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
            let record = record.clone();
            async move {
                println!(
                    "native fixture received: last_message={}, metadata={}, header_names={:?}",
                    body["messages"]
                        .as_array()
                        .and_then(|m| m.last())
                        .unwrap_or(&Value::Null),
                    body["metadata"],
                    headers.keys().collect::<Vec<_>>()
                );
                record.lock().unwrap().push((
                    body.to_string().contains("NATIVE_CLAUDE_TEST_PACK"),
                    headers.get("x-api-key").and_then(|h| h.to_str().ok())
                        == Some("dummy-relay-key"),
                ));
                reply(&body)
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let up = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let core = Arc::new(
        MitmCore::builder()
            .target(format!("http://{upstream_addr}/v1"))
            .anthropic_api_key(Some("dummy-relay-key".into()))
            .activation_gate(ActivationGate::new(BTreeMap::from([(
                "claude".into(),
                "NATIVE_CLAUDE_TEST_PACK".into(),
            )])))
            .response_parser(UniversalSseParser)
            .build()
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        axum::serve(listener, proxy_router(core)).await.unwrap();
    });
    let settings = f.0.join("home/.claude/settings.json");
    fs::write(
        &settings,
        claude::render_settings(None, &format!("http://{addr}")).unwrap(),
    )
    .unwrap();
    let session = uuid::Uuid::new_v4().to_string();
    for (index, prompt) in [
        "矩龙",
        "Reply once in this existing conversation.",
        "Reply once in a fresh conversation.",
    ]
    .iter()
    .enumerate()
    {
        let mut command = tokio::process::Command::new(&binary);
        command
            .current_dir(f.0.join("project"))
            .env("HOME", f.0.join("home"))
            .env("USERPROFILE", f.0.join("home"))
            .env("CLAUDE_CONFIG_DIR", f.0.join("home/.claude"))
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_AUTOUPDATER", "1")
            .env("DISABLE_TELEMETRY", "1")
            .env("DISABLE_ERROR_REPORTING", "1")
            .args([
                "-p",
                "--output-format",
                "json",
                "--model",
                "claude-sonnet-4-6",
                "--tools",
                "",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--disable-slash-commands",
                "--setting-sources",
                "user",
                "--max-turns",
                "1",
                "--system-prompt",
                "Answer briefly. Local integration test.",
            ])
            .kill_on_drop(true);
        for (key, _) in std::env::vars() {
            if key.starts_with("ANTHROPIC_")
                || key.starts_with("CLAUDE_CODE_USE_")
                || key == "CLAUDE_CODE_OAUTH_TOKEN"
            {
                command.env_remove(key);
            }
        }
        if index == 0 {
            command.args(["--session-id", &session]);
        }
        if index == 1 {
            command.args(["--resume", &session]);
        }
        command.arg(prompt);
        let output = tokio::time::timeout(Duration::from_secs(75), command.output())
            .await
            .expect("Claude timed out")
            .unwrap();
        assert!(
            output.status.success(),
            "Claude failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result["result"],
            if index == 0 {
                REPLY
            } else {
                "Local Claude fixture reply."
            }
        );
        println!(
            "native Claude turn {index}: reply={}, settings loaded from isolated user directory",
            result["result"]
        );
        if index == 0 {
            assert!(
                seen.lock().unwrap().is_empty(),
                "activation must stay local"
            );
        } else {
            assert_eq!(seen.lock().unwrap().last(), Some(&(index == 1, true)));
        }
    }
    assert_eq!(*seen.lock().unwrap(), vec![(true, true), (false, true)]);
    proxy.abort();
    up.abort();
}

#[tokio::test]
async fn messages_credentials_switch_with_provider_and_other_protocols_stay_unchanged() {
    let received = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
    let record = received.clone();
    let upstream = axum::Router::new().fallback(axum::routing::any(
        move |uri: http::Uri, headers: HeaderMap| {
            let record = record.clone();
            async move {
                record.lock().unwrap().push((
                    uri.to_string(),
                    headers
                        .get("authorization")
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .into(),
                    headers
                        .get("x-api-key")
                        .map(|h| h.to_str().unwrap())
                        .unwrap_or("")
                        .into(),
                ));
                axum::Json(json!({"ok":true}))
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let core = MitmCore::builder()
        .target(format!("http://{addr}/v1"))
        .anthropic_api_key(Some("key-one".into()))
        .response_parser(UniversalSseParser)
        .build()
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer old-client-key".parse().unwrap());
    for path in [
        "/v1/messages?beta=true",
        "/v1/messages/count_tokens",
        "/v1/responses",
    ] {
        core.handle_request(
            http::Method::POST,
            path.into(),
            headers.clone(),
            b"{}".as_slice().into(),
        )
        .await
        .unwrap();
        core.set_provider(format!("http://{addr}/v1"), "key-two")
            .await;
    }
    assert_eq!(
        *received.lock().unwrap(),
        vec![
            (
                "/v1/messages?beta=true".into(),
                "Bearer key-one".into(),
                "key-one".into()
            ),
            (
                "/v1/messages/count_tokens".into(),
                "Bearer key-two".into(),
                "key-two".into()
            ),
            (
                "/v1/responses".into(),
                "Bearer old-client-key".into(),
                "".into()
            ),
        ]
    );
    core.set_provider(format!("http://{addr}"), "").await;
    headers.insert(
        "authorization",
        "Bearer julong-local-proxy".parse().unwrap(),
    );
    assert!(core
        .handle_request(
            http::Method::POST,
            "/v1/messages".into(),
            headers,
            b"{}".as_slice().into()
        )
        .await
        .is_err());
    assert_eq!(received.lock().unwrap().len(), 3);
    task.abort();
}
