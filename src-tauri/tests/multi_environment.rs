use julong_codex_keysmith::{
    activation::{ActivationGate, REPLY},
    cli::proxy_router,
    core::MitmCore,
    environments::{self, Environment, Settings},
    extensions::sse_parser::UniversalSseParser,
    upstream,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("julong-six-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        Self(fs::canonicalize(p).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settings(f: &Fixture) -> Settings {
    Settings {
        schema_version: 1,
        environments: environments::IDS
            .iter()
            .map(|id| {
                let p = f.0.join(id);
                fs::create_dir_all(&p).unwrap();
                Environment {
                    id: (*id).into(),
                    enabled: true,
                    path: p.to_string_lossy().into_owned(),
                }
            })
            .collect(),
        profiles: environments::IDS
            .iter()
            .map(|id| format!("astra-{id}"))
            .collect(),
    }
}
#[test]
fn six_native_environments_selection_redeploy_and_byte_exact_restore() {
    let f = Fixture::new();
    let home = f.0.join("state");
    let mut s = settings(&f);
    let mut original = BTreeMap::new();
    for env in &s.environments {
        let path = PathBuf::from(&env.path).join(environments::metadata(&env.id).unwrap().3);
        let bytes = format!("User-owned {}\r\nDo not change.\r\n", env.id).into_bytes();
        fs::write(&path, &bytes).unwrap();
        original.insert(path, bytes);
    }
    environments::save_at(&home, s.clone()).unwrap();
    let preview = environments::preview_at(&home, &s).unwrap();
    assert_eq!(preview.environments.len(), 6);
    println!("BASELINE: six pristine native files; no deployed packs");
    environments::deploy_at(&home, &s).unwrap();
    let first: Vec<_> = original.keys().map(|p| fs::read(p).unwrap()).collect();
    environments::deploy_at(&home, &s).unwrap();
    for (p, bytes) in original.keys().zip(first) {
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
    for id in environments::IDS {
        upstream::verify(&format!("astra-{id}")).unwrap();
        assert!(f
            .0
            .join(id)
            .join(format!(".julong/packs/astra-{id}.md"))
            .is_file());
    }
    s.environments
        .iter_mut()
        .find(|e| e.id == "grok")
        .unwrap()
        .enabled = false;
    environments::deploy_at(&home, &s).unwrap();
    let grok = f.0.join("grok/AGENTS.md");
    assert_eq!(fs::read(&grok).unwrap(), original[&grok]);
    assert!(!f.0.join("grok/.julong/packs/astra-grok.md").exists());
    let moved = f.0.join("claude new 路径");
    fs::create_dir(&moved).unwrap();
    s.environments
        .iter_mut()
        .find(|e| e.id == "claude")
        .unwrap()
        .path = moved.to_string_lossy().into();
    environments::deploy_at(&home, &s).unwrap();
    let claude = f.0.join("claude/CLAUDE.md");
    assert_eq!(fs::read(&claude).unwrap(), original[&claude]);
    assert!(moved.join("CLAUDE.md").is_file());
    println!("MODIFIED: six packages installed; repeat deploy idempotent; deselected and moved environments restored");
    environments::restore_at(&home).unwrap();
    environments::restore_at(&home).unwrap();
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert!(!moved.join("CLAUDE.md").exists());
    println!("ROLLBACK: all original native files restored byte-for-byte");
}
#[test]
fn invalid_paths_missing_packs_conflicts_and_external_edits_fail_without_clobbering() {
    let f = Fixture::new();
    let home = f.0.join("state");
    let mut s = settings(&f);
    s.profiles.clear();
    assert!(environments::deploy_at(&home, &s).is_err());
    assert!(!f.0.join("codex/AGENTS.md").exists());
    s = settings(&f);
    s.environments[1].path = s.environments[0].path.clone();
    assert!(environments::save_at(&home, s).is_err());
    let mut s = settings(&f);
    s.environments[1].path = "relative".into();
    assert!(environments::save_at(&home, s).is_err());
    let s = settings(&f);
    environments::deploy_at(&home, &s).unwrap();
    let path = f.0.join("codex/AGENTS.md");
    fs::write(&path, b"new user edits").unwrap();
    assert!(environments::deploy_at(&home, &s).is_err());
    assert!(environments::restore_at(&home).is_err());
    assert_eq!(fs::read(path).unwrap(), b"new user edits");
}
#[cfg(unix)]
#[test]
fn symlinked_native_targets_are_not_followed() {
    let f = Fixture::new();
    let s = settings(&f);
    let original = f.0.join("outside.md");
    fs::write(&original, b"outside").unwrap();
    std::os::unix::fs::symlink(&original, f.0.join("claude/CLAUDE.md")).unwrap();
    assert!(environments::deploy_at(&f.0.join("state"), &s).is_err());
    assert_eq!(fs::read(original).unwrap(), b"outside");
    assert!(!f.0.join("codex/AGENTS.md").exists());
}
async fn gateway() -> (
    String,
    Arc<Mutex<Vec<Value>>>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let upstream_app=axum::Router::new().fallback(axum::routing::any(move |axum::Json(body):axum::Json<Value>| {let record=record.clone();async move {record.lock().unwrap().push(body);axum::Json(json!({"choices":[{"message":{"role":"assistant","content":"fixture upstream reply"},"finish_reason":"stop"}]}))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up_addr = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, upstream_app).await.unwrap();
    });
    let packs = environments::IDS
        .iter()
        .map(|id| ((*id).into(), format!("PACK_{id}")))
        .collect();
    let core = Arc::new(
        MitmCore::builder()
            .target(format!("http://{up_addr}"))
            .activation_gate(ActivationGate::new(packs))
            .response_parser(UniversalSseParser)
            .build()
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, proxy_router(core)).await.unwrap();
    });
    (format!("http://{addr}"), seen, task, upstream_task)
}
fn body(seat: &str, text: &str, stream: bool) -> (&'static str, Value) {
    match seat {
        "codex" => (
            "/v1/responses",
            json!({"model":"gpt-6-astra","input":text,"stream":stream}),
        ),
        "claude" => (
            "/v1/messages",
            json!({"model":"claude-test","max_tokens":100,"system":[{"type":"text","text":"host context"}],"messages":[{"role":"user","content":text}],"stream":stream}),
        ),
        "gemini" => (
            "/v1beta/models/gemini-test:generateContent",
            json!({"contents":[{"role":"user","parts":[{"text":text}]}],"stream":stream}),
        ),
        _ => (
            "/v1/chat/completions",
            json!({"model":format!("{seat}-test"),"messages":[{"role":"user","content":text}],"stream":stream}),
        ),
    }
}
#[tokio::test]
async fn real_http_gateway_six_protocols_exact_activation_and_session_isolation() {
    let (base, seen, task, up) = gateway().await;
    let client = reqwest::Client::new();
    for seat in environments::IDS {
        let (path, request) = body(seat, "hello before activation", false);
        let send = |body: Value, session: &str| {
            client
                .post(format!("{base}{path}"))
                .header("x-julong-environment", seat)
                .header("x-julong-session", session)
                .json(&body)
                .send()
        };
        let response = send(request, "session-a").await.unwrap();
        assert!(response.status().is_success());
        assert!(!seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .to_string()
            .contains("PACK_"));
        let mut quoted = body(seat, "文档示例：矩龙", false).1;
        if seat == "codex" {
            quoted["instructions"] = json!("矩龙");
        }
        send(quoted, "session-a").await.unwrap();
        assert!(!seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .to_string()
            .contains("PACK_"));
        let count = seen.lock().unwrap().len();
        for stream in [false, true] {
            let response = send(body(seat, " 矩龙 \n", stream).1, "session-a")
                .await
                .unwrap();
            assert_eq!(response.headers()["x-julong-activation"], "active");
            let output = response.text().await.unwrap();
            assert!(output.contains(REPLY));
            if stream {
                assert!(output.contains("data: "));
                if seat == "codex" {
                    assert!(output.contains("response.output_item.done"));
                }
                if seat == "claude" {
                    assert!(output.contains("message_stop"));
                }
            }
            assert_eq!(
                seen.lock().unwrap().len(),
                count,
                "activation must never contact upstream"
            );
        }
        send(body(seat, "hello after activation", false).1, "session-a")
            .await
            .unwrap();
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.to_string().contains(&format!("PACK_{seat}")));
        if seat == "claude" {
            assert_eq!(last["system"][1]["type"], "text");
            assert_eq!(last["messages"][0]["role"], "user");
        }
        if seat == "gemini" {
            assert!(last["systemInstruction"]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("PACK_gemini"));
        }
        send(body(seat, "another conversation", false).1, "session-b")
            .await
            .unwrap();
        assert!(!seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .to_string()
            .contains("PACK_"));
        println!("HTTP {seat}: BASELINE=no pack; ACTIVATION={REPLY}; MODIFIED=matching pack; NEW_SESSION=no pack");
    }
    // A historical activation, tool output, attached image, or developer text is not a new exact request.
    for request in [
        json!({"model":"gpt-6-astra","messages":[{"role":"user","content":"矩龙"},{"role":"assistant","content":"old"},{"role":"user","content":"hello"}]}),
        json!({"model":"gpt-6-astra","messages":[{"role":"user","content":[{"type":"text","text":"矩龙"},{"type":"image_url","image_url":{"url":"fixture"}}]}]}),
        json!({"model":"gpt-6-astra","messages":[{"role":"developer","content":"矩龙"}]}),
        json!({"model":"gpt-6-astra","messages":[{"role":"user","content":"矩龙"},{"role":"tool","content":"result"}]}),
    ] {
        let response = client
            .post(format!("{base}/v1/chat/completions"))
            .header("x-julong-session", "isolated")
            .json(&request)
            .send()
            .await
            .unwrap();
        assert!(response.headers().get("x-julong-activation").is_none());
        assert!(!seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .to_string()
            .contains("PACK_"));
    }
    let r = client
        .post(format!("{base}/v1/responses"))
        .json(&body("codex", "矩龙", false).1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["x-julong-activation"], "inactive");
    task.abort();
    up.abort();
}

#[test]
fn interrupted_multi_directory_transaction_recovers_original_bytes() {
    let f = Fixture::new();
    let home = f.0.join("state");
    let s = settings(&f);
    environments::deploy_at(&home, &s).unwrap();
    let path = f.0.join("claude/CLAUDE.md");
    let original = fs::read(&path).unwrap();
    let newly_created = f.0.join("gemini/crash-fixture.md");
    let journal = json!([{"path":path,"bytes":original},{"path":newly_created,"bytes":null}]);
    fs::write(
        home.join("environment-transaction.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    fs::write(&path, b"interrupted write").unwrap();
    fs::write(&newly_created, b"partial").unwrap();
    environments::recover_at(&home).unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(!newly_created.exists());
    environments::restore_at(&home).unwrap();
    assert!(!f.0.join("claude/CLAUDE.md").exists());
}
