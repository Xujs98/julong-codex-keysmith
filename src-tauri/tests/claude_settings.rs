use julong_codex_keysmith::environments::{self, Environment, Settings};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("julong-claude-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join("Claude 配置 with spaces")).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn settings(&self) -> Settings {
        Settings {
            schema_version: 1,
            profiles: vec!["astra-claude".into()],
            environments: environments::IDS
                .iter()
                .map(|id| Environment {
                    id: (*id).into(),
                    enabled: *id == "claude",
                    path: if *id == "claude" {
                        self.0
                            .join("Claude 配置 with spaces")
                            .to_string_lossy()
                            .into()
                    } else {
                        String::new()
                    },
                })
                .collect(),
        }
    }
    fn file(&self) -> PathBuf {
        self.0.join("Claude 配置 with spaces/settings.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn claude_settings_deploy_and_selected_restore() {
    let f = Fixture::new();
    let state = f.0.join("state");
    let settings = f.settings();
    let original = b"{\r\n  \"env\": {\"ANTHROPIC_BASE_URL\":\"https://original.example\",\"ANTHROPIC_AUTH_TOKEN\":\"fixture-only\",\"ANTHROPIC_MODEL\":\"custom-sonnet\",\"ANTHROPIC_CUSTOM_HEADERS\":\"X-Team: example\"},\r\n  \"model\":\"custom-sonnet\",\"permissions\":{\"deny\":[\"Bash(rm *)\"]},\"hooks\":{}\r\n}\r\n";
    fs::write(f.file(), original).unwrap();
    environments::save_at(&state, settings.clone()).unwrap();
    environments::deploy_at(&state, &settings).unwrap();
    let bytes = fs::read(f.file()).unwrap();
    let deployed: Value = serde_json::from_slice(&bytes).unwrap();
    println!(
        "OBSERVED after deploy: ANTHROPIC_BASE_URL={}",
        deployed["env"]["ANTHROPIC_BASE_URL"]
    );
    assert_eq!(
        deployed["env"]["ANTHROPIC_BASE_URL"],
        "http://127.0.0.1:8080"
    );
    assert_eq!(deployed["env"]["ANTHROPIC_AUTH_TOKEN"], "fixture-only");
    assert_eq!(deployed["env"]["ANTHROPIC_MODEL"], "custom-sonnet");
    assert_eq!(deployed["model"], "custom-sonnet");
    assert_eq!(deployed["permissions"], json!({"deny":["Bash(rm *)"]}));
    assert_eq!(deployed["hooks"], json!({}));
    assert_eq!(
        deployed["env"]["ANTHROPIC_CUSTOM_HEADERS"],
        "X-Team: example\nx-julong-environment: claude"
    );
    environments::deploy_at(&state, &settings).unwrap();
    assert_eq!(fs::read(f.file()).unwrap(), bytes);
    environments::restore_selected_configuration_at(&state, &["claude".into()]).unwrap();
    assert_eq!(fs::read(f.file()).unwrap(), original);
    println!("OBSERVED restore: original settings bytes restored; repeat deployment idempotent");
}

#[test]
fn claude_fresh_invalid_conflicted_and_deselected_settings() {
    let f = Fixture::new();
    let state = f.0.join("state");
    let mut settings = f.settings();
    environments::deploy_at(&state, &settings).unwrap();
    let deployed: Value = serde_json::from_slice(&fs::read(f.file()).unwrap()).unwrap();
    assert_eq!(
        deployed["env"]["ANTHROPIC_AUTH_TOKEN"],
        "julong-local-proxy"
    );
    assert!(deployed.get("model").is_none());
    settings.environments[1].enabled = false;
    environments::deploy_at(&state, &settings).unwrap();
    assert!(!f.file().exists());
    settings.environments[1].enabled = true;
    for invalid in [
        "broken",
        "[]",
        "{\"env\":[]}",
        "{\"env\":{\"ANTHROPIC_CUSTOM_HEADERS\":[]}}",
    ] {
        fs::write(f.file(), invalid).unwrap();
        assert!(environments::deploy_at(&state, &settings).is_err());
        assert_eq!(fs::read_to_string(f.file()).unwrap(), invalid);
    }
    fs::write(f.file(), "{}").unwrap();
    environments::deploy_at(&state, &settings).unwrap();
    fs::write(f.file(), "{\"userChanged\":true}").unwrap();
    assert!(environments::deploy_at(&state, &settings).is_err());
    assert!(environments::restore_selected_at(&state, &["claude".into()]).is_err());
    assert_eq!(
        fs::read_to_string(f.file()).unwrap(),
        "{\"userChanged\":true}"
    );
}

#[cfg(unix)]
#[test]
fn claude_symlinked_settings_are_rejected_before_any_writes() {
    let f = Fixture::new();
    let outside = f.0.join("outside.json");
    fs::write(&outside, "{}").unwrap();
    std::os::unix::fs::symlink(&outside, f.file()).unwrap();
    assert!(environments::deploy_at(&f.0.join("state"), &f.settings()).is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "{}");
    assert!(!f.file().with_file_name("CLAUDE.md").exists());
}
