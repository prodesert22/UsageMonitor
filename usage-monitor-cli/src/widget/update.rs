//! Widget self-update support: version comparison, update status and changelogs.
//!
//! Version detection is local: each installed widget records its version in a
//! stamp file (see [`super::install`]), and `check-update` compares that stamp
//! with this binary's `CARGO_PKG_VERSION`. The changelog body for the newer
//! version comes from the GitHub Releases page of that tag, with the
//! `CHANGELOG.md` section embedded in the binary as the offline fallback.

use std::cmp::Ordering;

use anyhow::Result;
use serde::Serialize;

use super::install::TARGETS;
use super::install::{VERSION, read_stamp};
use crate::cli::WidgetInstallTarget;

const REPO: &str = "prodesert22/UsageMonitor";
// In-package copy of the workspace CHANGELOG (kept in sync by
// `embedded_changelog_matches_workspace` below): `cargo publish` tarballs
// only ship the package directory, so the workspace root is unreachable.
const CHANGELOG_MD: &str = include_str!("../../CHANGELOG.md");

/// Parse the numeric prefix of a `major.minor.patch…` version string.
///
/// Tolerates a leading `v`, surrounding whitespace and non-numeric suffixes
/// (`1.2.3`, `v1.2`, `0.8.1-rc.1` all parse); unparsable parts become 0.
pub(crate) fn parse_version(raw: &str) -> Vec<u64> {
    raw.trim()
        .trim_start_matches(['v', 'V', '=', ' '])
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// Compare two version strings numerically, padding the shorter one with zeros.
pub(crate) fn compare_versions(installed: &str, current: &str) -> Ordering {
    let mut left = parse_version(installed);
    let mut right = parse_version(current);
    let width = left.len().max(right.len()).max(1);
    left.resize(width, 0);
    right.resize(width, 0);
    left.cmp(&right)
}

/// True when a widget stamp exists and is older than this binary.
///
/// A missing stamp means "never installed" (nothing to update); a stamp newer
/// than the binary (downgraded CLI) is not an update either.
pub(crate) fn is_outdated(installed: Option<&str>, current: &str) -> bool {
    matches!(installed, Some(v) if compare_versions(v, current) == Ordering::Less)
}

pub(crate) fn release_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{version}")
}

pub(crate) fn github_release_url(version: &str) -> String {
    format!("https://api.github.com/repos/{REPO}/releases/tags/v{version}")
}

/// Direct raw URL of the per-version release file, e.g.
/// `releases/v0.8.1.md`. The release files are self-contained notes, so they
/// render as-is with no link rewriting.
pub(crate) fn release_file_url(version: &str, git_ref: &str) -> String {
    format!("https://raw.githubusercontent.com/{REPO}/{git_ref}/releases/v{version}.md")
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct UpdateStatus {
    pub(crate) target: String,
    pub(crate) installed: Option<String>,
    pub(crate) available: String,
    pub(crate) outdated: bool,
}

pub(crate) fn status_for(target: &str) -> Result<UpdateStatus> {
    let installed = read_stamp(target)?;
    Ok(UpdateStatus {
        target: target.to_string(),
        installed: installed.clone(),
        available: VERSION.to_string(),
        outdated: is_outdated(installed.as_deref(), VERSION),
    })
}

pub(crate) fn check_updates(target: Option<WidgetInstallTarget>) -> Result<Vec<UpdateStatus>> {
    match target {
        Some(WidgetInstallTarget::Kde) => Ok(vec![status_for("kde")?]),
        Some(WidgetInstallTarget::Waybar) => Ok(vec![status_for("waybar")?]),
        Some(WidgetInstallTarget::All) | None => TARGETS
            .iter()
            .map(|target| status_for(target))
            .collect::<Result<Vec<_>>>(),
    }
}

pub(crate) fn run_check_update(target: Option<WidgetInstallTarget>, pretty: bool) -> Result<()> {
    let updates = check_updates(target)?;
    let payload = serde_json::json!({ "updates": updates });
    if pretty {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", serde_json::to_string(&payload)?);
    }
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct Changelog {
    pub(crate) version: String,
    pub(crate) url: String,
    pub(crate) body: String,
    /// Where `body` came from: `release-file` (releases/vX.Y.Z.md from the
    /// repo), `github` (Releases API), `embedded` (offline fallback) or
    /// `unavailable` (neither had notes for this version).
    pub(crate) source: String,
}

/// Extract the `## [version]` section from a Keep-a-Changelog document.
pub(crate) fn extract_section(markdown: &str, version: &str) -> Option<String> {
    let heading_bracket = format!("## [{version}]");
    let heading_plain = format!("## {version}");
    let mut lines = markdown.lines();
    loop {
        let line = lines.next()?;
        let stripped = line.trim();
        if stripped == heading_bracket || stripped == heading_plain {
            break;
        }
    }
    let mut body: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim_start().starts_with("## ") {
            break;
        }
        body.push(line);
    }
    let text = body.join("\n").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Parse the `body` field out of a GitHub release JSON payload.
pub(crate) fn parse_github_body(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let body = value.get("body")?.as_str()?.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("usage-monitor-cli/{VERSION}"))
        .build()
        .ok()
}

async fn fetch_release_file(version: &str) -> Option<String> {
    let client = http_client()?;
    // The version tag first (exact file for that release), then main (release
    // files merged without a tag, e.g. unreleased versions under test).
    let refs = [
        format!("refs/tags/v{version}"),
        "refs/heads/main".to_string(),
    ];
    for git_ref in &refs {
        let response = client
            .get(release_file_url(version, git_ref))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            continue;
        }
        let body = response.text().await.ok()?.trim().to_string();
        if !body.is_empty() {
            return Some(body);
        }
    }
    None
}

