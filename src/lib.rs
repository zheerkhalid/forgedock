//! ForgeDock: an app manager for the Forge family of Windows apps.
//!
//! The library holds everything; `main.rs` is a thin launcher.

pub mod actions;
pub mod catalog;
pub mod demo;
pub mod github;
pub mod installed;
pub mod net;
pub mod settings;
pub mod ui;
pub mod version;

/// Where the published catalogue lives.
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/zheerkhalid/forgedock/main/catalog.json";

/// The catalogue as it was when this build was made (the offline fallback).
pub const EMBEDDED_CATALOG: &str = include_str!("../catalog.json");

/// Icons of the apps known at build time, so the window looks right offline.
pub fn embedded_icon(id: &str) -> Option<&'static [u8]> {
    macro_rules! icons {
        ($($name:literal),*) => {
            match id {
                $($name => Some(&include_bytes!(concat!("../assets/icons/", $name, "-icon.png"))[..]),)*
                _ => None,
            }
        };
    }
    icons!(
        "snapforge",
        "reelforge",
        "unforge",
        "keepforge",
        "shredforge",
        "bootforge",
        "wireforge",
        "twinforge",
        "rescueforge",
        "spaceforge",
        "findforge",
        "pulseforge",
        "veilforge",
        "socketforge",
        "hourforge"
    )
}

/// Seconds since the Unix epoch.
pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
