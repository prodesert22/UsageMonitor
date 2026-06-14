//! Windsurf usage provider.
//!
//! Ports CodexBar's `GetPlanStatus` call on Windsurf's Connect RPC service
//! (`exa.seat_management_pb.SeatManagementService`). The request and response
//! are raw protobuf (`Content-Type: application/proto`, Connect unary), so this
//! module hand-encodes the request and decodes the response with the shared
//! [`crate::provider::proto`] reader. Field numbers come from CodexBar, which
//! reverse-engineered them from Windsurf's bundled protobuf metadata.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::proto::{self, Reader, WIRE_LEN, WIRE_VARINT};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

const PATH: &str = "/_backend/exa.seat_management_pb.SeatManagementService/GetPlanStatus";

/// Decoded `PlanStatus` quota fields.
#[derive(Debug, Default, Clone, PartialEq)]
struct PlanStatus {
    plan_name: Option<String>,
    daily_remaining_percent: Option<i64>,
    weekly_remaining_percent: Option<i64>,
    daily_reset_unix: Option<i64>,
    weekly_reset_unix: Option<i64>,
}

/// Windsurf usage provider (Devin-style session auth).
pub struct WindsurfProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl WindsurfProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "windsurf",
                name: "Windsurf",
                description: "Windsurf daily/weekly quota monitor",
                auth_methods: &["token", "env"],
                website: Some("https://windsurf.com"),
            },
            base_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn api_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or("https://windsurf.com")
    }

    fn clean(raw: &str) -> String {
        let mut v = raw.trim();
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = &v[1..v.len() - 1];
        }
        v.trim().to_string()
    }

    fn config_or_env(ctx: &ProviderContext, keys: &[&str], envs: &[&str]) -> Option<String> {
        for key in keys {
            if let Some(v) = ctx.config.get(*key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Some(c);
                }
            }
        }
        for env in envs {
            if let Ok(v) = std::env::var(env) {
                let c = Self::clean(&v);
                if !c.is_empty() {
                    return Some(c);
                }
            }
        }
        None
    }

    fn resolve_session(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        Self::config_or_env(
            ctx,
            &["session_token", "token", "api_key"],
            &["WINDSURF_SESSION_TOKEN"],
        )
        .ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "windsurf".into(),
                "no session token in session_token/token config or WINDSURF_SESSION_TOKEN".into(),
            )
        })
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Builds the Connect-unary request body: `{1: auth_token, 2: include_top_up}`.
    fn encode_request(session_token: &str) -> Vec<u8> {
        let mut body = Vec::new();
        proto::encode_string_field(1, session_token, &mut body);
        proto::encode_varint_field(2, 1, &mut body);
        body
    }

    /// Decodes the `GetPlanStatusResponse` → its nested `PlanStatus` (field 1).
    fn decode_response(data: &[u8]) -> Result<PlanStatus, SpendPanelError> {
        let mut reader = Reader::new(data);
        while let Some((field, wire)) = reader.next_key() {
            if field == 1 && wire == WIRE_LEN {
                let inner = reader.read_len().ok_or_else(parse_err)?;
                return Self::decode_plan_status(inner);
            }
            reader.skip(wire).ok_or_else(parse_err)?;
        }
        // Empty response (no plan status) is still valid — treat as no data.
        Ok(PlanStatus::default())
    }

    fn decode_plan_status(data: &[u8]) -> Result<PlanStatus, SpendPanelError> {
        let mut status = PlanStatus::default();
        let mut reader = Reader::new(data);
        while let Some((field, wire)) = reader.next_key() {
            match (field, wire) {
                (1, WIRE_LEN) => {
                    let inner = reader.read_len().ok_or_else(parse_err)?;
                    status.plan_name = Self::decode_plan_name(inner);
                }
                (14, WIRE_VARINT) => {
                    status.daily_remaining_percent = Some(reader.read_varint().ok_or_else(parse_err)? as i64);
                }
                (15, WIRE_VARINT) => {
                    status.weekly_remaining_percent = Some(reader.read_varint().ok_or_else(parse_err)? as i64);
                }
                (17, WIRE_VARINT) => {
                    status.daily_reset_unix = Some(reader.read_varint().ok_or_else(parse_err)? as i64);
                }
                (18, WIRE_VARINT) => {
                    status.weekly_reset_unix = Some(reader.read_varint().ok_or_else(parse_err)? as i64);
                }
                _ => {
                    reader.skip(wire).ok_or_else(parse_err)?;
                }
            }
        }
        Ok(status)
    }

    /// `PlanInfo { 1: teams_tier (varint), 2: plan_name (string) }`.
    fn decode_plan_name(data: &[u8]) -> Option<String> {
        let mut reader = Reader::new(data);
        while let Some((field, wire)) = reader.next_key() {
            if field == 2 && wire == WIRE_LEN {
                let bytes = reader.read_len()?;
                return std::str::from_utf8(bytes)
                    .ok()
                    .map(str::to_string)
                    .filter(|s| !s.is_empty());
            }
            reader.skip(wire)?;
        }
        None
    }

    fn snapshot_from(status: &PlanStatus) -> Result<UsageSnapshot, SpendPanelError> {
        let to_date =
            |unix: Option<i64>| -> Option<DateTime<Utc>> { unix.and_then(|s| Utc.timestamp_opt(s, 0).single()) };

        let mut snapshot = UsageSnapshot::new("windsurf");

        if let Some(daily) = status.daily_remaining_percent {
            let used = (100 - daily).clamp(0, 100) as u64;
            let mut w = RateWindow::new(used, 100, "Daily", 24 * 60);
            w.resets_at = to_date(status.daily_reset_unix);
            snapshot.primary_rate_window = Some(w);
        }
        if let Some(weekly) = status.weekly_remaining_percent {
            let used = (100 - weekly).clamp(0, 100) as u64;
            let mut w = RateWindow::new(used, 100, "Weekly", 7 * 24 * 60);
            w.resets_at = to_date(status.weekly_reset_unix);
            snapshot.secondary_rate_window = Some(w);
        }

        if snapshot.primary_rate_window.is_none() && snapshot.secondary_rate_window.is_none() {
            return Err(SpendPanelError::ParseError(
                "windsurf".into(),
                "no quota data in plan status response".into(),
            ));
        }

        if let Some(plan) = &status.plan_name {
            snapshot.plan = Some(PlanInfo {
                name: plan.clone(),
                tier: None,
                features: Vec::new(),
                price: None,
                currency: None,
                billing_period: None,
            });
        }
        Ok(snapshot)
    }
}

