use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, PlanInfo, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct ProjectsResponse {
    projects: Vec<Project>,
}

#[derive(Debug, serde::Deserialize, Clone, PartialEq)]
struct Project {
    project_id: String,
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct UsageResponse {
    start: Option<String>,
    end: Option<String>,
    results: Vec<UsageResult>,
}

#[derive(Debug, serde::Deserialize)]
struct UsageResult {
    hours: Option<f64>,
    total_hours: Option<f64>,
    agent_hours: Option<f64>,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    tts_characters: Option<u64>,
    requests: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct DeepgramUsage {
    project_id: String,
    project_name: Option<String>,
    project_count: usize,
    start: Option<String>,
    end: Option<String>,
    hours: f64,
    total_hours: f64,
    agent_hours: f64,
    tokens_in: u64,
    tokens_out: u64,
    tts_characters: u64,
    requests: u64,
}

pub struct DeepgramProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl DeepgramProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "deepgram",
                name: "Deepgram",
                description: "Deepgram usage breakdown monitor",
                auth_methods: &["api_key", "project_id", "env"],
                website: Some("https://deepgram.com"),
            },
            base_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn clean(raw: &str) -> String {
        let mut value = raw.trim();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = &value[1..value.len() - 1];
        }
        value.trim().to_string()
    }

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        if let Ok(value) = std::env::var("DEEPGRAM_API_KEY") {
            let cleaned = Self::clean(&value);
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "deepgram".into(),
            "no API key found in config, token, or DEEPGRAM_API_KEY".into(),
        ))
    }

    fn resolve_project_id(ctx: &ProviderContext) -> Option<String> {
        ctx.config
            .get("project_id")
            .map(|v| Self::clean(v))
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("DEEPGRAM_PROJECT_ID")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .filter(|v| !v.is_empty())
    }

    fn api_base(&self, ctx: &ProviderContext) -> String {
        let configured = ctx
            .config
            .get("api_url")
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| {
                std::env::var("DEEPGRAM_API_URL")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| "https://api.deepgram.com/v1".into());
        let base = if configured.starts_with("http://") || configured.starts_with("https://") {
            configured
        } else {
            format!("https://{}", configured)
        };
        base.trim_end_matches('/').to_string()
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
    ) -> Result<T, SpendPanelError> {
        let resp = client
            .get(url)
            .header("Authorization", format!("Token {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        match status.as_u16() {
            200 => serde_json::from_str(&body)
                .map_err(|e| SpendPanelError::ParseError("deepgram".into(), e.to_string())),
            401 => Err(SpendPanelError::AuthFailed(
                "deepgram".into(),
                "API key is invalid or expired".into(),
            )),
            403 => Err(SpendPanelError::AuthFailed(
                "deepgram".into(),
                "API key does not have access to project or Management API".into(),
            )),
            _ => Err(SpendPanelError::ProviderError(
                "deepgram".into(),
                format!("HTTP {}: {}", status, body),
            )),
        }
    }

    async fn list_projects(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
    ) -> Result<Vec<Project>, SpendPanelError> {
        let response: ProjectsResponse =
            Self::get_json(client, format!("{}/projects", base_url), api_key).await?;
        Ok(response.projects)
    }

    async fn fetch_project_usage(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        project: Project,
        ctx: &ProviderContext,
    ) -> Result<DeepgramUsage, SpendPanelError> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/projects/{}/usage/breakdown",
            base_url, project.project_id
        ))
        .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        for key in ["start", "end"] {
            if let Some(value) = ctx
                .config
                .get(key)
                .map(|v| Self::clean(v))
                .filter(|v| !v.is_empty())
            {
                url.query_pairs_mut().append_pair(key, &value);
            }
        }
        let response: UsageResponse = Self::get_json(client, url.to_string(), api_key).await?;
        Ok(Self::parse_usage(project, response))
    }

    fn parse_usage(project: Project, response: UsageResponse) -> DeepgramUsage {
        DeepgramUsage {
            project_id: project.project_id,
            project_name: project.name,
            project_count: 1,
            start: response.start,
            end: response.end,
            hours: response
                .results
                .iter()
                .map(|r| r.hours.unwrap_or(0.0))
                .sum(),
            total_hours: response
                .results
                .iter()
                .map(|r| r.total_hours.unwrap_or(0.0))
                .sum(),
            agent_hours: response
                .results
                .iter()
                .map(|r| r.agent_hours.unwrap_or(0.0))
                .sum(),
            tokens_in: response
                .results
                .iter()
                .map(|r| r.tokens_in.unwrap_or(0))
                .sum(),
            tokens_out: response
                .results
                .iter()
                .map(|r| r.tokens_out.unwrap_or(0))
                .sum(),
            tts_characters: response
                .results
                .iter()
                .map(|r| r.tts_characters.unwrap_or(0))
                .sum(),
            requests: response
                .results
                .iter()
                .map(|r| r.requests.unwrap_or(0))
                .sum(),
        }
    }

    fn aggregate(usages: Vec<DeepgramUsage>) -> Result<DeepgramUsage, SpendPanelError> {
        let Some(first) = usages.first() else {
            return Err(SpendPanelError::ProviderError(
                "deepgram".into(),
                "no projects returned".into(),
            ));
        };
        if usages.len() == 1 {
            return Ok(first.clone());
        }
        Ok(DeepgramUsage {
            project_id: "all".into(),
            project_name: None,
            project_count: usages.len(),
            start: usages.iter().filter_map(|u| u.start.clone()).min(),
            end: usages.iter().filter_map(|u| u.end.clone()).max(),
            hours: usages.iter().map(|u| u.hours).sum(),
            total_hours: usages.iter().map(|u| u.total_hours).sum(),
            agent_hours: usages.iter().map(|u| u.agent_hours).sum(),
            tokens_in: usages.iter().map(|u| u.tokens_in).sum(),
            tokens_out: usages.iter().map(|u| u.tokens_out).sum(),
            tts_characters: usages.iter().map(|u| u.tts_characters).sum(),
            requests: usages.iter().map(|u| u.requests).sum(),
        })
    }

    fn format_int(value: u64) -> String {
        let s = value.to_string();
        let mut out = String::new();
        for (i, ch) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out.chars().rev().collect()
    }

    fn format_decimal(value: f64) -> String {
        if value.fract() == 0.0 {
            format!("{:.0}", value)
        } else {
            format!("{:.1}", value)
        }
    }

    fn zero_window(label: impl Into<String>) -> RateWindow {
        RateWindow {
            label: label.into(),
            window_minutes: 0,
            usage_ratio: 0.0,
            limit: None,
            used: None,
            remaining: None,
            resets_at: None,
            status: RateWindowStatus::Normal,
        }
    }

    fn identity_label(usage: &DeepgramUsage) -> String {
        if usage.project_count > 1 {
            format!("{} projects", usage.project_count)
        } else if let Some(name) = usage
            .project_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            format!("Project: {}", name.trim())
        } else {
            format!("Project: {}", usage.project_id)
        }
    }

    fn snapshot_from_usage(usage: DeepgramUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("deepgram");
        snapshot.primary_rate_window = Some(Self::zero_window(format!(
            "Requests {}",
            Self::format_int(usage.requests)
        )));
        snapshot.secondary_rate_window = Some(Self::zero_window(format!(
            "Audio {} h · Billable {} h",
            Self::format_decimal(usage.hours),
            Self::format_decimal(usage.total_hours)
        )));
        let token_total = usage.tokens_in + usage.tokens_out;
        snapshot.tertiary_rate_window = Some(Self::zero_window(format!(
            "Models {} tokens · {} TTS chars",
            Self::format_int(token_total),
            Self::format_int(usage.tts_characters)
        )));
        if usage.agent_hours > 0.0 {
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: "agent-hours".into(),
                label: "Agent hours".into(),
                window: Self::zero_window(format!(
                    "Agent {} h",
                    Self::format_decimal(usage.agent_hours)
                )),
            });
        }
        let mut features = vec![Self::identity_label(&usage)];
        if let (Some(start), Some(end)) = (&usage.start, &usage.end) {
            features.push(format!("period: {} to {}", start, end));
        }
        snapshot.plan = Some(PlanInfo {
            name: "Deepgram API".into(),
            tier: None,
            features,
            price: None,
            currency: None,
            billing_period: None,
        });
        snapshot
    }
}

