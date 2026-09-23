//! Built-in Gemini OAuth login (PKCE) for the CLI.
//!
//! Ports the OAuth flow used by the open-source `opencode-gemini-auth` plugin:
//! builds an authorization URL, runs a local callback listener on
//! `http://localhost:8085/oauth2callback`, exchanges the returned code for
//! access/refresh tokens, resolves the account email, and writes the
//! gemini-cli-compatible credentials file (`~/.gemini/oauth_creds.json`).
//!
//! The credentials layout matches `GeminiProvider` in `gemini.rs`, so
//! `usage-monitor-cli fetch gemini` picks up the login without any extra
//! configuration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

use crate::error::SpendPanelError;
use crate::provider::gemini::{GEMINI_CLI_CLIENT_ID, GEMINI_CLI_CLIENT_SECRET};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
const REDIRECT_URI: &str = "http://localhost:8085/oauth2callback";
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform \
https://www.googleapis.com/auth/userinfo.email \
https://www.googleapis.com/auth/userinfo.profile";
const MAX_REQUEST_HEAD: usize = 8 * 1024;

/// Deterministic PKCE/state values, used by tests so the flow is reproducible.
pub struct LoginInjection {
    pub(crate) state: String,
    pub(crate) verifier: String,
    pub(crate) challenge: String,
}

/// Tokens returned by the token exchange.
#[derive(Debug)]
pub(crate) struct TokenResult {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_in: u64,
}

/// Result of a completed login.
#[derive(Debug)]
pub struct LoginOutcome {
    pub email: Option<String>,
    pub creds_path: PathBuf,
    pub expires_ms: f64,
}

/// OAuth endpoints + credentials location for the Gemini login flow.
pub struct GeminiOAuth {
    client_id: String,
    client_secret: String,
    authorize_url: String,
    token_url: String,
    userinfo_url: String,
    redirect_uri: String,
    creds_path: PathBuf,
    timeout: Duration,
}

impl GeminiOAuth {
    /// Production endpoints and the default `~/.gemini/oauth_creds.json` path.
    pub fn prod() -> Self {
        Self {
            client_id: GEMINI_CLI_CLIENT_ID.to_string(),
            client_secret: GEMINI_CLI_CLIENT_SECRET.to_string(),
            authorize_url: AUTHORIZE_URL.into(),
            token_url: TOKEN_URL.into(),
            userinfo_url: USERINFO_URL.into(),
            redirect_uri: REDIRECT_URI.into(),
            creds_path: Self::resolve_creds_path(None),
            timeout: Duration::from_secs(5 * 60),
        }
    }

    /// Test constructor pointing token/userinfo at a wiremock server and using
    /// fixed client credentials.
    #[cfg(test)]
    pub(crate) fn with_endpoints(
        token_url: String,
        userinfo_url: String,
        redirect_uri: String,
        creds_path: PathBuf,
    ) -> Self {
        Self {
            client_id: "test-client-id".into(),
            client_secret: "test-client-secret".into(),
            authorize_url: AUTHORIZE_URL.into(),
            token_url,
            userinfo_url,
            redirect_uri,
            creds_path,
            timeout: Duration::from_secs(5 * 60),
        }
    }

    /// Overrides the credentials path (used by the login command when a
    /// `credentials_path` is configured).
    pub fn with_creds_path(mut self, creds_path: PathBuf) -> Self {
        self.creds_path = creds_path;
        self
    }

    /// Resolves the credentials file: an explicit configured path wins,
    /// otherwise `$HOME/.gemini/oauth_creds.json` (mirrors `gemini.rs`).
    pub fn resolve_creds_path(configured: Option<&str>) -> PathBuf {
        match configured.map(str::trim).filter(|v| !v.is_empty()) {
            Some(path) => crate::provider::expand_credentials_path(path),
            None => {
                let home = std::env::var("HOME").unwrap_or_default();
                Path::new(&home).join(".gemini/oauth_creds.json")
            }
        }
    }

