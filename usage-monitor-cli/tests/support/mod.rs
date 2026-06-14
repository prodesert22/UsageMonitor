use std::path::PathBuf;
use std::process::{Command, Output};

pub struct TestEnv {
    home: PathBuf,
}

impl TestEnv {
    pub fn new(name: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "usage-monitor-cli-test-{}-{}",
            name,
            std::process::id()
        ));
        std::fs::create_dir_all(&home).unwrap();
        Self { home }
    }

    pub fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_usage-monitor-cli"));
        cmd.args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"));
        for key in ENV_VARS_TO_CLEAR {
            cmd.env_remove(key);
        }
        cmd.output().expect("run binary")
    }

    pub fn config_path(&self) -> PathBuf {
        self.home.join(".config/usage-monitor/config.toml")
    }

    pub fn write_claude_credentials(&self) {
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

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

const ENV_VARS_TO_CLEAR: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "CODEX_HOME",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "AZURE_OPENAI_DEPLOYMENT_NAME",
    "CURSOR_COOKIE",
    "CURSOR_SESSION_TOKEN",
    "OPENCODE_COOKIE",
    "OPENCODE_SESSION_TOKEN",
    "ALIBABA_CODING_PLAN_API_KEY",
    "ALIBABA_CODING_PLAN_COOKIE",
    "ALIBABA_TOKEN_PLAN_COOKIE",
    "FACTORY_COOKIE",
    "FACTORY_TOKEN",
    "GEMINI_CREDENTIALS",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_PROJECT",
    "GITHUB_TOKEN",
    "COPILOT_TOKEN",
    "DEVIN_TOKEN",
    "DEVIN_COOKIE",
    "Z_AI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_TOKEN",
    "MINIMAX_COOKIE",
    "MANUS_SESSION_TOKEN",
    "MANUS_COOKIE",
    "KIMI_AUTH_TOKEN",
    "KIMI_COOKIE",
    "KILO_API_KEY",
    "AUGMENT_TOKEN",
    "AUGMENT_COOKIE",
    "KIMI_K2_API_KEY",
    "KIMI_API_KEY",
    "MOONSHOT_API_KEY",
    "MOONSHOT_KEY",
    "AMP_ACCESS_TOKEN",
    "AMP_API_KEY",
    "AMP_COOKIE",
    "T3CHAT_COOKIE",
    "T3CHAT_SESSION_TOKEN",
    "OLLAMA_API_KEY",
    "OLLAMA_COOKIE",
    "SYNTHETIC_API_KEY",
    "WARP_API_KEY",
    "WARP_TOKEN",
    "OPENROUTER_API_KEY",
    "ELEVENLABS_API_KEY",
    "XI_API_KEY",
    "WINDSURF_TOKEN",
    "WINDSURF_COOKIE",
    "PERPLEXITY_SESSION_TOKEN",
    "PERPLEXITY_COOKIE",
    "MIMO_COOKIE",
    "XIAOMI_MIMO_COOKIE",
    "ARK_API_KEY",
    "VOLCENGINE_API_KEY",
    "DOUBAO_API_KEY",
    "ABACUS_COOKIE",
    "ABACUS_TOKEN",
    "MISTRAL_COOKIE",
    "MISTRAL_SESSION",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_KEY",
    "CODEBUFF_API_KEY",
    "CROF_API_KEY",
    "CROFAI_API_KEY",
    "VENICE_API_KEY",
    "VENICE_KEY",
    "COMMANDCODE_COOKIE",
    "COMMAND_CODE_COOKIE",
    "STEPFUN_USERNAME",
    "STEPFUN_PASSWORD",
    "STEPFUN_OASIS_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "CODEXBAR_BEDROCK_BUDGET",
    "GROK_COOKIE",
    "GROK_TOKEN",
    "GROK_ACCESS_TOKEN",
    "WINDSURF_SESSION_TOKEN",
    "WINDSURF_AUTH1_TOKEN",
    "WINDSURF_ACCOUNT_ID",
    "WINDSURF_PRIMARY_ORG_ID",
    "GROQ_API_KEY",
    "GROQ_TOKEN",
    "LLM_PROXY_API_KEY",
    "LLM_PROXY_BASE_URL",
    "DEEPGRAM_API_KEY",
    "DEEPGRAM_PROJECT_ID",
];
