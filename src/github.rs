//! Latest-release lookups on the GitHub API, with an on-disk cache so the
//! unauthenticated limit (60 requests an hour) is never a problem in practice.

use crate::catalog::{glob_match, App, OWNER};
use crate::net::{self, NetError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How long a cached answer is used without asking again.
pub const CACHE_SECONDS: u64 = 3600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
    /// Lower-case hex SHA-256, when the API reports a digest.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    /// `YYYY-MM-DD...` as given by the API.
    pub published: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Setup,
    Portable,
    Exe,
}

impl Release {
    pub fn version(&self) -> String {
        crate::version::display(&self.tag)
    }

    /// The asset matching the app's file name pattern for `kind`.
    pub fn asset(&self, app: &App, kind: AssetKind) -> Option<&Asset> {
        let pattern = match kind {
            AssetKind::Setup => &app.assets.setup,
            AssetKind::Portable => &app.assets.portable,
            AssetKind::Exe => &app.assets.exe,
        };
        if pattern.is_empty() {
            return None;
        }
        self.assets.iter().find(|a| glob_match(pattern, &a.name))
    }

    /// `2026-11-15T11:54:12Z` -> `15 Nov 2026`.
    pub fn date(&self) -> String {
        format_date(&self.published)
    }
}

pub fn format_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let parts: Vec<&str> = iso.get(..10).unwrap_or("").split('-').collect();
    if let [y, m, d] = parts[..] {
        if let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) {
            if (1..=12).contains(&m) && y.len() == 4 {
                return format!("{d} {} {y}", MONTHS[m - 1]);
            }
        }
    }
    String::new()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// No connection (or TLS / timeout trouble).
    Offline(String),
    /// The hourly limit was reached; `reset` is the epoch second it lifts.
    RateLimited {
        reset: Option<u64>,
    },
    /// The repository has no published release.
    NoRelease,
    Http(u32),
    BadAnswer(String),
}

impl ApiError {
    pub fn message(&self, now: u64) -> String {
        match self {
            ApiError::Offline(_) => {
                "You appear to be offline. Showing saved information.".to_string()
            }
            ApiError::RateLimited { reset } => {
                let wait = reset
                    .map(|r| r.saturating_sub(now).div_ceil(60).max(1))
                    .map(|m| format!(" Try again in about {m} min."))
                    .unwrap_or_default();
                format!(
                    "GitHub's hourly request limit was reached. Showing saved information.{wait}"
                )
            }
            ApiError::NoRelease => "No release has been published yet.".to_string(),
            ApiError::Http(s) => format!("GitHub answered with status {s}."),
            ApiError::BadAnswer(e) => {
                format!("GitHub sent an answer ForgeDock could not read ({e}).")
            }
        }
    }
}

