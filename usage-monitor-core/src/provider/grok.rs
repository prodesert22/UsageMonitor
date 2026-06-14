//! Grok usage provider.
//!
//! Ports CodexBar's `GetGrokCreditsConfig` call on Grok's gRPC-Web billing
//! service. The request is an empty gRPC-Web frame and the response is a
//! protobuf message whose exact schema is not published, so — like CodexBar —
//! this module generically scans the protobuf for the credit-usage percentage
//! (a `float`/fixed32 ending in field 1, in `0..=100`) and the quota reset
//! timestamp (a unix-seconds varint, preferring path `1.5.1`).

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::SpendPanelError;
use crate::model::{RateWindow, UsageSnapshot};
use crate::provider::proto::{Reader, WIRE_FIXED32, WIRE_FIXED64, WIRE_LEN, WIRE_VARINT};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

const ENDPOINT_PATH: &str = "/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";

#[derive(Debug, Default)]
struct Scan {
    /// (path, value, order) for fixed32 (float) fields.
    fixed32: Vec<(Vec<u32>, f32, usize)>,
    /// (path, value) for varint fields.
    varint: Vec<(Vec<u32>, u64)>,
}

/// Recursively scans a protobuf message, recording fixed32 and varint fields
/// with their nested field-number path (depth-limited, like CodexBar).
fn scan_protobuf(data: &[u8], path: &[u32], depth: u8, order: &mut usize, scan: &mut Scan) {
    let mut reader = Reader::new(data);
    while let Some((field, wire)) = reader.next_key() {
        let mut field_path = path.to_vec();
        field_path.push(field);
        match wire {
            WIRE_VARINT => {
                let Some(v) = reader.read_varint() else { return };
                scan.varint.push((field_path, v));
            }
            WIRE_FIXED64 => {
                if reader.read_fixed64().is_none() {
                    return;
                }
            }
            WIRE_LEN => {
                let Some(inner) = reader.read_len() else { return };
                if depth < 4 {
                    scan_protobuf(inner, &field_path, depth + 1, order, scan);
                }
            }
            WIRE_FIXED32 => {
                let Some(bits) = reader.read_fixed32() else { return };
                scan.fixed32.push((field_path, f32::from_bits(bits), *order));
                *order += 1;
            }
            _ => return,
        }
    }
}

/// Splits a gRPC-Web body into its data-frame payloads (flag bit 0x80 clear).
fn grpc_web_data_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut i = 0;
    while i + 5 <= data.len() {
        let flags = data[i];
        let len = u32::from_be_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]) as usize;
        let start = i + 5;
        let Some(end) = start.checked_add(len) else {
            return Vec::new();
        };
        if end > data.len() {
            return Vec::new();
        }
        if flags & 0x80 == 0 {
            frames.push(&data[start..end]);
        }
        i = end;
    }
    frames
}

/// gRPC status from trailer frames (flag bit 0x80 set), `None` if unset (== OK).
fn grpc_web_trailer_status(data: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i + 5 <= data.len() {
        let flags = data[i];
        let len = u32::from_be_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]) as usize;
        let start = i + 5;
        let end = start.checked_add(len)?;
        if end > data.len() {
            break;
        }
        let trailer = (flags & 0x80 != 0)
            .then(|| std::str::from_utf8(&data[start..end]).ok())
            .flatten();
        if let Some(text) = trailer {
            for line in text.split(['\r', '\n']).filter(|l| !l.is_empty()) {
                if let Some((_, value)) = line
                    .split_once(':')
                    .filter(|(k, _)| k.trim().eq_ignore_ascii_case("grpc-status"))
                {
                    return value.trim().parse::<i64>().ok();
                }
            }
        }
        i = end;
    }
    None
}

fn looks_like_protobuf(data: &[u8]) -> bool {
    let Some(&first) = data.first() else {
        return false;
    };
    let field = first >> 3;
    let wire = first & 0x07;
    field > 0 && matches!(wire, 0 | 1 | 2 | 5)
}

