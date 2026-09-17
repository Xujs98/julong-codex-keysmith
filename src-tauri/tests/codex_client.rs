//! Optional real-client regression. All requests stay on loopback, with an isolated
//! Codex home, dummy credentials and a fixed upstream response (no model or tools).
use axum::{body::Body, http::HeaderMap, response::Response};
use julong_codex_keysmith::{
    activation::{ActivationGate, REPLY},
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

// Enough Responses events for the native Codex stream reader to finish a turn.
fn upstream_reply() -> Response {
    let item = json!({"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Local fixture reply.","annotations":[]}]});
    let events = [
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"resp_fixture","object":"response","status":"completed","output":[item],"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}),
    ];
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(
            events
                .iter()
                .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
                .collect::<String>(),
        ))
        .unwrap()
}

#[tokio::test]
#[ignore = "set JULONG_CODEX_TEST_BIN to the installed Codex executable; uses local fixtures only"]
async fn installed_codex_activates_resumes_and_isolates_new_conversations() {
    let binary = std::env::var_os("JULONG_CODEX_TEST_BIN").expect("JULONG_CODEX_TEST_BIN required");
    let f =
        Fixture(std::env::temp_dir().join(format!("julong-native-codex-{}", uuid::Uuid::new_v4())));
    fs::create_dir_all(f.0.join("home")).unwrap();
    fs::create_dir_all(f.0.join("project")).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<bool>::new()));
    let record = seen.clone();
    let upstream = axum::Router::new().fallback(axum::routing::any(
        move |axum::Json(body): axum::Json<Value>| {
            let record = record.clone();
            async move {
                record
                    .lock()
                    .unwrap()
                    .push(body.to_string().contains("NATIVE_CODEX_TEST_PACK"));
                upstream_reply()
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
            .target(format!("http://{upstream_addr}"))
            .activation_gate(ActivationGate::new(BTreeMap::from([(
                "codex".into(),
                "NATIVE_CODEX_TEST_PACK".into(),
            )])))
            .response_parser(UniversalSseParser)
            .build()
            .unwrap(),
    );
    let header_names = Arc::new(Mutex::new(Vec::<String>::new()));
    let record_headers = header_names.clone();
    let app = proxy_router(core).layer(axum::middleware::from_fn(
        move |headers: HeaderMap, request: axum::extract::Request, next: axum::middleware::Next| {
            let record_headers = record_headers.clone();
            async move {
                // Record names only, never credentials, session values or message bodies.
                *record_headers.lock().unwrap() =
                    headers.keys().map(|k| k.as_str().to_string()).collect();
                next.run(request).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    fs::write(
        f.0.join("home/config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_provider = "local_fixture"
approval_policy = "never"
sandbox_mode = "read-only"
[model_providers.local_fixture]
name = "Local fixture"
base_url = "http://{addr}/v1"
wire_api = "responses"
requires_openai_auth = false
env_key = "JULONG_NATIVE_TEST_KEY"
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
    .unwrap();
    for (index, prompt) in [
        "矩龙",
        "A follow-up in the same conversation.",
        "A new unactivated conversation.",
    ]
    .iter()
    .enumerate()
    {
        let output_file = f.0.join(format!("reply-{index}.txt"));
        let mut command = tokio::process::Command::new(&binary);
        command
            .current_dir(f.0.join("project"))
            .env("CODEX_HOME", f.0.join("home"))
            .env("JULONG_NATIVE_TEST_KEY", "dummy-local-test-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_BASE_URL")
            .args(["exec", "--skip-git-repo-check", "--json", "-o"])
            .arg(&output_file)
            .kill_on_drop(true);
        if index == 1 {
            command.args(["resume", "--last"]);
        }
        command.arg(prompt);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .expect("Codex client timed out")
            .unwrap();
        assert!(
            output.status.success(),
            "Codex failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reply = fs::read_to_string(&output_file).unwrap();
        println!(
            "native turn {index}: headers={:?}",
            header_names.lock().unwrap()
        );
        if index == 0 {
            assert_eq!(reply.trim(), REPLY);
            assert!(
                seen.lock().unwrap().is_empty(),
                "activation must stay local"
            );
            let headers = header_names.lock().unwrap();
            assert!(headers
                .iter()
                .any(|k| k == "thread-id" || k == "session-id"));
            assert!(!headers.iter().any(|k| k.starts_with("x-julong-")));
        } else {
            assert_eq!(reply.trim(), "Local fixture reply.");
            assert_eq!(seen.lock().unwrap().last(), Some(&(index == 1)));
        }
    }
    assert_eq!(*seen.lock().unwrap(), vec![true, false]);
    proxy.abort();
    up.abort();
}