/// Turns a status code and the rate-limit headers into a verdict.
pub fn classify(status: u32, remaining: Option<u64>, reset: Option<u64>) -> Result<(), ApiError> {
    match status {
        200 => Ok(()),
        429 => Err(ApiError::RateLimited { reset }),
        403 if remaining == Some(0) => Err(ApiError::RateLimited { reset }),
        404 => Err(ApiError::NoRelease),
        other => Err(ApiError::Http(other)),
    }
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

fn simple_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Reads a `releases/latest` answer. Assets that do not live under the
/// repository's own release downloads are dropped.
pub fn parse_release(json: &str, repo: &str) -> Result<Release, ApiError> {
    let raw: RawRelease =
        serde_json::from_str(json).map_err(|e| ApiError::BadAnswer(e.to_string()))?;
    let assets = raw
        .assets
        .into_iter()
        .filter(|a| {
            simple_file_name(&a.name)
                && net::is_release_asset_url(&a.browser_download_url, Some(repo))
        })
        .map(|a| Asset {
            sha256: a
                .digest
                .as_deref()
                .and_then(|d| d.strip_prefix("sha256:"))
                .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
                .map(|h| h.to_ascii_lowercase()),
            name: a.name,
            size: a.size,
            url: a.browser_download_url,
        })
        .collect();
    let page = raw
        .html_url
        .filter(|u| u.starts_with(&format!("https://github.com/{OWNER}/")))
        .unwrap_or_else(|| format!("https://github.com/{OWNER}/{repo}/releases/latest"));
    Ok(Release {
        tag: raw.tag_name,
        published: raw.published_at.unwrap_or_default(),
        notes: raw.body.unwrap_or_default(),
        page,
        assets,
    })
}

/// Asks the API for the latest release of `zheerkhalid/<repo>`.
pub fn fetch_latest(repo: &str) -> Result<Release, ApiError> {
    let url = format!("https://api.github.com/repos/{OWNER}/{repo}/releases/latest");
    let headers = [
        "Accept: application/vnd.github+json",
        "X-GitHub-Api-Version: 2022-11-28",
    ];
    let resp = net::get(&url, &headers, 2 << 20).map_err(|e| match e {
        NetError::Unreachable(m) => ApiError::Offline(m),
        other => ApiError::BadAnswer(other.to_string()),
    })?;
    classify(resp.status, resp.rate_remaining, resp.rate_reset)?;
    let text = String::from_utf8_lossy(&resp.body);
    parse_release(&text, repo)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cached {
    pub fetched_at: u64,
    pub release: Release,
}

impl Cached {
    pub fn is_fresh(&self, now: u64, max_age: u64) -> bool {
        now >= self.fetched_at && now - self.fetched_at < max_age
    }
}

fn cache_file(cache_dir: &Path, repo: &str) -> PathBuf {
    cache_dir.join("releases").join(format!("{repo}.json"))
}

pub fn load_cached(cache_dir: &Path, repo: &str) -> Option<Cached> {
    let text = std::fs::read_to_string(cache_file(cache_dir, repo)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn store_cached(cache_dir: &Path, repo: &str, cached: &Cached) {
    let file = cache_file(cache_dir, repo);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(cached) {
        let _ = std::fs::write(file, json);
    }
}

/// Turns release notes (GitHub Markdown) into plain lines for display:
/// `(is_heading, is_bullet, text)`.
pub fn note_lines(notes: &str) -> Vec<(bool, bool, String)> {
    let mut out = Vec::new();
    for line in notes.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let (heading, bullet, body) = if let Some(h) = t.strip_prefix('#') {
            (true, false, h.trim_start_matches('#').trim())
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            (false, true, b.trim())
        } else {
            (false, false, t)
        };
        let mut text = body.replace("**", "").replace('`', "");
        // [label](address) -> label
        while let (Some(a), Some(b)) = (text.find('['), text.find("](")) {
            let Some(c) = text[b..].find(')') else { break };
            if a > b {
                break;
            }
            let label = text[a + 1..b].to_string();
            text.replace_range(a..b + c + 1, &label);
        }
        let heading = heading || (t.starts_with("**") && t.ends_with("**") && !bullet);
        out.push((heading, bullet, text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    const SAMPLE: &str = r#"{
      "tag_name": "v0.1.2", "name": "SnapForge 0.1.2",
      "published_at": "2026-11-15T11:54:12Z",
      "html_url": "https://github.com/zheerkhalid/snapforge/releases/tag/v0.1.2",
      "body": "**Easier sizing**\n\n- **Size slider** in the `tool bar`\n- See [docs](https://example.com/x)",
      "assets": [
        {"name": "SnapForge-0.1.2-portable-x64.zip", "size": 5574733,
         "digest": "sha256:6E167EB97C466E7EBAB4A0E7E2E636F38BE56FECE201AAFE56BFEE3D42B7F38A",
         "browser_download_url": "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge-0.1.2-portable-x64.zip"},
        {"name": "SnapForge-Setup.exe", "size": 6133367, "digest": null,
         "browser_download_url": "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge-Setup.exe"},
        {"name": "SnapForge.exe", "size": 12111360,
         "browser_download_url": "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge.exe"},
        {"name": "Evil-Setup.exe", "size": 1,
         "browser_download_url": "https://evil.example/zheerkhalid/snapforge/releases/download/v0.1.2/Evil-Setup.exe"},
        {"name": "..\\up.exe", "size": 1,
         "browser_download_url": "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/up.exe"}
      ]
    }"#;

    #[test]
    fn parses_a_release_and_matches_assets() {
        let rel = parse_release(SAMPLE, "snapforge").unwrap();
        assert_eq!(rel.version(), "0.1.2");
        assert_eq!(rel.date(), "15 Nov 2026");
        assert_eq!(rel.assets.len(), 3, "foreign and odd-named assets dropped");
        let cat = Catalog::embedded();
        let app = cat.apps.iter().find(|a| a.id == "snapforge").unwrap();
        let setup = rel.asset(app, AssetKind::Setup).unwrap();
        assert_eq!(setup.name, "SnapForge-Setup.exe");
        assert_eq!(setup.size, 6133367);
        assert_eq!(setup.sha256, None);
        let zip = rel.asset(app, AssetKind::Portable).unwrap();
        assert_eq!(zip.name, "SnapForge-0.1.2-portable-x64.zip");
        assert_eq!(
            zip.sha256.as_deref(),
            Some("6e167eb97c466e7ebab4a0e7e2e636f38be56fece201aafe56bfee3d42b7f38a")
        );
        assert_eq!(
            rel.asset(app, AssetKind::Exe).unwrap().name,
            "SnapForge.exe"
        );
        // Another app's patterns find nothing in this release.
        let other = cat.apps.iter().find(|a| a.id == "unforge").unwrap();
        assert!(rel.asset(other, AssetKind::Setup).is_none());
    }

    #[test]
    fn assets_of_another_repo_are_dropped() {
        let rel = parse_release(SAMPLE, "unforge").unwrap();
        assert!(rel.assets.is_empty());
        assert!(parse_release("[]", "snapforge").is_err());
    }

    #[test]
    fn rate_limit_handling() {
        assert_eq!(classify(200, Some(59), Some(1)), Ok(()));
        assert_eq!(
            classify(403, Some(0), Some(1_800_000_600)),
            Err(ApiError::RateLimited {
                reset: Some(1_800_000_600)
            })
        );
        assert_eq!(
            classify(429, None, None),
            Err(ApiError::RateLimited { reset: None })
        );
        // A 403 with requests left is not a rate limit.
        assert_eq!(classify(403, Some(12), None), Err(ApiError::Http(403)));
        assert_eq!(classify(404, Some(50), None), Err(ApiError::NoRelease));
        assert_eq!(classify(500, None, None), Err(ApiError::Http(500)));
        let msg = ApiError::RateLimited {
            reset: Some(1_800_000_600),
        }
        .message(1_800_000_000);
        assert!(msg.contains("about 10 min"), "{msg}");
        assert!(ApiError::RateLimited { reset: None }
            .message(0)
            .contains("hourly request limit"));
    }

    #[test]
    fn cache_freshness() {
        let rel = parse_release(SAMPLE, "snapforge").unwrap();
        let c = Cached {
            fetched_at: 1000,
            release: rel,
        };
        assert!(c.is_fresh(1000, CACHE_SECONDS));
        assert!(c.is_fresh(1000 + 3599, CACHE_SECONDS));
        assert!(!c.is_fresh(1000 + 3600, CACHE_SECONDS));
        // A clock set back does not make an entry immortal.
        assert!(!c.is_fresh(10, CACHE_SECONDS));
        let dir = std::env::temp_dir().join(format!("forgedock-test-{}", std::process::id()));
        store_cached(&dir, "snapforge", &c);
        assert_eq!(load_cached(&dir, "snapforge"), Some(c));
        assert_eq!(load_cached(&dir, "missing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_to_lines() {
        let rel = parse_release(SAMPLE, "snapforge").unwrap();
        let lines = note_lines(&rel.notes);
        assert_eq!(lines[0], (true, false, "Easier sizing".to_string()));
        assert_eq!(
            lines[1],
            (false, true, "Size slider in the tool bar".to_string())
        );
        assert_eq!(lines[2], (false, true, "See docs".to_string()));
    }

    #[test]
    fn dates() {
        assert_eq!(format_date("2026-01-05T00:00:00Z"), "5 Jan 2026");
        assert_eq!(format_date("garbage"), "");
        assert_eq!(format_date(""), "");
    }
}
