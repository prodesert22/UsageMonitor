//! Provider for OpenCode Go via the opencode.ai web dashboard.
//!
//! Manual setup: there is no public usage API, so this provider authenticates
//! with a browser session Cookie header configured by the user and scrapes the
//! workspace dashboard hydration payload. One cookie can cover multiple
//! workspaces. See `docs/providers/opencode-go.md` for the full extraction spec.

use async_trait::async_trait;
use chrono::Utc;

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

const DEFAULT_BASE: &str = "https://opencode.ai";
/// Build-specific hash of the SolidStart server function that lists
/// workspaces. Changes when opencode.ai redeploys; users can bypass discovery
/// by configuring `workspaces` explicitly.
const WORKSPACES_SERVER_ID: &str =
    "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

/// One parsed usage window from the dashboard payload.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParsedWindow {
    /// 0–100.
    percent: f64,
    reset_in_sec: i64,
}

/// A workspace reference: id plus an optional human-readable name.
///
/// Names come from the discovery payload (fetched automatically) or from a
/// manual `wrk_id=Name` config entry, which takes precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRef {
    pub id: String,
    pub name: Option<String>,
}

impl WorkspaceRef {
    /// Name when known, id otherwise.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Serializes back to a config entry (`wrk_id` or `wrk_id=Name`).
    pub fn to_entry(&self) -> String {
        match &self.name {
            Some(name) => format!("{}={}", self.id, name),
            None => self.id.clone(),
        }
    }
}

/// Usage of a single workspace.
#[derive(Debug, Clone, PartialEq)]
struct WorkspaceUsage {
    workspace: WorkspaceRef,
    rolling: ParsedWindow,
    weekly: ParsedWindow,
    monthly: Option<ParsedWindow>,
}