    /// Generates a PKCE (verifier, challenge) pair: 32 random bytes, base64url
    /// NO_PAD verifier and the S256 challenge.
    pub(crate) fn generate_pkce_pair() -> (String, String) {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("OS randomness unavailable");
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        (verifier, challenge)
    }

    /// Generates a 32-byte random state as lowercase hex.
    pub(crate) fn generate_state() -> String {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("OS randomness unavailable");
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Builds the Google authorization URL with PKCE and a `#usage-monitor`
    /// fragment so stray terminal glyphs are ignored by the auth server.
    fn build_authorize_url(&self, challenge: &str, state: &str) -> String {
        let mut url = Url::parse(&self.authorize_url).expect("authorize_url must parse");
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        url.set_fragment(Some("usage-monitor"));
        url.to_string()
    }

    /// Binds the local callback listener on 127.0.0.1 at the port parsed from
    /// `redirect_uri`. A busy port surfaces as an error the login flow treats
    /// as "fall back to paste mode".
    pub(crate) async fn start_callback_listener(
        &self,
    ) -> Result<CallbackListener, SpendPanelError> {
        let url = Url::parse(&self.redirect_uri)
            .map_err(|e| SpendPanelError::ConfigError(format!("invalid redirect_uri: {e}")))?;
        let port = url.port().unwrap_or(80);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
            SpendPanelError::AuthFailed(
                "gemini".into(),
                format!(
                    "could not start the local callback listener on port {port}: {e} \
                     — you'll need to paste the callback URL or authorization code"
                ),
            )
        })?;
        Ok(CallbackListener { listener })
    }

    /// Exchanges an authorization code for tokens.
    async fn exchange_code(
        &self,
        client: &reqwest::Client,
        code: &str,
        verifier: &str,
    ) -> Result<TokenResult, SpendPanelError> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", verifier),
        ];
        let resp = client
            .post(&self.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                format!("token exchange failed (HTTP {}): {}", status.as_u16(), body),
            ));
        }
        #[derive(serde::Deserialize)]
        struct TokenResp {
            access_token: Option<String>,
            expires_in: Option<u64>,
            refresh_token: Option<String>,
        }
        let parsed: TokenResp = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;
        let access_token = parsed
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                SpendPanelError::AuthFailed(
                    "gemini".into(),
                    "missing access token in response".into(),
                )
            })?;
        let refresh_token = parsed
            .refresh_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                SpendPanelError::AuthFailed(
                    "gemini".into(),
                    "missing refresh token in response".into(),
                )
            })?;
        Ok(TokenResult {
            access_token,
            refresh_token,
            expires_in: parsed.expires_in.unwrap_or(3600),
        })
    }

    /// Best-effort email lookup from the userinfo endpoint.
    async fn fetch_email(&self, client: &reqwest::Client, access_token: &str) -> Option<String> {
        #[derive(serde::Deserialize)]
        struct UserInfo {
            email: Option<String>,
        }
        let resp = client
            .get(&self.userinfo_url)
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let info: UserInfo = resp.json().await.ok()?;
        info.email.filter(|e| !e.trim().is_empty())
    }

    /// Writes the credentials file (0600, overwriting), returning its path.
    fn write_creds(&self, token: &TokenResult) -> Result<PathBuf, SpendPanelError> {
        #[derive(serde::Serialize)]
        struct CredsFile<'a> {
            access_token: &'a str,
            refresh_token: &'a str,
            expiry_date: f64,
        }
        if let Some(parent) = self.creds_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| SpendPanelError::ConfigError(format!("create creds dir: {e}")))?;
        }
        let creds = CredsFile {
            access_token: &token.access_token,
            refresh_token: &token.refresh_token,
            expiry_date: now_ms() + token.expires_in as f64 * 1000.0,
        };
        let json = serde_json::to_string(&creds)
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&self.creds_path).map_err(|e| {
            SpendPanelError::ConfigError(format!("open {}: {e}", self.creds_path.display()))
        })?;
        use std::io::Write;
        file.write_all(json.as_bytes()).map_err(|e| {
            SpendPanelError::ConfigError(format!("write {}: {e}", self.creds_path.display()))
        })?;
        Ok(self.creds_path.clone())
    }

    /// Runs the full login flow. When `open_browser` is true the authorization
    /// URL is opened with `xdg-open` (non-headless only).
    pub async fn login(
        &self,
        open_browser: bool,
        inject: Option<LoginInjection>,
    ) -> Result<LoginOutcome, SpendPanelError> {
        let (state, verifier, challenge) = match inject {
            Some(injection) => (injection.state, injection.verifier, injection.challenge),
            None => {
                let (verifier, challenge) = Self::generate_pkce_pair();
                (Self::generate_state(), verifier, challenge)
            }
        };

        if is_headless() {
            return self.paste_login(&state, &verifier, &challenge).await;
        }

        let authorize_url = self.build_authorize_url(&challenge, &state);

        let listener = match self.start_callback_listener().await {
            Ok(listener) => {
                println!("Open this URL to log in: {authorize_url}");
                if open_browser {
                    Self::open_browser(&authorize_url);
                }
                println!("Waiting for the browser callback…");
                Some(listener)
            }
            Err(e) => {
                println!("Warning: {e}\nOpen this URL to log in: {authorize_url}");
                None
            }
        };

        let Some(listener) = listener else {
            return self.paste_login(&state, &verifier, &challenge).await;
        };

        let client = Self::build_client(self.timeout)?;
        loop {
            let code = listener.wait_for_callback(&state, self.timeout).await?;
            match self.exchange_code(&client, &code, &verifier).await {
                Ok(token) => return self.finalize(&client, token).await,
                Err(e) if should_ignore_malformed_auth_code(&e) => {
                    println!(
                        "Received a malformed authorization code; waiting for the next redirect…"
                    );
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Exchange + email + write, shared by the listener and paste paths.
    async fn finalize(
        &self,
        client: &reqwest::Client,
        token: TokenResult,
    ) -> Result<LoginOutcome, SpendPanelError> {
        let email = self.fetch_email(client, &token.access_token).await;
        let expires_ms = now_ms() + token.expires_in as f64 * 1000.0;
        let path = self.write_creds(&token)?;
        match &email {
            Some(email) => println!(
                "Logged in as {email}; credentials saved to {}",
                path.display()
            ),
            None => println!("Logged in; credentials saved to {}", path.display()),
        }
        Ok(LoginOutcome {
            email,
            creds_path: path,
            expires_ms,
        })
    }

    /// Headless / no-listener fallback: print the URL and read pasted input.
    async fn paste_login(
        &self,
        state: &str,
        verifier: &str,
        challenge: &str,
    ) -> Result<LoginOutcome, SpendPanelError> {
        println!(
            "Headless environment detected. Open this URL to log in: {}",
            self.build_authorize_url(challenge, state)
        );
        println!(
            "Paste the full redirected URL (e.g. {}?code=...&state=...) \
             or just the authorization code:",
            self.redirect_uri
        );
        let input = read_pasted_input()?;
        let (code, pasted_state) = parse_oauth_callback_input(&input).ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "gemini".into(),
                "no authorization code found in pasted input".into(),
            )
        })?;
        if let Some(pasted) = pasted_state
            && pasted != state
        {
            return Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                "state mismatch in callback input (possible CSRF attempt)".into(),
            ));
        }
        let client = Self::build_client(self.timeout)?;
        let token = self.exchange_code(&client, &code, verifier).await?;
        self.finalize(&client, token).await
    }

    /// Opens the URL in the default browser (best-effort, detached).
    pub(crate) fn open_browser(url: &str) {
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(url);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        if let Ok(_child) = cmd.spawn() {
            // Detached: let the browser outlive the CLI without waiting on it.
        }
    }

    fn build_client(timeout: Duration) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }
}

