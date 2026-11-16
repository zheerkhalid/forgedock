//! Demo mode: the built-in catalogue, a snapshot of release information taken
//! when this build was made, and an invented set of installed apps. Nothing
//! is read from the network or the registry and nothing on the PC changes.

use crate::github::{parse_release, Release};
use crate::installed::RegEntry;
use std::collections::HashMap;

const RELEASES: &str = include_str!("../assets/demo-releases.json");

/// Release information by repository name.
pub fn releases() -> HashMap<String, Release> {
    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(RELEASES).unwrap_or_default();
    raw.into_iter()
        .filter_map(|(repo, value)| {
            parse_release(&value.to_string(), &repo)
                .ok()
                .map(|r| (repo, r))
        })
        .collect()
}

/// Invented uninstall entries: five apps installed, two of them out of date.
pub fn registry() -> Vec<RegEntry> {
    let entry = |name: &str, version: &str, per_user: bool| {
        let folder = if per_user {
            format!("C:\\Users\\Alex\\AppData\\Local\\Programs\\{name}")
        } else {
            format!("C:\\Program Files\\{name}")
        };
        RegEntry {
            key_name: format!("{{DEMO-{name}}}_is1"),
            display_name: format!("{name} {version}"),
            display_version: version.to_string(),
            uninstall_string: format!("\"{folder}\\unins000.exe\""),
            install_location: format!("{folder}\\"),
            per_user,
        }
    };
    vec![
        entry("SnapForge", "0.1.1", true),
        entry("UnForge", "1.0.0", false),
        entry("KeepForge", "1.0.0", true),
        entry("WireForge", "1.0.0", true),
        entry("TwinForge", "1.0.0", true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::github::AssetKind;

    #[test]
    fn demo_data_covers_every_published_app() {
        let cat = Catalog::embedded();
        let rel = releases();
        for app in cat.apps.iter().filter(|a| a.available()) {
            let r = rel.get(app.repo_name().unwrap()).expect("release");
            assert!(r.asset(app, AssetKind::Setup).is_some(), "{}", app.id);
            assert!(r.asset(app, AssetKind::Portable).is_some(), "{}", app.id);
            assert!(r.asset(app, AssetKind::Exe).is_some(), "{}", app.id);
        }
        let reg = registry();
        let installed = cat
            .apps
            .iter()
            .filter(|a| crate::installed::find(&reg, &a.detect).is_some())
            .count();
        assert_eq!(installed, 5);
    }
}