pub struct OpenCodeGoProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl OpenCodeGoProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "opencode-go",
                name: "OpenCode Go",
                description: "OpenCode Go workspace usage via opencode.ai dashboard (manual cookie)",
                auth_methods: &["cookie"],
                website: Some("https://opencode.ai"),
            },
            base_url: None,
        }
    }

    /// Creates a provider with a custom base URL (for tests).
    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn api_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or(DEFAULT_BASE)
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Session cookie from the `token` config field (`cookie` kept as alias).
    fn resolve_cookie(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        let raw = ctx.config.get("token").or_else(|| ctx.config.get("cookie"));
        match raw.map(|c| c.trim()) {
            Some(cookie) if !cookie.is_empty() => Ok(normalize_cookie_header(cookie)),
            _ => Err(SpendPanelError::AuthFailed(
                "opencode-go".into(),
                "no session token configured; run `usage-monitor opencode-go set token \"<Cookie header or auth value>\"` (see docs/providers/opencode-go.md)".into(),
            )),
        }
    }

    /// Workspace refs from config (`workspaces = "wrk_a=Name,wrk_b"`), if set.
    fn configured_workspaces(ctx: &ProviderContext) -> Option<Vec<WorkspaceRef>> {
        let raw = ctx.config.get("workspaces")?;
        let refs: Vec<WorkspaceRef> = raw.split(',').filter_map(parse_workspace_entry).collect();
        if refs.is_empty() { None } else { Some(refs) }
    }

    /// A 200 response can still be a login page; detect signed-out payloads.
    fn looks_signed_out(text: &str) -> bool {
        let lower = text.to_lowercase();
        lower.contains("login")
            || lower.contains("sign in")
            || lower.contains("auth/authorize")
            || lower.contains("not associated with an account")
            || lower.contains("actor of type \"public\"")
    }

    /// Discovers workspaces (id + name) via the internal server function.
    async fn discover_workspaces(
        base: &str,
        client: &reqwest::Client,
        cookie: &str,
    ) -> Result<Vec<WorkspaceRef>, SpendPanelError> {
        let url = format!("{}/_server?id={}", base, WORKSPACES_SERVER_ID);
        let resp = client
            .get(&url)
            .header("cookie", cookie)
            .header("x-server-id", WORKSPACES_SERVER_ID)
            .header(
                "x-server-instance",
                format!("server-fn:{:x}", std::process::id()),
            )
            .header("origin", base.to_string())
            .header("referer", format!("{}/", base))
            .header(
                "accept",
                "text/javascript, application/json;q=0.9, */*;q=0.8",
            )
            .header("user-agent", USER_AGENT)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if status == 401 || status == 403 || Self::looks_signed_out(&body) {
            return Err(SpendPanelError::AuthFailed(
                "opencode-go".into(),
                "session cookie rejected or expired; copy a fresh Cookie header".into(),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "opencode-go".into(),
                format!("workspace discovery HTTP {}", status),
            ));
        }

        let refs = parse_discovered_workspaces(&body);
        if refs.is_empty() {
            return Err(SpendPanelError::ParseError(
                "opencode-go".into(),
                "no workspace ids in discovery payload; configure `workspaces` manually".into(),
            ));
        }
        Ok(refs)
    }

    /// Fetches and parses the usage of one workspace dashboard page.
    async fn fetch_workspace_usage(
        base: &str,
        client: &reqwest::Client,
        cookie: &str,
        workspace: &WorkspaceRef,
    ) -> Result<WorkspaceUsage, SpendPanelError> {
        let workspace_id = &workspace.id;
        let url = format!("{}/workspace/{}/go", base, workspace_id);
        let resp = client
            .get(&url)
            .header("cookie", cookie)
            .header("user-agent", USER_AGENT)
            .header(
                "accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if status == 401 || status == 403 || Self::looks_signed_out(&body) {
            return Err(SpendPanelError::AuthFailed(
                "opencode-go".into(),
                "session cookie rejected or expired; copy a fresh Cookie header".into(),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "opencode-go".into(),
                format!("workspace {} HTTP {}", workspace_id, status),
            ));
        }

        parse_workspace_page(workspace, &body)
    }

    fn rate_window(label: String, window_minutes: u32, w: &ParsedWindow) -> RateWindow {
        let ratio = (w.percent / 100.0).clamp(0.0, 1.0);
        RateWindow {
            label,
            window_minutes,
            usage_ratio: ratio,
            limit: None,
            used: None,
            remaining: None,
            resets_at: Some(Utc::now() + chrono::Duration::seconds(w.reset_in_sec.max(0))),
            status: RateWindowStatus::from_ratio(ratio),
        }
    }

    fn snapshot_from_usages(usages: &[WorkspaceUsage]) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("opencode-go");
        snapshot.collected_at = Utc::now();

        let Some(first) = usages.first() else {
            return snapshot;
        };

        let first_name = first.workspace.display_name();

        snapshot.primary_rate_window = Some(Self::rate_window(
            format!("{} Rolling (5h)", first_name),
            300,
            &first.rolling,
        ));
        snapshot.secondary_rate_window = Some(Self::rate_window(
            format!("{} Weekly", first_name),
            10_080,
            &first.weekly,
        ));
        if let Some(monthly) = &first.monthly {
            snapshot.tertiary_rate_window = Some(Self::rate_window(
                format!("{} Monthly", first_name),
                43_200,
                monthly,
            ));
        }

        // Additional workspaces (same cookie) become named extra windows.
        for usage in &usages[1..] {
            let ws = &usage.workspace;
            let name = ws.display_name();
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: format!("{}-rolling", ws.id),
                label: format!("{} Rolling (5h)", name),
                window: Self::rate_window(format!("{} Rolling (5h)", name), 300, &usage.rolling),
            });
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: format!("{}-weekly", ws.id),
                label: format!("{} Weekly", name),
                window: Self::rate_window(format!("{} Weekly", name), 10_080, &usage.weekly),
            });
            if let Some(monthly) = &usage.monthly {
                snapshot.extra_rate_windows.push(NamedRateWindow {
                    id: format!("{}-monthly", ws.id),
                    label: format!("{} Monthly", name),
                    window: Self::rate_window(format!("{} Monthly", name), 43_200, monthly),
                });
            }
        }

        snapshot
    }
}

/// Normalizes user-provided auth into a valid Cookie header value.
///
/// Accepted inputs:
/// - Full Cookie header value: `auth=Fe26...; other=value`
/// - Header line copied with name: `Cookie: auth=Fe26...`
/// - Bare opencode auth cookie value: `Fe26...` → `auth=Fe26...`
fn normalize_cookie_header(raw: &str) -> String {
    let value = raw.trim();
    let value = value
        .strip_prefix("Cookie:")
        .or_else(|| value.strip_prefix("cookie:"))
        .map(str::trim)
        .unwrap_or(value);

    if looks_like_cookie_header(value) {
        value.to_string()
    } else {
        format!("auth={}", value)
    }
}

