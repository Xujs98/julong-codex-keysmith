use julong_codex_keysmith::{
    activation::ActivationGate,
    claude_desktop::{self, FileKind, PROFILE_ID},
    cli::proxy_router,
    core::MitmCore,
    environments::{self, Environment, Settings},
    extensions::sse_parser::UniversalSseParser,
    providers::{self, Provider},
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
        let root = std::env::temp_dir().join(format!("julong-desktop-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Application Support/Claude")).unwrap();
        fs::create_dir_all(root.join("code")).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
    fn root(&self) -> PathBuf {
        self.0.join("Application Support/Claude")
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
    fn settings(&self) -> Settings {
        Settings {
            schema_version: 1,
            profiles: vec!["astra-claude".into()],
            environments: environments::IDS
                .iter()
                .map(|id| Environment {
                    id: (*id).into(),
                    enabled: matches!(*id, "claude" | "claude-desktop"),
                    path: match *id {
                        "claude" => self.0.join("code").to_string_lossy().into(),
                        "claude-desktop" => self.root().to_string_lossy().into(),
                        _ => String::new(),
                    },
                })
                .collect(),
        }
    }
    fn provider(&self) -> Provider {
        serde_json::from_value(json!({"id":"fixture", "category":"claude", "name":"fixture", "request_url":"http://127.0.0.1:19099", "api_key":"dummy-key-one", "default_model":"claude-sonnet-4-6", "models":["gpt-6-astra","claude-sonnet-4-6","claude-haiku-4-5"]})).unwrap()
    }
    fn save(&self) {
        environments::save_at(&self.state(), self.settings()).unwrap();
        fs::create_dir_all(self.state().join("control")).unwrap();
        providers::save_list(&self.state().join("control"), &[self.provider()]).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn json_at(path: PathBuf) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn profile(f: &Fixture) -> PathBuf {
    f.root()
        .with_file_name("Claude-3p")
        .join(format!("configLibrary/{PROFILE_ID}.json"))
}

#[test]
fn custom_role_mapping_redeploy_switch_and_restore_preserve_first_backup() {
    let f = Fixture::new();
    f.save();
    let code_path = f.0.join("code/settings.json");
    let original = br#"{"env":{"ANTHROPIC_DEFAULT_SONNET_MODEL_NAME":"original"},"permissions":{"allow":["Read"]}}"#;
    fs::write(&code_path, original).unwrap();
    let mut p = f.provider();
    p.default_model = "gpt-6-astra".into();
    p.models.clear();
    p.claude_models = Some(
        serde_json::from_value(json!({
            "sonnet":{"model":"gpt-6-astra","display_name":"Astra 主力","supports_1m":true},
            "opus":{"model":"deepseek-v4-pro"},"subagent":{"model":"small-model"}
        }))
        .unwrap(),
    );
    providers::save_list(&f.state().join("control"), &[p.clone()]).unwrap();
    environments::deploy_at(&f.state(), &f.settings()).unwrap();
    environments::deploy_at(&f.state(), &f.settings()).unwrap();
    assert_eq!(
        json_at(code_path.clone())["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "gpt-6-astra[1M]"
    );
    assert_eq!(
        json_at(profile(&f))["inferenceModels"][0]["labelOverride"],
        "Astra 主力"
    );
    p.claude_models.as_mut().unwrap().sonnet.model = "other-model".into();
    p.claude_models
        .as_mut()
        .unwrap()
        .sonnet
        .display_name
        .clear();
    environments::sync_claude_provider_at(&f.state(), &p).unwrap();
    assert_eq!(
        json_at(profile(&f))["inferenceModels"][0]["labelOverride"],
        "other-model"
    );
    assert!(json_at(code_path.clone())["env"]
        .get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
        .is_none());
    environments::restore_selected_at(&f.state(), &["claude".into(), "claude-desktop".into()])
        .unwrap();
    assert_eq!(fs::read(code_path).unwrap(), original);
    assert!(!profile(&f).exists());
}

#[test]
fn deploy_both_claudes_sync_selected_and_restore_original_bytes() {
    let f = Fixture::new();
    f.save();
    let settings = f.settings();
    let config = f.root().join("claude_desktop_config.json");
    let original=b"{\r\n\"preferences\":{\"theme\":\"dark\"},\"mcpServers\":{\"user\":{}},\"deploymentMode\":\"1p\"\r\n}\r\n";
    fs::write(&config, original).unwrap();
    let meta = profile(&f).with_file_name("_meta.json");
    fs::create_dir_all(meta.parent().unwrap()).unwrap();
    let meta_original=b"{\"entries\":[{\"id\":\"other\",\"name\":\"User profile\"}],\"appliedId\":\"other\",\"custom\":true}";
    fs::write(&meta, meta_original).unwrap();
    let preview = environments::preview_at(&f.state(), &settings).unwrap();
    assert!(preview
        .actions
        .iter()
        .any(|a| a.contains("Claude-3p/configLibrary/_meta.json")));
    environments::deploy_at(&f.state(), &settings).unwrap();
    let p = json_at(profile(&f));
    assert_eq!(p["inferenceGatewayBaseUrl"], claude_desktop::BASE_URL);
    assert_eq!(p["inferenceGatewayApiKey"], "dummy-key-one");
    assert_eq!(
        p["inferenceModels"],
        json!([
            {"name":"claude-sonnet-4-6","labelOverride":"claude-sonnet-4-6","supports1m":false},
            {"name":"claude-opus-4-6","labelOverride":"claude-sonnet-4-6","supports1m":false},
            {"name":"claude-fable-5","labelOverride":"claude-sonnet-4-6","supports1m":false},
            {"name":"claude-haiku-4-5","labelOverride":"claude-haiku-4-5","supports1m":false}
        ])
    );
    assert!(p.get("coworkEgressAllowedHosts").is_none());
    assert_eq!(json_at(config.clone())["mcpServers"], json!({"user":{}}));
    assert_eq!(json_at(config.clone())["deploymentMode"], "3p");
    assert_eq!(
        json_at(meta.clone())["entries"].as_array().unwrap().len(),
        2
    );
    let code = json_at(f.0.join("code/settings.json"));
    assert_eq!(code["env"]["ANTHROPIC_AUTH_TOKEN"], "dummy-key-one");
    assert_eq!(code["model"], "claude-sonnet-4-6");
    environments::deploy_at(&f.state(), &settings).unwrap();
    let mut provider = f.provider();
    provider.api_key = "dummy-key-two".into();
    provider.models = vec!["claude-opus-4-6".into()];
    provider.default_model = "claude-opus-4-6".into();
    environments::sync_claude_provider_at(&f.state(), &provider).unwrap();
    assert_eq!(
        json_at(profile(&f))["inferenceGatewayApiKey"],
        "dummy-key-two"
    );
    assert_eq!(
        json_at(f.0.join("code/settings.json"))["model"],
        "claude-opus-4-6"
    );
    let desktop_bytes = fs::read(profile(&f)).unwrap();
    let mut selected = settings.clone();
    selected
        .environments
        .iter_mut()
        .find(|e| e.id == "claude-desktop")
        .unwrap()
        .enabled = false;
    environments::save_at(&f.state(), selected).unwrap();
    provider.api_key = "dummy-key-three".into();
    environments::sync_claude_provider_at(&f.state(), &provider).unwrap();
    assert_eq!(fs::read(profile(&f)).unwrap(), desktop_bytes);
    assert_eq!(
        json_at(f.0.join("code/settings.json"))["env"]["ANTHROPIC_AUTH_TOKEN"],
        "dummy-key-three"
    );
    environments::restore_selected_at(&f.state(), &["claude".into()]).unwrap();
    assert!(!f.0.join("code/settings.json").exists());
    assert_eq!(fs::read(profile(&f)).unwrap(), desktop_bytes);
    environments::restore_selected_at(&f.state(), &["claude-desktop".into()]).unwrap();
    assert_eq!(fs::read(config).unwrap(), original);
    assert_eq!(fs::read(meta).unwrap(), meta_original);
    assert!(!profile(&f).exists());
    assert!(!f
        .root()
        .with_file_name("Claude-3p")
        .join("claude_desktop_config.json")
        .exists());
}

#[test]
fn desktop_invalid_configs_models_paths_and_external_edits_fail_without_overwrite() {
    let f = Fixture::new();
    f.save();
    let settings = f.settings();
    let meta = profile(&f).with_file_name("_meta.json");
    fs::create_dir_all(meta.parent().unwrap()).unwrap();
    fs::write(&meta, b"{\"entries\":{}}").unwrap();
    assert!(environments::deploy_at(&f.state(), &settings).is_err());
    assert!(!f.root().join("claude_desktop_config.json").exists());
    fs::remove_file(&meta).unwrap();
    let mut p = f.provider();
    p.models.clear();
    p.default_model.clear();
    assert!(claude_desktop::render(FileKind::Profile, None, &p).is_err());
    p = f.provider();
    p.api_key.clear();
    assert!(claude_desktop::render(FileKind::Profile, None, &p).is_err());
    assert!(claude_desktop::threep_dir(&f.root().with_file_name("Claude-3p")).is_err());
    let mut overlap = settings.clone();
    overlap.environments[1].path = meta.parent().unwrap().to_string_lossy().into();
    assert!(environments::save_at(&f.state(), overlap).is_err());
    environments::deploy_at(&f.state(), &settings).unwrap();
    fs::write(&meta, b"{\"entries\":[],\"external\":true}").unwrap();
    assert!(environments::sync_claude_provider_at(&f.state(), &f.provider()).is_err());
    assert!(environments::restore_selected_at(&f.state(), &["claude-desktop".into()]).is_err());
    assert_eq!(json_at(meta)["external"], true);
}

#[cfg(unix)]
#[test]
fn desktop_sibling_symlink_is_rejected() {
    let f = Fixture::new();
    f.save();
    let outside = f.0.join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.root().with_file_name("Claude-3p")).unwrap();
    assert!(environments::deploy_at(&f.state(), &f.settings()).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn six_client_settings_upgrade_without_changing_choices_and_platform_candidates() {
    let f = Fixture::new();
    let mut settings = f.settings();
    settings.environments.pop();
    fs::create_dir_all(f.state()).unwrap();
    fs::write(
        f.state().join("environments.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let upgraded = environments::load_at(&f.state()).unwrap();
    assert_eq!(upgraded.environments.len(), 7);
    assert_eq!(
        serde_json::to_value(&upgraded.environments[..6]).unwrap(),
        serde_json::to_value(settings.environments).unwrap()
    );
    assert!(!upgraded.environments[6].enabled);
    assert_eq!(
        environments::selected_pack(&upgraded, "claude-desktop"),
        Some("astra-claude")
    );
    assert_eq!(
        claude_desktop::candidates(f.root().parent().unwrap()),
        vec![f.root()]
    );
    let base = f.0.join("LocalAppData");
    fs::create_dir_all(base.join("ClaudeDev")).unwrap();
    fs::create_dir_all(base.join("ClaudeDev-3p")).unwrap();
    assert_eq!(
        claude_desktop::candidates(&base),
        vec![base.join("ClaudeDev")]
    );
}

#[tokio::test]
async fn desktop_http_models_activation_messages_and_credentials_share_production_pipeline() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let upstream=axum::Router::new().fallback(axum::routing::any(move |method:http::Method, uri:http::Uri, headers:http::HeaderMap, body:axum::body::Bytes| {
        let record=record.clone();async move {
            assert_eq!(headers["authorization"],"Bearer dummy-upstream-key");assert!(!headers.contains_key("x-julong-environment"));
            record.lock().unwrap().push((method.clone(),uri.to_string(),body.to_vec()));
            if method==http::Method::GET { axum::Json(json!({"data":[{"id":"claude-sonnet-4-6","type":"model","display_name":"Sonnet"}],"has_more":false})) }
            else { axum::Json(json!({"type":"message","role":"assistant","content":[{"type":"text","text":"OK"}]})) }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upaddr = listener.local_addr().unwrap();
    let up = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let core = Arc::new(
        MitmCore::builder()
            .target(format!("http://{upaddr}/v1"))
            .anthropic_api_key(Some("dummy-upstream-key".into()))
            .activation_gate(ActivationGate::new(BTreeMap::from([
                ("claude-desktop".into(), "DESKTOP_ONLY_PACK".into()),
                ("claude".into(), "CODE_ONLY_PACK".into()),
            ])))
            .response_parser(UniversalSseParser)
            .build()
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        axum::serve(listener, proxy_router(core)).await.unwrap();
    });
    let client = reqwest::Client::new();
    let response: Value = client
        .get(format!("http://{addr}/claude-desktop/v1/models?limit=5"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["data"][0]["id"], "claude-sonnet-4-6");
    assert!(seen.lock().unwrap()[0].2.is_empty());
    assert_eq!(seen.lock().unwrap()[0].1, "/v1/models?limit=5");
    for (route, prompt, session, expected_pack) in [
        ("claude-desktop/v1/messages", "矩龙", "same", None),
        ("claude-desktop/v1/messages", "hello", "same", Some(true)),
        ("v1/messages", "hello", "same", Some(false)),
        ("claude-desktop/v1/messages", "hello", "new", Some(false)),
    ] {
        let response: Value = client
            .post(format!("http://{addr}/{route}"))
            .header("x-session-id", session)
            .json(
                &json!({"model":"claude-sonnet-4-6","messages":[{"role":"user","content":prompt}]}),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(expected) = expected_pack {
            let body = String::from_utf8(seen.lock().unwrap().last().unwrap().2.clone()).unwrap();
            assert_eq!(body.contains("DESKTOP_ONLY_PACK"), expected);
            assert!(!body.contains("CODE_ONLY_PACK"));
        } else {
            assert_eq!(
                response["content"][0]["text"],
                julong_codex_keysmith::activation::REPLY
            );
            assert_eq!(seen.lock().unwrap().len(), 1);
        }
    }
    let f = Fixture::new();
    let mut provider = f.provider();
    provider.request_url = format!("http://{upaddr}");
    provider.api_key = "dummy-upstream-key".into();
    assert!(providers::test_claude(&provider)
        .await
        .unwrap()
        .contains("验收通过"));
    proxy.abort();
    up.abort();
}

#[test]
fn legacy_manifest_can_sync_and_restore_without_redeploying_other_clients() {
    let f = Fixture::new();
    f.save();
    let mut settings = f.settings();
    settings
        .environments
        .iter_mut()
        .find(|e| e.id == "claude-desktop")
        .unwrap()
        .enabled = false;
    environments::deploy_at(&f.state(), &settings).unwrap();
    let path = f.state().join("environment-deployment.json");
    let mut manifest = json_at(path.clone());
    manifest["settings"]["environments"]
        .as_array_mut()
        .unwrap()
        .pop();
    for entry in manifest["entries"].as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("owner");
    }
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    environments::save_at(&f.state(), settings).unwrap();
    let mut provider = f.provider();
    provider.api_key = "dummy-new-key".into();
    environments::sync_claude_provider_at(&f.state(), &provider).unwrap();
    assert_eq!(
        json_at(f.0.join("code/settings.json"))["env"]["ANTHROPIC_AUTH_TOKEN"],
        "dummy-new-key"
    );
    environments::restore_selected_at(&f.state(), &["claude".into()]).unwrap();
    assert!(!f.0.join("code/settings.json").exists());
    assert!(!profile(&f).exists());
}

#[tokio::test]
async fn claude_protocol_probe_rejects_openai_shaped_success() {
    let app = axum::Router::new()
        .fallback(|| async { axum::Json(json!({"choices":[{"message":{"content":"OK"}}]})) });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let f = Fixture::new();
    let mut provider = f.provider();
    provider.request_url = format!("http://{addr}");
    assert!(providers::test_claude(&provider)
        .await
        .unwrap_err()
        .contains("不是 Anthropic"));
    task.abort();
}

#[test]
fn provider_categories_preserve_legacy_and_only_claude_can_update_claude_files() {
    let f = Fixture::new();
    f.save();
    let claude = f.provider();
    let legacy:Provider=serde_json::from_value(json!({"id":"legacy","name":"OpenAI","request_url":"https://openai.example","api_key":"openai-key"})).unwrap();
    assert_eq!(legacy.category, "openai");
    providers::save_list(
        &f.state().join("control"),
        &[legacy.clone(), claude.clone()],
    )
    .unwrap();
    assert_eq!(
        providers::selected_claude_at(&f.state().join("control"))
            .unwrap()
            .unwrap()
            .id,
        claude.id
    );
    environments::deploy_at(&f.state(), &f.settings()).unwrap();
    let before = fs::read(profile(&f)).unwrap();
    environments::sync_claude_provider_at(&f.state(), &legacy).unwrap();
    assert_eq!(fs::read(profile(&f)).unwrap(), before);
    assert_eq!(
        json_at(profile(&f))["inferenceGatewayApiKey"],
        "dummy-key-one"
    );
    providers::save_list(&f.state().join("control"), &[claude, legacy.clone()]).unwrap();
    assert_eq!(
        providers::configured_relay_url(&f.state().join("control")),
        Some(legacy.normalized_url())
    );
}

#[tokio::test]
async fn category_routing_switches_independently_and_never_leaks_keys_to_other_group() {
    let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let record = seen.clone();
    let app = axum::Router::new().fallback(axum::routing::any(
        move |uri: http::Uri, headers: http::HeaderMap| {
            let record = record.clone();
            async move {
                record.lock().unwrap().push((
                    uri.to_string(),
                    headers["authorization"].to_str().unwrap().to_owned(),
                ));
                axum::Json(json!({"ok":true}))
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let f = Fixture::new();
    let mut claude = f.provider();
    claude.request_url = format!("http://{addr}/claude-one");
    claude.full_url = true;
    let mut openai = claude.clone();
    openai.category = "openai".into();
    openai.request_url = format!("http://{addr}/openai-one");
    let core = MitmCore::builder()
        .target("unused")
        .openai_provider(Some(&openai))
        .claude_provider(Some(&claude))
        .response_parser(UniversalSseParser)
        .build()
        .unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer original-openai-key".parse().unwrap(),
    );
    for path in [
        "/v1/responses",
        "/claude-desktop/v1/messages",
        "/v1/messages",
    ] {
        core.handle_request(
            http::Method::POST,
            path.into(),
            headers.clone(),
            b"{}".as_slice().into(),
        )
        .await
        .unwrap();
    }
    claude.request_url = format!("http://{addr}/claude-two");
    claude.api_key = "dummy-key-two".into();
    core.update_category(&claude).await;
    core.handle_request(
        http::Method::POST,
        "/v1/responses".into(),
        headers.clone(),
        b"{}".as_slice().into(),
    )
    .await
    .unwrap();
    core.handle_request(
        http::Method::POST,
        "/v1/messages".into(),
        headers.clone(),
        b"{}".as_slice().into(),
    )
    .await
    .unwrap();
    openai.request_url = format!("http://{addr}/openai-two");
    core.update_category(&openai).await;
    core.handle_request(
        http::Method::POST,
        "/v1/messages".into(),
        headers.clone(),
        b"{}".as_slice().into(),
    )
    .await
    .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            (
                "/openai-one/v1/responses".into(),
                "Bearer original-openai-key".into()
            ),
            (
                "/claude-one/v1/messages".into(),
                "Bearer dummy-key-one".into()
            ),
            (
                "/claude-one/v1/messages".into(),
                "Bearer dummy-key-one".into()
            ),
            (
                "/openai-one/v1/responses".into(),
                "Bearer original-openai-key".into()
            ),
            (
                "/claude-two/v1/messages".into(),
                "Bearer dummy-key-two".into()
            ),
            (
                "/claude-two/v1/messages".into(),
                "Bearer dummy-key-two".into()
            )
        ]
    );
    core.clear_claude_provider().await;
    assert!(core
        .handle_request(
            http::Method::POST,
            "/v1/messages".into(),
            headers.clone(),
            b"{}".as_slice().into()
        )
        .await
        .is_err());
    core.clear_openai_provider().await;
    assert!(core
        .handle_request(
            http::Method::POST,
            "/v1/responses".into(),
            headers,
            b"{}".as_slice().into()
        )
        .await
        .is_err());
    assert_eq!(seen.lock().unwrap().len(), 6);
    task.abort();
}