/// Local HTTP listener that captures the OAuth redirect.
pub(crate) struct CallbackListener {
    listener: tokio::net::TcpListener,
}

impl CallbackListener {
    /// The bound socket address (useful in tests).
    #[cfg(test)]
    pub(crate) fn local_addr(&self) -> std::net::SocketAddr {
        self.listener
            .local_addr()
            .expect("bound listener has a local address")
    }

    /// Waits for a callback carrying the expected state, returning the
    /// authorization `code`. Mismatched/incomplete callbacks are answered and
    /// skipped; an `error` param fails immediately; the whole wait is bounded
    /// by `timeout`.
    pub(crate) async fn wait_for_callback(
        &self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<String, SpendPanelError> {
        let inner = async {
            loop {
                let (mut socket, _peer) = self.listener.accept().await.map_err(|e| {
                    SpendPanelError::NetworkError(format!("callback listener accept: {e}"))
                })?;
                let head = read_request_head(&mut socket).await?;
                match classify_callback(&head, expected_state) {
                    CallbackDecision::Code(code) => {
                        respond(
                            &mut socket,
                            "200 OK",
                            "text/html; charset=utf-8",
                            SUCCESS_HTML,
                        )
                        .await?;
                        return Ok(code);
                    }
                    CallbackDecision::Error(message) => {
                        respond(
                            &mut socket,
                            "200 OK",
                            "text/html; charset=utf-8",
                            FAILURE_HTML,
                        )
                        .await?;
                        return Err(SpendPanelError::AuthFailed("gemini".into(), message));
                    }
                    CallbackDecision::MismatchedState => {
                        respond(
                            &mut socket,
                            "400 Bad Request",
                            "text/plain; charset=utf-8",
                            "Ignoring mismatched OAuth callback state. Return to the Google sign-in flow.",
                        )
                        .await?;
                    }
                    CallbackDecision::Incomplete => {
                        respond(
                            &mut socket,
                            "400 Bad Request",
                            "text/plain; charset=utf-8",
                            "Ignoring incomplete OAuth callback. Return to the Google sign-in flow.",
                        )
                        .await?;
                    }
                }
            }
        };
        match tokio::time::timeout(timeout, inner).await {
            Ok(result) => result,
            Err(_) => Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                "timed out waiting for OAuth callback".into(),
            )),
        }
    }
}