fn looks_like_cookie_header(value: &str) -> bool {
    let first_pair = value.split(';').next().unwrap_or(value).trim();
    let Some((name, cookie_value)) = first_pair.split_once('=') else {
        return false;
    };
    !name.trim().is_empty()
        && !cookie_value.trim().is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Normalizes a workspace reference: bare `wrk_...` id, a dashboard URL
/// containing `/workspace/<id>/`, or any string embedding a `wrk_` id.
pub fn normalize_workspace_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = trimmed.find("wrk_")?;
    let id: String = trimmed[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if id.len() > 4 { Some(id) } else { None }
}

/// Parses a config entry: `wrk_id`, `wrk_id=Name`, or a dashboard URL
/// (optionally with `=Name`).
pub fn parse_workspace_entry(raw: &str) -> Option<WorkspaceRef> {
    let (id_part, name) = match raw.split_once('=') {
        Some((id, name)) if !name.trim().is_empty() => (id, Some(name.trim().to_string())),
        Some((id, _)) => (id, None),
        None => (raw, None),
    };
    let id = normalize_workspace_id(id_part)?;
    Some(WorkspaceRef { id, name })
}

/// Adds a workspace (id or dashboard URL, with an optional name) to a list of
/// config entries. Errors when the reference has no `wrk_` id. Adding an
/// existing id updates its name; otherwise duplicates are a no-op.
pub fn add_workspace(
    list: &[String],
    raw: &str,
    name: Option<&str>,
) -> Result<Vec<String>, SpendPanelError> {
    let mut new_ref = parse_workspace_entry(raw).ok_or_else(|| {
        SpendPanelError::ConfigError(format!(
            "'{}' has no workspace id; expected wrk_... or a dashboard URL like https://opencode.ai/workspace/wrk_xxx/go",
            raw
        ))
    })?;
    if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
        new_ref.name = Some(name.to_string());
    }

    let mut refs: Vec<WorkspaceRef> = list
        .iter()
        .filter_map(|e| parse_workspace_entry(e))
        .collect();
    match refs.iter_mut().find(|r| r.id == new_ref.id) {
        Some(existing) => {
            if new_ref.name.is_some() {
                existing.name = new_ref.name;
            }
        }
        None => refs.push(new_ref),
    }
    Ok(refs.iter().map(WorkspaceRef::to_entry).collect())
}

/// Removes a workspace (matched by id) from a list of config entries.
pub fn remove_workspace(list: &[String], raw: &str) -> Result<Vec<String>, SpendPanelError> {
    let id = normalize_workspace_id(raw).ok_or_else(|| {
        SpendPanelError::ConfigError(format!("'{}' has no workspace id (expected wrk_...)", raw))
    })?;
    Ok(list
        .iter()
        .filter(|e| parse_workspace_entry(e).is_none_or(|r| r.id != id))
        .cloned()
        .collect())
}