fn parse_err() -> SpendPanelError {
    SpendPanelError::ParseError("windsurf".into(), "malformed protobuf response".into())
}

impl Default for WindsurfProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for WindsurfProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("WINDSURF_SESSION_TOKEN")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let session = Self::resolve_session(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!("{}{}", self.api_base().trim_end_matches('/'), PATH);

        // Optional Devin-session headers; the request body carries the token too.
        let auth1 = Self::config_or_env(ctx, &["auth1_token"], &["WINDSURF_AUTH1_TOKEN"]);
        let account_id = Self::config_or_env(ctx, &["account_id"], &["WINDSURF_ACCOUNT_ID"]);
        let org_id = Self::config_or_env(ctx, &["primary_org_id"], &["WINDSURF_PRIMARY_ORG_ID"]);

        let mut req = client
            .post(url)
            .header("Content-Type", "application/proto")
            .header("Connect-Protocol-Version", "1")
            .header("Origin", "https://windsurf.com")
            .header("Referer", "https://windsurf.com/profile")
            .header("x-auth-token", &session)
            .header("x-devin-session-token", &session)
            .body(Self::encode_request(&session));
        if let Some(v) = &auth1 {
            req = req.header("x-devin-auth1-token", v);
        }
        if let Some(v) = &account_id {
            req = req.header("x-devin-account-id", v);
        }
        if let Some(v) = &org_id {
            req = req.header("x-devin-primary-org-id", v);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(SpendPanelError::AuthFailed(
                "windsurf".into(),
                format!("session token rejected (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            return Err(SpendPanelError::ProviderError(
                "windsurf".into(),
                format!("HTTP {}: {}", status, body.chars().take(200).collect::<String>()),
            ));
        }
        let plan_status = Self::decode_response(&bytes)?;
        Self::snapshot_from(&plan_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::proto::{encode_key, encode_varint, encode_varint_field};
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Wraps `inner` as a length-delimited field of `parent_field`.
    fn nested(parent_field: u32, inner: &[u8], out: &mut Vec<u8>) {
        encode_key(parent_field, WIRE_LEN, out);
        encode_varint(inner.len() as u64, out);
        out.extend_from_slice(inner);
    }

    fn sample_response() -> Vec<u8> {
        // PlanInfo { 1: tier=2, 2: name="Pro" }
        let mut plan_info = Vec::new();
        encode_varint_field(1, 2, &mut plan_info);
        proto::encode_string_field(2, "Pro", &mut plan_info);

        // PlanStatus { 1: plan_info, 14: daily=25, 15: weekly=80, 17: dReset, 18: wReset }
        let mut plan_status = Vec::new();
        nested(1, &plan_info, &mut plan_status);
        encode_varint_field(14, 25, &mut plan_status);
        encode_varint_field(15, 80, &mut plan_status);
        encode_varint_field(17, 1_788_000_000, &mut plan_status);
        encode_varint_field(18, 1_788_500_000, &mut plan_status);

        // GetPlanStatusResponse { 1: plan_status }
        let mut resp = Vec::new();
        nested(1, &plan_status, &mut resp);
        resp
    }

    #[test]
    fn test_metadata() {
        assert_eq!(WindsurfProvider::new().metadata().id, "windsurf");
    }

    #[test]
    fn test_encode_request_roundtrips() {
        let body = WindsurfProvider::encode_request("sess-abc");
        let mut r = Reader::new(&body);
        assert_eq!(r.next_key(), Some((1, WIRE_LEN)));
        assert_eq!(r.read_len(), Some(&b"sess-abc"[..]));
        assert_eq!(r.next_key(), Some((2, WIRE_VARINT)));
        assert_eq!(r.read_varint(), Some(1));
    }

    #[test]
    fn test_decode_response() {
        let status = WindsurfProvider::decode_response(&sample_response()).unwrap();
        assert_eq!(status.plan_name.as_deref(), Some("Pro"));
        assert_eq!(status.daily_remaining_percent, Some(25));
        assert_eq!(status.weekly_remaining_percent, Some(80));
        assert_eq!(status.daily_reset_unix, Some(1_788_000_000));
    }

    #[test]
    fn test_snapshot_maps_windows() {
        let status = WindsurfProvider::decode_response(&sample_response()).unwrap();
        let snap = WindsurfProvider::snapshot_from(&status).unwrap();
        // 25% remaining → 75% used; 80% remaining → 20% used.
        assert_eq!(snap.primary_rate_window.as_ref().unwrap().used, Some(75));
        assert_eq!(snap.secondary_rate_window.as_ref().unwrap().used, Some(20));
        assert!(snap.primary_rate_window.as_ref().unwrap().resets_at.is_some());
        assert_eq!(snap.plan.unwrap().name, "Pro");
    }

    #[test]
    fn test_empty_plan_status_is_error() {
        // Response with a plan status that carries no quota fields.
        let mut resp = Vec::new();
        nested(1, &[], &mut resp);
        let status = WindsurfProvider::decode_response(&resp).unwrap();
        assert!(matches!(
            WindsurfProvider::snapshot_from(&status).unwrap_err(),
            SpendPanelError::ParseError(_, _)
        ));
    }

    #[test]
    fn test_resolve_session_missing() {
        assert!(matches!(
            WindsurfProvider::resolve_session(&ProviderContext::new()).unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .and(header("x-auth-token", "sess"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(sample_response(), "application/proto"),
            )
            .mount(&server)
            .await;
        let provider = WindsurfProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("session_token".into(), "sess".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(75));
    }

    #[tokio::test]
    async fn test_fetch_usage_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = WindsurfProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("session_token".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
