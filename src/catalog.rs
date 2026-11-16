//! The app catalogue (`catalog.json`): what exists, how to describe it and
//! how to find its downloads. New apps are added by editing that file only.

use serde::{Deserialize, Serialize};

pub const OWNER: &str = "zheerkhalid";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Available,
    ComingSoon,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Specs {
    pub platform: String,
    pub language: String,
    pub licence: String,
    pub needs_admin: String,
    pub size: String,
}

/// File name patterns of the release assets; `*` matches anything.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Assets {
    pub setup: String,
    pub portable: String,
    pub exe: String,
}

/// How an installed copy is recognised in the uninstall registry keys.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Detect {
    /// The Inno Setup AppId, with braces: the key is `<AppId>_is1`.
    pub inno_app_id: String,
    /// The start of DisplayName.
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub category: String,
    pub status: Status,
    #[serde(default)]
    pub description: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub specs: Specs,
    /// `owner/name` on GitHub; empty for apps that are not published yet.
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub accent: Vec<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub assets: Assets,
    #[serde(default)]
    pub detect: Detect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    #[serde(default)]
    pub publisher: String,
    pub apps: Vec<App>,
}

fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !s.starts_with('.')
}

impl Catalog {
    /// Parses and checks a catalogue. Entries the app cannot handle safely
    /// make the whole file invalid, so a damaged download falls back to the
    /// cached or built-in copy.
    pub fn parse(text: &str) -> Result<Catalog, String> {
        let cat: Catalog = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if cat.schema != 1 {
            return Err(format!("unsupported catalogue version {}", cat.schema));
        }
        if cat.apps.is_empty() {
            return Err("the catalogue is empty".into());
        }
        let mut seen = std::collections::HashSet::new();
        for app in &cat.apps {
            if !safe_name(&app.id) || app.name.trim().is_empty() {
                return Err(format!("bad app id or name: {:?}", app.id));
            }
            if !seen.insert(app.id.as_str()) {
                return Err(format!("duplicate app id {}", app.id));
            }
            if app.status == Status::Available && app.repo_name().is_none() {
                return Err(format!("{}: repo must be {OWNER}/<name>", app.id));
            }
        }
        Ok(cat)
    }

    pub fn embedded() -> Catalog {
        Catalog::parse(crate::EMBEDDED_CATALOG).expect("the built-in catalogue is valid")
    }

    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for app in &self.apps {
            if !app.category.is_empty() && !out.contains(&app.category) {
                out.push(app.category.clone());
            }
        }
        out.sort();
        out
    }
}

impl App {
    /// The repository name, only when the repo belongs to the publisher.
    pub fn repo_name(&self) -> Option<&str> {
        let (owner, name) = self.repo.split_once('/')?;
        (owner == OWNER && safe_name(name)).then_some(name)
    }

    pub fn available(&self) -> bool {
        self.status == Status::Available
    }

    pub fn repo_url(&self) -> Option<String> {
        self.repo_name()
            .map(|n| format!("https://github.com/{OWNER}/{n}"))
    }

    /// The two gradient colours of the app's icon.
    pub fn accents(&self) -> ([u8; 3], [u8; 3]) {
        let a = self
            .accent
            .first()
            .and_then(|s| parse_hex(s))
            .unwrap_or([0x4f, 0x46, 0xe5]);
        let b = self.accent.get(1).and_then(|s| parse_hex(s)).unwrap_or(a);
        (a, b)
    }

    /// Search over name, tagline, category and description.
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = format!(
            "{} {} {} {} {}",
            self.name,
            self.tagline,
            self.category,
            self.description.join(" "),
            self.features.join(" ")
        )
        .to_lowercase();
        q.split_whitespace().all(|word| hay.contains(word))
    }
}

/// `#RRGGBB` -> bytes.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

/// Case-insensitive match where `*` stands for any run of characters.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid() {
        let cat = Catalog::embedded();
        assert!(cat.apps.len() >= 15);
        let rescue = cat.apps.iter().find(|a| a.id == "rescueforge").unwrap();
        assert_eq!(rescue.status, Status::Available);
        let snap = cat.apps.iter().find(|a| a.id == "snapforge").unwrap();
        assert_eq!(snap.name, "SnapForge");
        assert_eq!(snap.repo_name(), Some("snapforge"));
        assert_eq!(snap.status, Status::Available);
        assert!(!snap.features.is_empty() && !snap.screenshots.is_empty());
        assert_eq!(snap.accents().0, [0x4f, 0x6b, 0xff]);
        assert_eq!(
            cat.apps.iter().filter(|a| a.available()).count(),
            15,
            "fifteen published apps"
        );
        for app in cat.apps.iter().filter(|a| a.available()) {
            assert!(crate::net::is_allowed_fetch(&app.icon), "{}", app.icon);
            for shot in &app.screenshots {
                assert!(crate::net::is_allowed_fetch(shot), "{shot}");
            }
            assert!(app.detect.inno_app_id.starts_with('{'));
        }
    }

    #[test]
    fn parses_a_minimal_entry_and_ignores_unknown_fields() {
        let text = r#"{"schema":1,"apps":[{"id":"newforge","name":"NewForge",
            "status":"coming-soon","future_field":42}]}"#;
        let cat = Catalog::parse(text).unwrap();
        assert_eq!(cat.apps[0].status, Status::ComingSoon);
        assert!(cat.apps[0].features.is_empty());
    }

    #[test]
    fn rejects_bad_catalogs() {
        assert!(Catalog::parse("not json").is_err());
        assert!(Catalog::parse(r#"{"schema":2,"apps":[]}"#).is_err());
        assert!(Catalog::parse(r#"{"schema":1,"apps":[]}"#).is_err());
        // An available app must live in the publisher's account.
        let foreign = r#"{"schema":1,"apps":[{"id":"x","name":"X","status":"available",
            "repo":"someoneelse/x"}]}"#;
        assert!(Catalog::parse(foreign).is_err());
        let dup = r#"{"schema":1,"apps":[{"id":"x","name":"X","status":"coming-soon"},
            {"id":"x","name":"Y","status":"coming-soon"}]}"#;
        assert!(Catalog::parse(dup).is_err());
        let path = r#"{"schema":1,"apps":[{"id":"../x","name":"X","status":"coming-soon"}]}"#;
        assert!(Catalog::parse(path).is_err());
    }

    #[test]
    fn glob() {
        assert!(glob_match(
            "SnapForge-*-portable-x64.zip",
            "SnapForge-0.1.2-portable-x64.zip"
        ));
        assert!(glob_match("SnapForge-Setup.exe", "snapforge-setup.exe"));
        assert!(!glob_match("SnapForge-Setup.exe", "SnapForge.exe"));
        assert!(!glob_match("SnapForge.exe", "SnapForge-Setup.exe"));
        assert!(!glob_match(
            "SnapForge-*-portable-x64.zip",
            "SnapForge-0.1.2-portable-arm64.zip"
        ));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn search() {
        let cat = Catalog::embedded();
        let hits: Vec<_> = cat
            .apps
            .iter()
            .filter(|a| a.matches_query("password"))
            .map(|a| a.id.as_str())
            .collect();
        assert!(hits.contains(&"keepforge"));
        assert!(!hits.contains(&"reelforge"));
        assert!(cat.apps[0].matches_query("  "));
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#FF8A3D"), Some([0xff, 0x8a, 0x3d]));
        assert_eq!(parse_hex("FF8A3D"), None);
        assert_eq!(parse_hex("#GG0000"), None);
    }
}