async fn fetch_github_body(version: &str) -> Option<String> {
    let url = github_release_url(version);
    let client = http_client()?;
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    parse_github_body(&text)
}

pub(crate) async fn changelog_for(version: &str) -> Changelog {
    let version = version.trim().trim_start_matches(['v', 'V']).to_string();
    if let Some(body) = fetch_release_file(&version).await {
        return Changelog {
            version: version.clone(),
            url: release_url(&version),
            body,
            source: "release-file".to_string(),
        };
    }
    if let Some(body) = fetch_github_body(&version).await {
        return Changelog {
            version: version.clone(),
            url: release_url(&version),
            body,
            source: "github".to_string(),
        };
    }
    if let Some(body) = extract_section(CHANGELOG_MD, &version) {
        return Changelog {
            version: version.clone(),
            url: release_url(&version),
            body,
            source: "embedded".to_string(),
        };
    }
    Changelog {
        version: version.clone(),
        url: release_url(&version),
        body: String::new(),
        source: "unavailable".to_string(),
    }
}

pub(crate) async fn run_changelog(version: &str, pretty: bool) -> Result<()> {
    let payload = changelog_for(version).await;
    if pretty {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", serde_json::to_string(&payload)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn embedded_changelog_matches_workspace() {
        // The packaged copy must mirror the workspace CHANGELOG (re-sync it
        // on every release); otherwise the offline fallback drifts.
        const WORKSPACE_MD: &str = include_str!("../../../CHANGELOG.md");
        assert_eq!(CHANGELOG_MD, WORKSPACE_MD);
    }

    #[test]
    fn parse_version_tolerates_prefixes_and_suffixes() {
        assert_eq!(parse_version("0.8.1"), vec![0, 8, 1]);
        assert_eq!(parse_version("v0.8.1"), vec![0, 8, 1]);
        assert_eq!(parse_version("  1.2 "), vec![1, 2]);
        assert_eq!(parse_version("0.8.1-rc.1"), vec![0, 8, 1, 1]);
    }

    #[test]
    fn compare_versions_pads_shorter_side() {
        assert_eq!(compare_versions("0.8.1", "0.8.1"), Ordering::Equal);
        assert_eq!(compare_versions("0.8.0", "0.8.1"), Ordering::Less);
        assert_eq!(compare_versions("0.9", "0.8.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.8", "0.8.0"), Ordering::Equal);
        assert_eq!(compare_versions("v0.7.0", "0.8.1"), Ordering::Less);
    }

    #[test]
    fn is_outdated_only_for_older_stamps() {
        assert!(
            !is_outdated(None, "0.8.1"),
            "never installed: nothing to update"
        );
        assert!(!is_outdated(Some("0.8.1"), "0.8.1"));
        assert!(is_outdated(Some("0.8.0"), "0.8.1"));
        assert!(
            !is_outdated(Some("0.9.0"), "0.8.1"),
            "downgraded CLI is not an update"
        );
    }

    #[test]
    fn extract_section_stops_at_next_heading() {
        let md = "# Changelog\n\n## [0.8.1]\n\n### Added\n- x\n\n## [0.8.0]\n\n- y\n";
        let section = extract_section(md, "0.8.1").unwrap();
        assert!(section.contains("- x"));
        assert!(!section.contains("- y"));
        assert!(extract_section(md, "9.9.9").is_none());
    }

    #[test]
    fn extract_section_accepts_plain_heading() {
        let md = "## 0.8.1\n\nNotes here\n\n## 0.8.0\n";
        assert_eq!(extract_section(md, "0.8.1").as_deref(), Some("Notes here"));
    }

    #[test]
    fn embedded_changelog_covers_current_version() {
        // The offline fallback only works when CHANGELOG.md has a section for
        // the binary version; a release that forgets it degrades to
        // `unavailable` instead of showing the wrong notes.
        assert!(
            extract_section(CHANGELOG_MD, VERSION).is_some(),
            "CHANGELOG.md must contain a section for the current version {VERSION}"
        );
    }

    #[test]
    fn parse_github_body_reads_body_field() {
        let raw = r#"{"tag_name":"v0.8.1","body":"Notes here"}"#;
        assert_eq!(parse_github_body(raw).as_deref(), Some("Notes here"));
        assert!(parse_github_body(r#"{"body":""}"#).is_none());
        assert!(parse_github_body("not json").is_none());
    }

    #[test]
    fn release_url_points_at_tag_page() {
        assert_eq!(
            release_url("0.8.1"),
            "https://github.com/prodesert22/UsageMonitor/releases/tag/v0.8.1"
        );
    }

    #[test]
    fn github_release_url_points_at_api() {
        // Regression: the `/repos/` segment was missing once, so every fetch
        // 404'd and the changelog silently fell back to `embedded`.
        assert_eq!(
            github_release_url("0.8.1"),
            "https://api.github.com/repos/prodesert22/UsageMonitor/releases/tags/v0.8.1"
        );
    }

    #[test]
    fn release_file_url_points_at_raw() {
        assert_eq!(
            release_file_url("0.8.1", "refs/tags/v0.8.1"),
            "https://raw.githubusercontent.com/prodesert22/UsageMonitor/refs/tags/v0.8.1/releases/v0.8.1.md"
        );
        assert_eq!(
            release_file_url("0.8.1", "refs/heads/main"),
            "https://raw.githubusercontent.com/prodesert22/UsageMonitor/refs/heads/main/releases/v0.8.1.md"
        );
    }

    fn with_temp_data_home<R>(f: impl FnOnce() -> R) -> R {
        // Same process-global lock as install.rs tests: HOME/XDG_* mutation
        // races across parallel test threads otherwise.
        let _guard = super::super::install::test_env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let keys = ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"];
        let saved: Vec<(String, Option<std::ffi::OsString>)> = keys
            .iter()
            .map(|k| (k.to_string(), std::env::var_os(k)))
            .collect();
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_DATA_HOME", tmp.path().join("share"));
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        let result = f();
        unsafe {
            for (key, value) in saved {
                match value {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
        result
    }

    #[test]
    fn check_updates_flags_only_stale_stamps() {
        with_temp_data_home(|| {
            // Nothing installed: no update pending anywhere.
            let all = check_updates(None).unwrap();
            assert_eq!(all.len(), 2);
            assert!(all.iter().all(|u| u.installed.is_none() && !u.outdated));

            // A stale kde stamp is outdated; waybar stays clean.
            let stamp = super::super::install::stamp_path("kde").unwrap();
            std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
            std::fs::write(&stamp, "0.0.1").unwrap();

            let kde = check_updates(Some(crate::cli::WidgetInstallTarget::Kde)).unwrap();
            assert_eq!(kde.len(), 1);
            assert_eq!(kde[0].installed.as_deref(), Some("0.0.1"));
            assert!(kde[0].outdated);

            let all = check_updates(None).unwrap();
            let waybar = all.iter().find(|u| u.target == "waybar").unwrap();
            assert!(!waybar.outdated);
        });
    }
}
