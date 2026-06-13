//! End-to-end tests for the CLI binary.
//!
//! Each test runs the compiled binary with an isolated HOME/XDG_CONFIG_HOME
//! so the real user config and credentials are never touched.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TestEnv {
    home: PathBuf,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "usage-monitor-cli-test-{}-{}",
            name,
            std::process::id()
        ));
        std::fs::create_dir_all(&home).unwrap();
        Self { home }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_usage-monitor-cli"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_HOME")
            .output()
            .expect("run binary")
    }

    fn config_path(&self) -> PathBuf {
        self.home.join(".config/usage-monitor/config.toml")
    }

    fn write_claude_credentials(&self) {
        let dir = self.home.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"at","refreshToken":"rt","expiresAt":99999999999999.0}}"#,
        )
        .unwrap();
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.home).ok();
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn test_list_shows_all_providers_auto_disabled_without_credentials() {
    let env = TestEnv::new("list-auto");
    let out = env.run(&["list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    for id in ["anthropic", "claude", "codex", "openai", "opencode-go"] {
        assert!(text.contains(id), "missing {} in: {}", id, text);
    }
    // Fresh HOME has no credentials → everything auto-disabled.
    assert_eq!(text.matches("disabled (auto)").count(), 5, "got: {}", text);
}

#[test]
fn test_list_auto_enables_detected_credentials() {
    let env = TestEnv::new("list-detected");
    env.write_claude_credentials();
    let out = env.run(&["list"]);
    let text = stdout(&out);
    assert!(
        text.contains("claude       enabled (auto)"),
        "got: {}",
        text
    );
    assert_eq!(text.matches("disabled (auto)").count(), 4, "got: {}", text);
}

#[test]
fn test_enable_persists_and_list_reflects_it() {
    let env = TestEnv::new("enable");
    let out = env.run(&["enable", "openai"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("openai: enabled"));

    let config = std::fs::read_to_string(env.config_path()).unwrap();
    assert!(config.contains("[providers.openai]"));
    assert!(config.contains("enabled = true"));

    let text = stdout(&env.run(&["list"]));
    assert!(text.contains("openai       enabled "), "got: {}", text);
}

#[test]
fn test_disable_blocks_explicit_fetch() {
    let env = TestEnv::new("disable-fetch");
    env.run(&["disable", "codex"]);

    let out = env.run(&["fetch", "codex"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("provider 'codex' is disabled"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn test_auto_clears_explicit_toggle() {
    let env = TestEnv::new("auto-clear");
    env.run(&["disable", "claude"]);
    assert!(
        std::fs::read_to_string(env.config_path())
            .unwrap()
            .contains("enabled = false")
    );

    let out = env.run(&["auto", "claude"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("auto (currently disabled)"));
    assert!(
        !std::fs::read_to_string(env.config_path())
            .unwrap()
            .contains("enabled = false")
    );

    let text = stdout(&env.run(&["list"]));
    assert!(
        text.contains("claude       disabled (auto)"),
        "got: {}",
        text
    );
}

#[test]
fn test_fetch_all_without_enabled_providers() {
    let env = TestEnv::new("fetch-empty");
    let out = env.run(&["fetch"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("No enabled providers"));
}

#[test]
fn test_provider_config_set_show_unset() {
    let env = TestEnv::new("config-set");
    let out = env.run(&["opencode-go", "set", "token", "session=secret-cookie-value"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // Secret values are masked in output.
    assert!(!stdout(&out).contains("secret-cookie-value"));

    // Stored flat in [providers.opencode-go], no .config subtable.
    let raw = std::fs::read_to_string(env.config_path()).unwrap();
    assert!(
        raw.contains(r#"token = "session=secret-cookie-value""#),
        "got: {}",
        raw
    );
    assert!(
        !raw.contains("[providers.opencode-go.config]"),
        "got: {}",
        raw
    );

    let out = env.run(&["opencode-go", "show"]);
    assert!(stdout(&out).contains("token = session="));
    assert!(!stdout(&out).contains("secret-cookie-value"));

    env.run(&["opencode-go", "unset", "token"]);
    let out = env.run(&["opencode-go", "show"]);
    assert!(stdout(&out).contains("provider = opencode-go"));
}

#[test]
fn test_workspace_add_remove_list() {
    let env = TestEnv::new("workspace");
    let out = env.run(&["opencode-go", "workspace", "add", "wrk_first"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let out = env.run(&["opencode-go", "workspace", "add", "wrk_first"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("already configured"));

    // URL form is accepted and normalized.
    let out = env.run(&[
        "opencode-go",
        "workspace",
        "add",
        "https://opencode.ai/workspace/wrk_second/go",
    ]);
    assert!(
        stdout(&out).contains("wrk_first, wrk_second"),
        "got: {}",
        stdout(&out)
    );

    // Persisted as a real TOML array (pretty serializer emits it multi-line).
    let raw = std::fs::read_to_string(env.config_path()).unwrap();
    assert!(raw.contains("workspaces = ["), "got: {}", raw);
    assert!(raw.contains(r#""wrk_first""#), "got: {}", raw);
    assert!(raw.contains(r#""wrk_second""#), "got: {}", raw);

    let out = env.run(&["opencode-go", "workspace", "list"]);
    assert_eq!(stdout(&out).trim(), "wrk_first\nwrk_second");

    env.run(&["opencode-go", "workspace", "remove", "wrk_first"]);
    let out = env.run(&["opencode-go", "workspace", "list"]);
    assert_eq!(stdout(&out).trim(), "wrk_second");

    env.run(&["opencode-go", "workspace", "remove", "wrk_second"]);
    let out = env.run(&["opencode-go", "workspace", "list"]);
    assert!(stdout(&out).contains("auto-discovery"));

    let out = env.run(&["opencode-go", "workspace", "remove", "wrk_second"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not configured"));

    // Optional display name persists as `id=Name` and shows in list.
    env.run(&["opencode-go", "workspace", "add", "wrk_named", "Production"]);
    let raw = std::fs::read_to_string(env.config_path()).unwrap();
    assert!(raw.contains(r#""wrk_named=Production""#), "got: {}", raw);
    let out = env.run(&["opencode-go", "workspace", "list"]);
    assert!(stdout(&out).contains("wrk_named"), "got: {}", stdout(&out));
    assert!(stdout(&out).contains("Production"), "got: {}", stdout(&out));
    env.run(&["opencode-go", "workspace", "remove", "wrk_named"]);

    // Invalid reference fails instead of silently wiping.
    let out = env.run(&["opencode-go", "workspace", "add", "not-a-workspace"]);
    assert!(!out.status.success());

    let out = env.run(&[
        "opencode-go",
        "workspace",
        "add",
        "wrk_comma",
        "Client, Production",
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("cannot contain comma"));
}

#[test]
fn test_enable_unknown_provider_fails() {
    let env = TestEnv::new("unknown");
    let out = env.run(&["enable", "ghost"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown provider 'ghost'"));
}