/// Grok usage provider (xAI Bearer token or grok.com cookie auth).
pub struct GrokProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl GrokProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "grok",
                name: "Grok",
                description: "Grok credit-usage monitor (gRPC-Web billing)",
                auth_methods: &["token", "cookie", "env"],
                website: Some("https://grok.com"),
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
        self.base_url.as_deref().unwrap_or("https://grok.com")
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

    /// Resolves `(authorization, cookie)` — at least one is required.
    fn resolve_auth(ctx: &ProviderContext) -> Result<(Option<String>, Option<String>), SpendPanelError> {
        let token = ["token", "access_token", "api_key"]
            .iter()
            .find_map(|k| ctx.config.get(*k).map(|v| Self::clean(v)).filter(|c| !c.is_empty()))
            .or_else(|| {
                ["GROK_TOKEN", "GROK_ACCESS_TOKEN"]
                    .iter()
                    .find_map(|e| std::env::var(e).ok().map(|v| Self::clean(&v)).filter(|c| !c.is_empty()))
            });
        let cookie = ctx
            .config
            .get("cookie")
            .map(|v| Self::clean(v))
            .filter(|c| !c.is_empty())
            .or_else(|| std::env::var("GROK_COOKIE").ok().map(|v| Self::clean(&v)).filter(|c| !c.is_empty()));

        if token.is_none() && cookie.is_none() {
            return Err(SpendPanelError::AuthFailed(
                "grok".into(),
                "no Bearer token or cookie in config (token/cookie) or GROK_TOKEN/GROK_COOKIE".into(),
            ));
        }
        Ok((token.map(|t| format!("Bearer {}", t)), cookie))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Extracts the credit-usage percentage and reset from the protobuf payloads.
    fn parse_payloads(payloads: &[&[u8]], now: DateTime<Utc>) -> Result<UsageSnapshot, SpendPanelError> {
        let mut scan = Scan::default();
        let mut order = 0usize;
        for payload in payloads {
            scan_protobuf(payload, &[], 0, &mut order, &mut scan);
        }

        // Usage percent: a float field whose path ends in field 1, in 0..=100.
        // Prefer the shallowest path, then the earliest-seen value.
        let parsed_percent = scan
            .fixed32
            .iter()
            .filter(|(path, v, _)| {
                path.last() == Some(&1) && v.is_finite() && *v >= 0.0 && *v <= 100.0
            })
            .min_by(|a, b| {
                a.0.len()
                    .cmp(&b.0.len())
                    .then(a.2.cmp(&b.2))
            })
            .map(|(_, v, _)| *v as f64);

        // Reset: a future unix-seconds varint, preferring path 1.5.1.
        let now_ts = now.timestamp() as u64;
        let resets: Vec<(&Vec<u32>, DateTime<Utc>)> = scan
            .varint
            .iter()
            .filter(|(_, raw)| *raw >= 1_700_000_000 && *raw <= 2_100_000_000)
            .filter_map(|(path, raw)| Utc.timestamp_opt(*raw as i64, 0).single().map(|d| (path, d)))
            .filter(|(_, d)| d.timestamp() as u64 > now_ts)
            .collect();
        let preferred_reset = resets
            .iter()
            .filter(|(path, _)| path.as_slice() == [1, 5, 1])
            .map(|(_, d)| *d)
            .min();
        let reset = preferred_reset.or_else(|| resets.iter().map(|(_, d)| *d).min());

        // A fresh billing period can report no usage yet (no float, but a reset
        // and a usage-period marker) — treat that as 0% used.
        let has_usage_period = scan.varint.iter().any(|(path, value)| {
            path.starts_with(&[1, 6]) || (path.as_slice() == [1, 8, 1] && (*value == 1 || *value == 2))
        });
        let no_usage_yet =
            parsed_percent.is_none() && scan.fixed32.is_empty() && reset.is_some() && has_usage_period;

        let percent = parsed_percent
            .or(if no_usage_yet { Some(0.0) } else { None })
            .ok_or_else(|| {
                SpendPanelError::ParseError("grok".into(), "no credit usage found in response".into())
            })?;

        let mut snapshot = UsageSnapshot::new("grok");
        let mut window = RateWindow::new(percent.round() as u64, 100, "Credits", 30 * 24 * 60);
        window.resets_at = reset;
        snapshot.primary_rate_window = Some(window);
        Ok(snapshot)
    }

    fn parse_response(data: &[u8], now: DateTime<Utc>) -> Result<UsageSnapshot, SpendPanelError> {
        if let Some(status) = grpc_web_trailer_status(data).filter(|s| *s != 0) {
            return Err(SpendPanelError::ProviderError(
                "grok".into(),
                format!("gRPC status {} (re-authenticate at grok.com)", status),
            ));
        }
        let mut payloads = grpc_web_data_frames(data);
        if payloads.is_empty() && looks_like_protobuf(data) {
            payloads = vec![data];
        }
        if payloads.is_empty() {
            return Err(SpendPanelError::ParseError(
                "grok".into(),
                "empty gRPC-Web response".into(),
            ));
        }
        Self::parse_payloads(&payloads, now)
    }
}