impl Default for DeepgramProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for DeepgramProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("DEEPGRAM_API_KEY").is_ok_and(|v| !Self::clean(&v).is_empty())
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let base_url = self.api_base(ctx);
        let usages = if let Some(project_id) = Self::resolve_project_id(ctx) {
            vec![
                Self::fetch_project_usage(
                    &client,
                    &base_url,
                    &api_key,
                    Project {
                        project_id,
                        name: None,
                    },
                    ctx,
                )
                .await?,
            ]
        } else {
            let projects = Self::list_projects(&client, &base_url, &api_key).await?;
            if projects.is_empty() {
                return Err(SpendPanelError::ProviderError(
                    "deepgram".into(),
                    "no projects returned".into(),
                ));
            }
            let mut usages = Vec::with_capacity(projects.len());
            for project in projects {
                usages.push(
                    Self::fetch_project_usage(&client, &base_url, &api_key, project, ctx).await?,
                );
            }
            usages
        };
        Ok(Self::snapshot_from_usage(Self::aggregate(usages)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const USAGE: &str = r#"{
      "start":"2025-01-16",
      "end":"2025-01-23",
      "results":[
        {"hours":1619.7242069444444,"total_hours":1621.7395791666668,"agent_hours":41.33564388888889,"tokens_in":1200,"tokens_out":340,"tts_characters":9158866,"requests":373381},
        {"hours":2.25,"total_hours":3.5,"requests":19}
      ]
    }"#;

    fn usage_response() -> UsageResponse {
        serde_json::from_str(USAGE).unwrap()
    }

    #[test]
    fn test_provider_metadata() {
        let meta = DeepgramProvider::new().metadata().clone();
        assert_eq!(meta.id, "deepgram");
        assert_eq!(meta.name, "Deepgram");
    }

    #[test]
    fn test_parse_usage_breakdown_response() {
        let usage = DeepgramProvider::parse_usage(
            Project {
                project_id: "project-123".into(),
                name: None,
            },
            usage_response(),
        );
        assert_eq!(usage.requests, 373_400);
        assert_eq!(usage.tokens_in, 1200);
        assert_eq!(usage.tokens_out, 340);
        assert_eq!(usage.tts_characters, 9_158_866);
        let snapshot = DeepgramProvider::snapshot_from_usage(usage);
        assert_eq!(
            snapshot.primary_rate_window.unwrap().label,
            "Requests 373,400"
        );
        assert!(
            snapshot
                .secondary_rate_window
                .unwrap()
                .label
                .contains("Audio 1622.0 h")
        );
        assert!(
            snapshot
                .tertiary_rate_window
                .unwrap()
                .label
                .contains("1,540 tokens")
        );
        assert_eq!(snapshot.extra_rate_windows.len(), 1);
    }

    #[test]
    fn test_aggregate_projects() {
        let a = DeepgramUsage {
            project_id: "a".into(),
            project_name: Some("A".into()),
            project_count: 1,
            start: Some("2025-01-16".into()),
            end: Some("2025-01-23".into()),
            hours: 1.0,
            total_hours: 2.0,
            agent_hours: 0.0,
            tokens_in: 1,
            tokens_out: 2,
            tts_characters: 3,
            requests: 4,
        };
        let b = DeepgramUsage {
            project_id: "b".into(),
            project_name: Some("B".into()),
            project_count: 1,
            start: Some("2025-01-17".into()),
            end: Some("2025-01-24".into()),
            hours: 4.0,
            total_hours: 5.0,
            agent_hours: 0.0,
            tokens_in: 10,
            tokens_out: 20,
            tts_characters: 30,
            requests: 6,
        };
        let usage = DeepgramProvider::aggregate(vec![a, b]).unwrap();
        assert_eq!(usage.project_id, "all");
        assert_eq!(usage.project_count, 2);
        assert_eq!(usage.requests, 10);
        assert_eq!(usage.hours, 5.0);
        assert_eq!(usage.start.as_deref(), Some("2025-01-16"));
        assert_eq!(usage.end.as_deref(), Some("2025-01-24"));
    }

    #[tokio::test]
    async fn test_fetch_usage_calls_breakdown_endpoint_with_token_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/project-123/usage/breakdown"))
            .and(query_param("start", "2025-01-16"))
            .and(query_param("end", "2025-01-23"))
            .and(header("authorization", "Token dg-test"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(USAGE, "application/json"))
            .mount(&server)
            .await;
        let provider = DeepgramProvider::with_base_url(&format!("{}/v1", server.uri()));
        let mut ctx = ProviderContext::with_api_key("dg-test");
        ctx.config.insert("project_id".into(), "project-123".into());
        ctx.config.insert("start".into(), "2025-01-16".into());
        ctx.config.insert("end".into(), "2025-01-23".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(
            snapshot.primary_rate_window.unwrap().label,
            "Requests 373,400"
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_discovers_projects_when_project_id_omitted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"projects":[{"project_id":"project-a","name":"Alpha"},{"project_id":"project-b","name":"Beta"}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/project-a/usage/breakdown"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"start":"2025-01-16","end":"2025-01-23","results":[{"hours":1,"total_hours":2,"requests":3}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/project-b/usage/breakdown"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"start":"2025-01-17","end":"2025-01-24","results":[{"hours":4,"total_hours":5,"requests":6}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider = DeepgramProvider::with_base_url(&format!("{}/v1", server.uri()));
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("dg-test"))
            .await
            .unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().label, "Requests 9");
        assert!(
            snapshot
                .plan
                .unwrap()
                .features
                .contains(&"2 projects".into())
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/project-123/usage/breakdown"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = DeepgramProvider::with_base_url(&format!("{}/v1", server.uri()));
        let mut ctx = ProviderContext::with_api_key("bad");
        ctx.config.insert("project_id".into(), "project-123".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