enum CallbackDecision {
    Code(String),
    Error(String),
    MismatchedState,
    Incomplete,
}

/// Reads the raw request head (up to 8 KiB) until `\r\n\r\n`.
async fn read_request_head(stream: &mut tokio::net::TcpStream) -> Result<String, SpendPanelError> {
    let mut buf = [0u8; 1024];
    let mut head = Vec::with_capacity(1024);
    loop {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| SpendPanelError::NetworkError(format!("read callback request: {e}")))?;
        if n == 0 {
            return Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                "connection closed while reading callback request".into(),
            ));
        }
        head.extend_from_slice(&buf[..n]);
        if head.len() >= MAX_REQUEST_HEAD {
            return Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                "callback request too large".into(),
            ));
        }
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Classifies a raw request head against the expected state.
fn classify_callback(head: &str, expected_state: &str) -> CallbackDecision {
    let Some(target) = parse_request_target(head) else {
        return CallbackDecision::Incomplete;
    };
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
        return CallbackDecision::Incomplete;
    };
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if let Some(error) = params.get("error") {
        let message = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| error.clone());
        return CallbackDecision::Error(message);
    }
    match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) if state == expected_state => {
            CallbackDecision::Code(code.clone())
        }
        (Some(_), Some(_)) => CallbackDecision::MismatchedState,
        _ => CallbackDecision::Incomplete,
    }
}