/// Extracts workspaces (`wrk_...` id plus the `name:"..."` that follows it in
/// the same object, when present) from a discovery payload.
fn parse_discovered_workspaces(text: &str) -> Vec<WorkspaceRef> {
    let mut refs: Vec<WorkspaceRef> = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("wrk_") {
        let candidate: String = rest[pos..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = &rest[pos + candidate.len().max(4)..];
        if candidate.len() > 4 && !refs.iter().any(|r| r.id == candidate) {
            // The name sits in the same object, e.g. {id:"wrk_x",name:"Default"}.
            let segment_end = after.find('}').unwrap_or(after.len());
            let name = extract_string(&after[..segment_end], "name");
            refs.push(WorkspaceRef {
                id: candidate,
                name,
            });
        }
        rest = after;
    }
    refs
}

/// Finds `key:"value"` (or `"key":"value"`) inside a segment.
fn extract_string(segment: &str, key: &str) -> Option<String> {
    let pos = segment.find(key)?;
    let after = segment[pos + key.len()..]
        .trim_start_matches('"')
        .trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    let value = &after[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Extracts `<window>...usagePercent: N` / `resetInSec: N` pairs from the
/// dashboard hydration payload.
fn parse_window(text: &str, window_key: &str) -> Option<ParsedWindow> {
    for (start, _) in text.match_indices(window_key) {
        // Window object ends at the first closing brace after the key. Some
        // payloads also contain scalar billing fields such as
        // `monthlyUsage:null` before the real workspace usage object; skip
        // segments without `usagePercent` and keep searching.
        let segment_end = text[start..]
            .find('}')
            .map(|i| start + i)
            .unwrap_or(text.len());
        let segment = &text[start..segment_end];

        let Some(percent) = extract_number(segment, "usagePercent") else {
            continue;
        };
        let reset_in_sec = extract_number(segment, "resetInSec").unwrap_or(0.0) as i64;

        // Some payload variants emit ratios instead of percentages.
        let percent = if (0.0..=1.0).contains(&percent) {
            percent * 100.0
        } else {
            percent
        };
        return Some(ParsedWindow {
            percent: percent.clamp(0.0, 100.0),
            reset_in_sec,
        });
    }

    None
}

/// Finds `key: <number>` (with optional quotes around the number) inside a segment.
fn extract_number(segment: &str, key: &str) -> Option<f64> {
    let pos = segment.find(key)?;
    let after = &segment[pos + key.len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"').unwrap_or(after);
    let number: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok()
}

fn parse_workspace_page(
    workspace: &WorkspaceRef,
    text: &str,
) -> Result<WorkspaceUsage, SpendPanelError> {
    let rolling = parse_window(text, "rollingUsage");
    let weekly = parse_window(text, "weeklyUsage");
    match (rolling, weekly) {
        (Some(rolling), Some(weekly)) => Ok(WorkspaceUsage {
            workspace: workspace.clone(),
            rolling,
            weekly,
            monthly: parse_window(text, "monthlyUsage"),
        }),
        _ => Err(SpendPanelError::ParseError(
            "opencode-go".into(),
            format!("workspace {} page is missing usage fields", workspace.id),
        )),
    }
}

impl Default for OpenCodeGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for OpenCodeGoProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    // Manual provider: nothing detectable on disk, stays disabled until the
    // user configures a cookie and enables it.
    fn detect_credentials(&self) -> bool {
        false
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let base = self.api_base();

        let workspaces = match Self::configured_workspaces(ctx) {
            Some(mut refs) => {
                // Pinned ids may lack names; enrich them from discovery on a
                // best-effort basis (manual names always win).
                if refs.iter().any(|r| r.name.is_none())
                    && let Ok(discovered) = Self::discover_workspaces(base, &client, &cookie).await
                {
                    for r in refs.iter_mut().filter(|r| r.name.is_none()) {
                        r.name = discovered
                            .iter()
                            .find(|d| d.id == r.id)
                            .and_then(|d| d.name.clone());
                    }
                }
                refs
            }
            None => Self::discover_workspaces(base, &client, &cookie).await?,
        };

        let mut usages = Vec::new();
        let mut first_error: Option<SpendPanelError> = None;
        for ws in &workspaces {
            match Self::fetch_workspace_usage(base, &client, &cookie, ws).await {
                Ok(usage) => usages.push(usage),
                Err(e) => {
                    tracing::warn!("opencode-go workspace {} failed: {}", ws.id, e);
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }

        if usages.is_empty() {
            return Err(first_error.unwrap_or_else(|| {
                SpendPanelError::ProviderError("opencode-go".into(), "no workspaces fetched".into())
            }));
        }

        Ok(Self::snapshot_from_usages(&usages))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn dashboard_page(rolling_pct: f64, weekly_pct: f64, monthly: Option<f64>) -> String {
        let monthly_part = monthly
            .map(|m| format!(r#"monthlyUsage:{{usagePercent:{},resetInSec:864000}},"#, m))
            .unwrap_or_default();
        format!(
            r#"<html><body><script>self.__data={{billing:{{rollingUsage:{{usagePercent:{},resetInSec:3600}},weeklyUsage:{{usagePercent:{},resetInSec:172800}},{}plan:"go"}}}};</script></body></html>"#,
            rolling_pct, weekly_pct, monthly_part
        )
    }

    #[test]
    fn test_normalize_workspace_id() {
        assert_eq!(
            normalize_workspace_id("wrk_abc123"),
            Some("wrk_abc123".into())
        );
        assert_eq!(
            normalize_workspace_id("  wrk_abc123  "),
            Some("wrk_abc123".into())
        );
        assert_eq!(
            normalize_workspace_id("https://opencode.ai/workspace/wrk_abc123/go"),
            Some("wrk_abc123".into())
        );
        assert_eq!(normalize_workspace_id("wrk_"), None);
        assert_eq!(normalize_workspace_id("nope"), None);
        assert_eq!(normalize_workspace_id(""), None);
    }

    #[test]
    fn test_add_workspace() {
        let v = add_workspace(&[], "wrk_a", None).unwrap();
        assert_eq!(v, vec!["wrk_a"]);
        let v = add_workspace(&v, "https://opencode.ai/workspace/wrk_b/go", None).unwrap();
        assert_eq!(v, vec!["wrk_a", "wrk_b"]);
        // Duplicate is a no-op.
        let v = add_workspace(&v, "wrk_a", None).unwrap();
        assert_eq!(v, vec!["wrk_a", "wrk_b"]);
        assert!(add_workspace(&v, "not-a-workspace", None).is_err());
    }

    #[test]
    fn test_add_workspace_with_name() {
        let v = add_workspace(&[], "wrk_a", Some("Production")).unwrap();
        assert_eq!(v, vec!["wrk_a=Production"]);
        // Re-adding with a new name updates it.
        let v = add_workspace(&v, "wrk_a", Some("Staging")).unwrap();
        assert_eq!(v, vec!["wrk_a=Staging"]);
        // Re-adding without a name keeps the existing one.
        let v = add_workspace(&v, "wrk_a", None).unwrap();
        assert_eq!(v, vec!["wrk_a=Staging"]);
    }

    #[test]
    fn test_parse_workspace_entry() {
        assert_eq!(
            parse_workspace_entry("wrk_a=Prod"),
            Some(WorkspaceRef {
                id: "wrk_a".into(),
                name: Some("Prod".into())
            })
        );
        assert_eq!(
            parse_workspace_entry("wrk_a"),
            Some(WorkspaceRef {
                id: "wrk_a".into(),
                name: None
            })
        );
        assert_eq!(
            parse_workspace_entry("https://opencode.ai/workspace/wrk_a/go=My Team"),
            Some(WorkspaceRef {
                id: "wrk_a".into(),
                name: Some("My Team".into())
            })
        );
        assert_eq!(parse_workspace_entry("garbage"), None);
    }

    #[test]
    fn test_remove_workspace() {
        let list = vec!["wrk_a".to_string(), "wrk_b".to_string()];
        assert_eq!(remove_workspace(&list, "wrk_a").unwrap(), vec!["wrk_b"]);
        assert!(remove_workspace(&list[..1], "wrk_a").unwrap().is_empty());
        assert_eq!(remove_workspace(&list, "wrk_other").unwrap(), list);
        // Invalid reference is an error, not a silent wipe.
        assert!(remove_workspace(&list, "garbage").is_err());
    }

    #[test]
    fn test_parse_discovered_workspaces() {
        let payload = r#"{"workspaces":[{"id":"wrk_aaa1","name":"Production"},{"id":"wrk_bbb2"},{"id":"wrk_aaa1"}]}"#;
        let refs = parse_discovered_workspaces(payload);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].id, "wrk_aaa1");
        assert_eq!(refs[0].name.as_deref(), Some("Production"));
        assert_eq!(refs[1].id, "wrk_bbb2");
        assert_eq!(refs[1].name, None);
        assert!(parse_discovered_workspaces("no ids here").is_empty());
    }

    #[test]
    fn test_parse_discovered_workspaces_hydration_payload() {
        // Unquoted-key hydration format used by the dashboard's JS payload.
        let payload =
            r#"($R=>$R[0]=[$R[1]={id:"wrk_01K6AR1ZET89H8NB691FQ2C2VB",name:"Default",slug:null}])"#;
        let refs = parse_discovered_workspaces(payload);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].id, "wrk_01K6AR1ZET89H8NB691FQ2C2VB");
        assert_eq!(refs[0].name.as_deref(), Some("Default"));
    }

    #[test]
    fn test_parse_window_percent_and_reset() {
        let page = dashboard_page(42.5, 80.0, Some(12.0));
        let rolling = parse_window(&page, "rollingUsage").unwrap();
        assert_eq!(rolling.percent, 42.5);
        assert_eq!(rolling.reset_in_sec, 3600);

        let weekly = parse_window(&page, "weeklyUsage").unwrap();
        assert_eq!(weekly.percent, 80.0);

        let monthly = parse_window(&page, "monthlyUsage").unwrap();
        assert_eq!(monthly.percent, 12.0);
    }

    #[test]
    fn test_parse_window_skips_scalar_billing_usage() {
        let page = r#"
            billing:{monthlyUsage:null,timeMonthlyUsageUpdated:null}
            workspace:{monthlyUsage:{status:"ok",resetInSec:72652,usagePercent:99}}
        "#;

        let monthly = parse_window(page, "monthlyUsage").unwrap();
        assert_eq!(monthly.percent, 99.0);
        assert_eq!(monthly.reset_in_sec, 72652);
    }

    #[test]
    fn test_parse_window_ratio_heuristic() {
        // Values <= 1.0 are ratios and get scaled to percent.
        let page = "rollingUsage:{usagePercent:0.42,resetInSec:60}";
        let w = parse_window(page, "rollingUsage").unwrap();
        assert!((w.percent - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_window_clamps_over_100() {
        let page = "rollingUsage:{usagePercent:140,resetInSec:60}";
        let w = parse_window(page, "rollingUsage").unwrap();
        assert_eq!(w.percent, 100.0);
    }

    #[test]
    fn test_parse_workspace_page_missing_fields() {
        let ws = WorkspaceRef {
            id: "wrk_x".into(),
            name: None,
        };
        let result = parse_workspace_page(&ws, "<html>nothing here</html>");
        assert!(matches!(result, Err(SpendPanelError::ParseError(_, _))));
    }

    #[test]
    fn test_looks_signed_out() {
        assert!(OpenCodeGoProvider::looks_signed_out(
            "<a href=\"/auth/authorize\">Sign in</a>"
        ));
        assert!(OpenCodeGoProvider::looks_signed_out(
            r#"actor of type "public""#
        ));
        assert!(!OpenCodeGoProvider::looks_signed_out(
            "rollingUsage:{usagePercent:1}"
        ));
    }

    #[test]
    fn test_resolve_cookie_missing() {
        let ctx = ProviderContext::new();
        assert!(matches!(
            OpenCodeGoProvider::resolve_cookie(&ctx),
            Err(SpendPanelError::AuthFailed(_, _))
        ));
    }

    #[test]
    fn test_resolve_cookie_token_field_preferred() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "session=token".into());
        ctx.config.insert("cookie".into(), "session=alias".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_cookie(&ctx).unwrap(),
            "session=token"
        );

        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=alias".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_cookie(&ctx).unwrap(),
            "session=alias"
        );
    }

    #[test]
    fn test_normalize_cookie_header_accepts_full_cookie_header() {
        assert_eq!(
            normalize_cookie_header("auth=Fe26.2**abc; other=value"),
            "auth=Fe26.2**abc; other=value"
        );
    }

    #[test]
    fn test_normalize_cookie_header_strips_cookie_prefix() {
        assert_eq!(
            normalize_cookie_header("Cookie: auth=Fe26.2**abc; other=value"),
            "auth=Fe26.2**abc; other=value"
        );
        assert_eq!(
            normalize_cookie_header("cookie: auth=Fe26.2**abc"),
            "auth=Fe26.2**abc"
        );
    }

    #[test]
    fn test_normalize_cookie_header_prefixes_bare_auth_value() {
        assert_eq!(
            normalize_cookie_header("Fe26.2**abc"),
            "auth=Fe26.2**abc"
        );
    }

    #[test]
    fn test_provider_metadata() {
        let p = OpenCodeGoProvider::new();
        assert_eq!(p.metadata().id, "opencode-go");
        assert!(!p.detect_credentials());
    }

    #[tokio::test]
    async fn test_fetch_with_configured_workspaces() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/workspace/wrk_one/go"))
            .and(header("cookie", "session=abc"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(dashboard_page(10.0, 50.0, None)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_two/go"))
            .respond_with(ResponseTemplate::new(200).set_body_string(dashboard_page(
                95.0,
                99.0,
                Some(40.0),
            )))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());
        ctx.config
            .insert("workspaces".into(), "wrk_one, wrk_two".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.provider_id, "opencode-go");

        let primary = snap.primary_rate_window.unwrap();
        assert!((primary.usage_ratio - 0.10).abs() < 1e-9);
        assert_eq!(primary.window_minutes, 300);
        assert!(snap.tertiary_rate_window.is_none()); // first ws has no monthly

        // Second workspace → extra windows (rolling, weekly, monthly).
        assert_eq!(snap.extra_rate_windows.len(), 3);
        assert_eq!(snap.extra_rate_windows[0].id, "wrk_two-rolling");
        assert_eq!(snap.extra_rate_windows[2].id, "wrk_two-monthly");
        assert_eq!(snap.extra_rate_windows[2].label, "wrk_two Monthly");
        assert_eq!(
            snap.extra_rate_windows[1].window.status,
            RateWindowStatus::Critical
        );
    }

    #[tokio::test]
    async fn test_fetch_discovers_workspaces() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/_server"))
            .and(header("x-server-id", WORKSPACES_SERVER_ID))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"[{"id":"wrk_disc","slug":"main"}]"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_disc/go"))
            .respond_with(ResponseTemplate::new(200).set_body_string(dashboard_page(
                30.0,
                60.0,
                Some(5.0),
            )))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!((snap.primary_rate_window.unwrap().usage_ratio - 0.30).abs() < 1e-9);
        assert!(snap.tertiary_rate_window.is_some());
        assert!(snap.extra_rate_windows.is_empty());
    }

    #[tokio::test]
    async fn test_discovered_names_label_windows() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/_server"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{"id":"wrk_one","name":"Production"},{"id":"wrk_two","name":"Staging"}]"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_one/go"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(dashboard_page(10.0, 50.0, None)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_two/go"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(dashboard_page(20.0, 60.0, None)),
            )
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(
            snap.primary_rate_window.unwrap().label,
            "Production Rolling (5h)"
        );
        assert_eq!(snap.extra_rate_windows[0].label, "Staging Rolling (5h)");
    }

    #[tokio::test]
    async fn test_pinned_ids_enriched_with_discovered_names() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/_server"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{"id":"wrk_one","name":"Production"},{"id":"wrk_two","name":"Staging"}]"#,
            ))
            .mount(&server)
            .await;
        for ws in ["wrk_one", "wrk_two"] {
            Mock::given(method("GET"))
                .and(path(format!("/workspace/{}/go", ws)))
                .respond_with(
                    ResponseTemplate::new(200).set_body_string(dashboard_page(10.0, 50.0, None)),
                )
                .mount(&server)
                .await;
        }

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());
        // wrk_one pinned without a name (enriched from discovery);
        // wrk_two has a manual name, which wins over the discovered one.
        ctx.config
            .insert("workspaces".into(), "wrk_one,wrk_two=Manual".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(
            snap.primary_rate_window.unwrap().label,
            "Production Rolling (5h)"
        );
        assert_eq!(snap.extra_rate_windows[0].label, "Manual Rolling (5h)");
    }

    #[tokio::test]
    async fn test_pinned_ids_work_when_discovery_fails() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/_server"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_one/go"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(dashboard_page(10.0, 50.0, None)),
            )
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());
        ctx.config.insert("workspaces".into(), "wrk_one".into());

        // Name enrichment is best-effort; a broken discovery endpoint must
        // not break pinned workspaces.
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().label, "wrk_one Rolling (5h)");
    }

    #[tokio::test]
    async fn test_signed_out_page_is_auth_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_x/go"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>Please sign in</html>"))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "stale=1".into());
        ctx.config.insert("workspaces".into(), "wrk_x".into());

        let result = provider.fetch_usage(&ctx).await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[tokio::test]
    async fn test_partial_workspace_failure_keeps_successes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_ok/go"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(dashboard_page(20.0, 40.0, None)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_broken/go"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());
        ctx.config
            .insert("workspaces".into(), "wrk_ok,wrk_broken".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!(snap.primary_rate_window.is_some());
        assert!(snap.extra_rate_windows.is_empty());
    }

    #[tokio::test]
    async fn test_all_workspaces_fail_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/workspace/wrk_a/go"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "session=abc".into());
        ctx.config.insert("workspaces".into(), "wrk_a".into());

        let result = provider.fetch_usage(&ctx).await;
        assert!(matches!(result, Err(SpendPanelError::ProviderError(_, _))));
    }
}