impl Default for GrokProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for GrokProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["GROK_TOKEN", "GROK_ACCESS_TOKEN", "GROK_COOKIE"]
            .iter()
            .any(|e| std::env::var(e).map(|v| !v.trim().is_empty()).unwrap_or(false))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let (authorization, cookie) = Self::resolve_auth(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!("{}{}", self.api_base().trim_end_matches('/'), ENDPOINT_PATH);

        let mut req = client
            .post(url)
            .header("Content-Type", "application/grpc-web+proto")
            .header("x-grpc-web", "1")
            .header("Accept", "*/*")
            .header("Origin", "https://grok.com")
            .header("Referer", "https://grok.com/?_s=usage")
            // Empty gRPC-Web frame: 1 flag byte + 4-byte length (0).
            .body(vec![0u8, 0, 0, 0, 0]);
        if let Some(auth) = &authorization {
            req = req.header("Authorization", auth);
        }
        if let Some(cookie) = &cookie {
            req = req.header("Cookie", cookie);
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
                "grok".into(),
                format!("credentials rejected (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "grok".into(),
                format!("HTTP {}", status.as_u16()),
            ));
        }
        Self::parse_response(&bytes, Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::proto::{encode_key, encode_varint};
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Encodes a fixed32 (float) field.
    fn float_field(field: u32, value: f32, out: &mut Vec<u8>) {
        encode_key(field, WIRE_FIXED32, out);
        out.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Wraps `inner` as a length-delimited field.
    fn nested(field: u32, inner: &[u8], out: &mut Vec<u8>) {
        encode_key(field, WIRE_LEN, out);
        encode_varint(inner.len() as u64, out);
        out.extend_from_slice(inner);
    }

    /// Wraps a protobuf payload in a gRPC-Web data frame.
    fn data_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0u8];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    fn trailer_frame(text: &str) -> Vec<u8> {
        let mut frame = vec![0x80u8];
        frame.extend_from_slice(&(text.len() as u32).to_be_bytes());
        frame.extend_from_slice(text.as_bytes());
        frame
    }

    #[test]
    fn test_metadata() {
        assert_eq!(GrokProvider::new().metadata().id, "grok");
    }

    #[test]
    fn test_resolve_auth_missing() {
        assert!(matches!(
            GrokProvider::resolve_auth(&ProviderContext::new()).unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }

    #[test]
    fn test_resolve_auth_bearer() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "xai-key".into());
        let (auth, cookie) = GrokProvider::resolve_auth(&ctx).unwrap();
        assert_eq!(auth.as_deref(), Some("Bearer xai-key"));
        assert!(cookie.is_none());
    }

    #[test]
    fn test_scan_finds_percent() {
        // message { 1: { 1: float 42.5 } } → path [1,1] ends in field 1.
        let mut inner = Vec::new();
        float_field(1, 42.5, &mut inner);
        let mut msg = Vec::new();
        nested(1, &inner, &mut msg);

        let snap = GrokProvider::parse_payloads(&[&msg], Utc::now()).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(43)); // 42.5 rounds to 43
    }

    #[test]
    fn test_no_usage_yet_zero_percent() {
        // No float, but a future reset at path 1.5.1 and a usage-period marker
        // at 1.6.x → 0% used.
        let future = (Utc::now().timestamp() + 86_400) as u64;
        // 1 -> { 5 -> { 1: varint reset }, 6 -> { 1: varint 1 } }
        let mut f5 = Vec::new();
        encode_key(1, WIRE_VARINT, &mut f5);
        encode_varint(future, &mut f5);
        let mut f6 = Vec::new();
        encode_key(1, WIRE_VARINT, &mut f6);
        encode_varint(1, &mut f6);
        let mut f1 = Vec::new();
        nested(5, &f5, &mut f1);
        nested(6, &f6, &mut f1);
        let mut msg = Vec::new();
        nested(1, &f1, &mut msg);

        let snap = GrokProvider::parse_payloads(&[&msg], Utc::now()).unwrap();
        let window = snap.primary_rate_window.unwrap();
        assert_eq!(window.used, Some(0));
        assert!(window.resets_at.is_some());
    }

    #[test]
    fn test_grpc_web_frame_split() {
        let payload = vec![0x0d, 0, 0, 0, 0];
        let mut body = data_frame(&payload);
        body.extend_from_slice(&trailer_frame("grpc-status:0\r\n"));
        let frames = grpc_web_data_frames(&body);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], &payload[..]);
        assert_eq!(grpc_web_trailer_status(&body), Some(0));
    }

    #[test]
    fn test_parse_response_rejects_grpc_error() {
        let body = trailer_frame("grpc-status:16\r\ngrpc-message:unauthenticated\r\n");
        assert!(matches!(
            GrokProvider::parse_response(&body, Utc::now()).unwrap_err(),
            SpendPanelError::ProviderError(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let mut inner = Vec::new();
        float_field(1, 30.0, &mut inner);
        let mut msg = Vec::new();
        nested(1, &inner, &mut msg);
        let mut body = data_frame(&msg);
        body.extend_from_slice(&trailer_frame("grpc-status:0\r\n"));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/grpc-web+proto"))
            .mount(&server)
            .await;
        let provider = GrokProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "xai".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(30));
    }

    #[tokio::test]
    async fn test_fetch_usage_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT_PATH))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = GrokProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