/// Extracts the `GET <target> HTTP/1.1` request target.
fn parse_request_target(head: &str) -> Option<String> {
    let request_line = head.lines().next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next().map(str::to_string)
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), SpendPanelError> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|e| SpendPanelError::NetworkError(format!("write callback response: {e}")))?;
    Ok(())
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<title>Usage Monitor — Gemini connected</title>
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
         font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
         background: #f5f5f4; color: #1c1917; }
  main { width: min(420px, calc(100% - 3rem)); background: #ffffff; border-radius: 16px;
         padding: 2.5rem; box-shadow: 0 1px 3px rgba(0,0,0,.1), 0 8px 24px rgba(0,0,0,.08); }
  h1 { margin: 0 0 .75rem; font-size: 1.5rem; }
  p { margin: 0; color: #57534e; line-height: 1.6; }
  @media (prefers-color-scheme: dark) {
    body { background: #1c1917; color: #fafaf9; }
    main { background: #292524; }
    p { color: #d6d3d1; }
  }
</style>
</head>
<body>
<main>
  <h1>You're connected to Usage Monitor</h1>
  <p>Your Google account is now linked. You can close this window and continue in the terminal.</p>
</main>
</body>
</html>"#;

const FAILURE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<title>Usage Monitor — login not completed</title>
<style>
  body { font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; padding: 2rem; }
</style>
</head>
<body>
<h1>Login not completed</h1>
<p>You can close this window and try again.</p>
</body>
</html>"#;

fn is_headless() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|v| !v.is_empty()))
}

/// True when an exchange failure is Google's "malformed auth code" transient
/// error, in which case the flow should keep waiting for the next redirect.
fn should_ignore_malformed_auth_code(error: &SpendPanelError) -> bool {
    match error {
        SpendPanelError::AuthFailed(_, message) => {
            let lower = message.to_lowercase();
            lower.contains("invalid_grant") && lower.contains("malformed auth code")
        }
        _ => false,
    }
}

/// Parses pasted OAuth input: a full redirect URL, a query-string fragment, or
/// a bare authorization code. Returns `(code, state?)`.
pub(crate) fn parse_oauth_callback_input(input: &str) -> Option<(String, Option<String>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed).ok()?;
        let code = query_param(&url, "code")?;
        return Some((code, query_param(&url, "state")));
    }
    let candidate = trimmed.strip_prefix('?').unwrap_or(trimmed);
    if candidate.contains('=') {
        let params = parse_query_params(candidate);
        let code = params.get("code").cloned();
        let state = params.get("state").cloned();
        if code.is_some() || state.is_some() {
            return Some((code?, state));
        }
    }
    Some((trimmed.to_string(), None))
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn parse_query_params(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn read_pasted_input() -> Result<String, SpendPanelError> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| {
            SpendPanelError::AuthFailed(
                "gemini".into(),
                format!("reading pasted input failed: {e}"),
            )
        })?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Err(SpendPanelError::AuthFailed(
        "gemini".into(),
        "no authorization code provided".into(),
    ))
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_oauth(redirect_uri: &str) -> (GeminiOAuth, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let creds_path = dir.path().join("oauth_creds.json");
        let oauth = GeminiOAuth::with_endpoints(
            "http://localhost/token".into(),
            "http://localhost/userinfo".into(),
            redirect_uri.into(),
            creds_path,
        );
        (oauth, dir)
    }

    async fn send_callback(addr: std::net::SocketAddr, path_and_query: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let request = format!(
            "GET {path_and_query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let mut buf = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("response must arrive")
            .expect("read response");
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    async fn start_listener(redirect_uri: &str) -> (GeminiOAuth, CallbackListener) {
        let (oauth, _dir) = test_oauth(redirect_uri);
        let listener = oauth
            .start_callback_listener()
            .await
            .expect("listener binds on an ephemeral port");
        (oauth, listener)
    }

    #[test]
    fn test_generate_pkce_pair() {
        let (verifier, challenge) = GeminiOAuth::generate_pkce_pair();
        assert_eq!(verifier.len(), 43);
        assert_eq!(challenge.len(), 43);
        assert!(
            verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
        let (verifier2, _) = GeminiOAuth::generate_pkce_pair();
        assert_ne!(verifier, verifier2);
    }

    #[test]
    fn test_generate_state() {
        let a = GeminiOAuth::generate_state();
        let b = GeminiOAuth::generate_state();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn test_build_authorize_url() {
        let oauth = GeminiOAuth::prod();
        let (_, challenge) = GeminiOAuth::generate_pkce_pair();
        let state = "test-state-42";
        let url = Url::parse(&oauth.build_authorize_url(&challenge, state)).expect("valid url");
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], GEMINI_CLI_CLIENT_ID);
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["redirect_uri"], oauth.redirect_uri);
        assert_eq!(params["code_challenge"], challenge);
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["access_type"], "offline");
        assert_eq!(params["prompt"], "consent");
        assert_eq!(params["state"], state);
        let scope = &params["scope"];
        assert!(scope.contains("https://www.googleapis.com/auth/cloud-platform"));
        assert!(scope.contains("https://www.googleapis.com/auth/userinfo.email"));
        assert!(scope.contains("https://www.googleapis.com/auth/userinfo.profile"));
        assert_eq!(url.fragment(), Some("usage-monitor"));
    }

    #[test]
    fn test_resolve_creds_path() {
        let path = GeminiOAuth::resolve_creds_path(Some("/tmp/custom.json"));
        assert_eq!(path, PathBuf::from("/tmp/custom.json"));
        let home = std::env::var("HOME").unwrap_or_default();
        let expected = Path::new(&home).join(".gemini/oauth_creds.json");
        assert_eq!(GeminiOAuth::resolve_creds_path(None), expected);
        assert_eq!(GeminiOAuth::resolve_creds_path(Some("  ")), expected);
    }

    #[tokio::test]
    async fn test_callback_listener_valid_code() {
        let (_oauth, listener) = start_listener("http://127.0.0.1:0/oauth2callback").await;
        let addr = listener.local_addr();
        let wait = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(5),
                listener.wait_for_callback("state-abc", Duration::from_secs(5)),
            )
            .await
            .expect("must not hang")
        });
        let response = send_callback(addr, "/oauth2callback?code=code-123&state=state-abc").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let code = wait.await.expect("task joined").expect("callback ok");
        assert_eq!(code, "code-123");
    }

    #[tokio::test]
    async fn test_callback_listener_mismatched_state_then_valid() {
        let (_oauth, listener) = start_listener("http://127.0.0.1:0/oauth2callback").await;
        let addr = listener.local_addr();
        let wait = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(5),
                listener.wait_for_callback("state-abc", Duration::from_secs(5)),
            )
            .await
            .expect("must not hang")
        });
        let bad = send_callback(addr, "/oauth2callback?code=code-bad&state=state-wrong").await;
        assert!(bad.starts_with("HTTP/1.1 400 Bad Request"));
        let good = send_callback(addr, "/oauth2callback?code=code-good&state=state-abc").await;
        assert!(good.starts_with("HTTP/1.1 200 OK"));
        let code = wait.await.expect("task joined").expect("callback ok");
        assert_eq!(code, "code-good");
    }

    #[tokio::test]
    async fn test_callback_listener_error_param() {
        let (_oauth, listener) = start_listener("http://127.0.0.1:0/oauth2callback").await;
        let addr = listener.local_addr();
        let wait = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(5),
                listener.wait_for_callback("state-abc", Duration::from_secs(5)),
            )
            .await
            .expect("must not hang")
        });
        let response = send_callback(
            addr,
            "/oauth2callback?error=access_denied&error_description=User%20denied",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let err = wait
            .await
            .expect("task joined")
            .expect_err("error callback must fail");
        assert!(matches!(err, SpendPanelError::AuthFailed(_, m) if m.contains("User denied")));
    }

    #[tokio::test]
    async fn test_callback_listener_timeout() {
        let (_oauth, listener) = start_listener("http://127.0.0.1:0/oauth2callback").await;
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            listener.wait_for_callback("state-abc", Duration::from_millis(200)),
        )
        .await
        .expect("must not hang");
        let err = result.expect_err("no callback must time out");
        assert!(matches!(err, SpendPanelError::AuthFailed(_, m) if m.contains("timed out")));
    }

    #[tokio::test]
    async fn test_exchange_code_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"at-1","expires_in":3599,"refresh_token":"rt-1"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let (oauth, _dir) = test_oauth("http://127.0.0.1:0/oauth2callback");
        let oauth = GeminiOAuth {
            token_url: format!("{}/token", server.uri()),
            ..oauth
        };
        let client = reqwest::Client::new();
        let token = oauth
            .exchange_code(&client, "code-x", "verifier-x")
            .await
            .expect("exchange succeeds");
        assert_eq!(token.access_token, "at-1");
        assert_eq!(token.refresh_token, "rt-1");
        assert_eq!(token.expires_in, 3599);
    }

    #[tokio::test]
    async fn test_exchange_code_missing_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"at-1","expires_in":3599}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let (oauth, _dir) = test_oauth("http://127.0.0.1:0/oauth2callback");
        let oauth = GeminiOAuth {
            token_url: format!("{}/token", server.uri()),
            ..oauth
        };
        let client = reqwest::Client::new();
        let err = oauth
            .exchange_code(&client, "code-x", "verifier-x")
            .await
            .expect_err("missing refresh token must fail");
        assert!(
            matches!(err, SpendPanelError::AuthFailed(_, m) if m.contains("missing refresh token"))
        );
    }

    #[tokio::test]
    async fn test_exchange_code_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_raw(
                r#"{"error":"invalid_grant","error_description":"Malformed auth code."}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let (oauth, _dir) = test_oauth("http://127.0.0.1:0/oauth2callback");
        let oauth = GeminiOAuth {
            token_url: format!("{}/token", server.uri()),
            ..oauth
        };
        let client = reqwest::Client::new();
        let err = oauth
            .exchange_code(&client, "code-x", "verifier-x")
            .await
            .expect_err("HTTP 400 must fail");
        assert!(matches!(
            &err,
            SpendPanelError::AuthFailed(_, m) if m.contains("HTTP 400") && m.contains("invalid_grant")
        ));
        assert!(should_ignore_malformed_auth_code(&err));
    }

    #[test]
    fn test_should_ignore_malformed_auth_code() {
        let malformed = SpendPanelError::AuthFailed(
            "gemini".into(),
            "token exchange failed (HTTP 400): {\"error\":\"invalid_grant\",\"error_description\":\"Malformed auth code.\"}".into(),
        );
        assert!(should_ignore_malformed_auth_code(&malformed));
        let other = SpendPanelError::AuthFailed(
            "gemini".into(),
            "token exchange failed (HTTP 400): invalid_client".into(),
        );
        assert!(!should_ignore_malformed_auth_code(&other));
        let network = SpendPanelError::NetworkError("boom".into());
        assert!(!should_ignore_malformed_auth_code(&network));
    }

    #[test]
    fn test_parse_oauth_callback_input() {
        assert_eq!(
            parse_oauth_callback_input(
                "http://localhost:8085/oauth2callback?code=abc123&state=xyz"
            ),
            Some(("abc123".into(), Some("xyz".into())))
        );
        assert_eq!(
            parse_oauth_callback_input("?code=abc&state=xyz"),
            Some(("abc".into(), Some("xyz".into())))
        );
        assert_eq!(
            parse_oauth_callback_input("state=xyz&code=abc"),
            Some(("abc".into(), Some("xyz".into())))
        );
        assert_eq!(
            parse_oauth_callback_input("abc123"),
            Some(("abc123".into(), None))
        );
        assert_eq!(parse_oauth_callback_input("   "), None);
        // Full URL without a code → nothing usable.
        assert_eq!(
            parse_oauth_callback_input("http://localhost:8085/oauth2callback"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_write_creds_mode_0600() {
        let dir = tempfile::tempdir().expect("tempdir");
        let creds_path = dir.path().join("nested/oauth_creds.json");
        let (oauth, _) = test_oauth("http://127.0.0.1:0/oauth2callback");
        let oauth = GeminiOAuth {
            creds_path: creds_path.clone(),
            ..oauth
        };
        let token = TokenResult {
            access_token: "at-1".into(),
            refresh_token: "rt-1".into(),
            expires_in: 3600,
        };
        let written = oauth.write_creds(&token).expect("creds written");
        assert_eq!(written, creds_path);
        use std::os::unix::fs::MetadataExt;
        let mode = std::fs::metadata(&creds_path).expect("metadata").mode() & 0o777;
        assert_eq!(mode, 0o600);
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&creds_path).unwrap()).unwrap();
        assert_eq!(json["access_token"], "at-1");
        assert_eq!(json["refresh_token"], "rt-1");
        assert!(json["expiry_date"].as_f64().unwrap() > now_ms());
    }

    #[tokio::test]
    async fn test_full_login_flow_manual() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"at-flow","expires_in":3599,"refresh_token":"rt-flow"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"email":"user@example.com"}"#, "application/json"),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let creds_path = dir.path().join("oauth_creds.json");
        let oauth = GeminiOAuth::with_endpoints(
            format!("{}/token", server.uri()),
            format!("{}/userinfo", server.uri()),
            "http://127.0.0.1:0/oauth2callback".into(),
            creds_path.clone(),
        );
        let (verifier, challenge) = GeminiOAuth::generate_pkce_pair();
        let state = "flow-state-1";

        let listener = oauth.start_callback_listener().await.expect("listener");
        let addr = listener.local_addr();
        let wait = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(10),
                listener.wait_for_callback(state, Duration::from_secs(10)),
            )
            .await
            .expect("must not hang")
        });
        // A mismatched-state callback must be skipped before the valid one.
        send_callback(addr, "/oauth2callback?code=cb-wrong&state=wrong").await;
        let response = send_callback(addr, "/oauth2callback?code=cb-1&state=flow-state-1").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let code = wait.await.expect("task joined").expect("callback ok");
        assert_eq!(code, "cb-1");

        let client = reqwest::Client::new();
        let token = oauth
            .exchange_code(&client, &code, &verifier)
            .await
            .expect("exchange");
        let email = oauth.fetch_email(&client, &token.access_token).await;
        assert_eq!(email.as_deref(), Some("user@example.com"));
        let path = oauth.write_creds(&token).expect("write");
        assert_eq!(path, creds_path);

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(!json["access_token"].as_str().unwrap().is_empty());
        assert!(!json["refresh_token"].as_str().unwrap().is_empty());
        let expiry = json["expiry_date"].as_f64().expect("numeric expiry");
        let now = now_ms();
        assert!(expiry > now && expiry < now + 4_000_000.0);
        // The challenge used in the URL must be the PKCE challenge, not the verifier.
        let authorize_url = oauth.build_authorize_url(&challenge, state);
        let parsed = Url::parse(&authorize_url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["code_challenge"], challenge);
    }

    #[tokio::test]
    async fn test_login_end_to_end() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"at-login","expires_in":3599,"refresh_token":"rt-login"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"email":"login@example.com"}"#, "application/json"),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let creds_path = dir.path().join("oauth_creds.json");
        let oauth = GeminiOAuth::with_endpoints(
            format!("{}/token", server.uri()),
            format!("{}/userinfo", server.uri()),
            "http://127.0.0.1:18446/oauth2callback".into(),
            creds_path.clone(),
        );
        let injection = LoginInjection {
            state: "inj-state".into(),
            verifier: "inj-verifier".into(),
            challenge: "inj-challenge".into(),
        };
        let login = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(15), oauth.login(false, Some(injection)))
                .await
                .expect("login must not hang")
        });

        // Retry-connect until the listener is up, then deliver the callback.
        let addr: std::net::SocketAddr = "127.0.0.1:18446".parse().expect("addr");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(mut stream) => {
                    let request = "GET /oauth2callback?code=cb-2&state=inj-state HTTP/1.1\r\n\
                                   Host: 127.0.0.1\r\nConnection: close\r\n\r\n";
                    stream
                        .write_all(request.as_bytes())
                        .await
                        .expect("write callback");
                    break;
                }
                Err(_) if std::time::Instant::now() > deadline => {
                    panic!("login listener never came up on {addr}");
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }

        let outcome = login.await.expect("task joined").expect("login ok");
        assert_eq!(outcome.email.as_deref(), Some("login@example.com"));
        assert_eq!(outcome.creds_path, creds_path);
        assert!(outcome.creds_path.exists());
        assert!(outcome.expires_ms > now_ms());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&outcome.creds_path).unwrap()).unwrap();
        assert_eq!(json["access_token"], "at-login");
        assert_eq!(json["refresh_token"], "rt-login");
    }
}
