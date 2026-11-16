//! User settings (`%LOCALAPPDATA%\ForgeDock\settings.json`) and the folders
//! ForgeDock uses.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    Dark,
    Light,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// Fetch the catalogue and the latest versions when ForgeDock starts.
    pub check_at_start: bool,
    /// Where portable ZIPs are saved; `None` means the Downloads folder.
    pub download_dir: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::Dark,
            check_at_start: true,
            download_dir: None,
        }
    }
}

/// The folders ForgeDock reads and writes.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// `%LOCALAPPDATA%\ForgeDock`
    pub root: PathBuf,
    /// The user's Downloads folder.
    pub downloads: PathBuf,
}

impl Dirs {
    pub fn real() -> Dirs {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Dirs {
            root: local.join("ForgeDock"),
            downloads: profile.join("Downloads"),
        }
    }

    /// Invented folders for demo mode; nothing is read from or written to them.
    pub fn demo() -> Dirs {
        Dirs {
            root: PathBuf::from("C:\\Users\\Alex\\AppData\\Local\\ForgeDock"),
            downloads: PathBuf::from("C:\\Users\\Alex\\Downloads"),
        }
    }

    /// Catalogue, release answers, icons and screenshots.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Downloaded installers.
    pub fn installers(&self) -> PathBuf {
        self.root.join("downloads")
    }
}

impl Settings {
    pub fn load(dirs: &Dirs) -> Settings {
        std::fs::read_to_string(dirs.root.join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dirs: &Dirs) -> Result<(), String> {
        std::fs::create_dir_all(&dirs.root).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dirs.root.join("settings.json"), json)
            .map_err(|e| format!("Could not save settings: {e}"))
    }

    pub fn portable_dir(&self, dirs: &Dirs) -> PathBuf {
        match &self.download_dir {
            Some(d) if !d.trim().is_empty() => PathBuf::from(d),
            _ => dirs.downloads.clone(),
        }
    }
}

/// Total size of the files under `dir`.
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(m) if m.is_dir() => total += dir_size(&path),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let s = Settings::default();
        assert!(s.check_at_start);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // Unknown or missing fields fall back to defaults.
        let partial: Settings = serde_json::from_str(r#"{"theme":"Light"}"#).unwrap();
        assert_eq!(partial.theme, ThemeChoice::Light);
        assert!(partial.check_at_start);
    }

    #[test]
    fn portable_folder() {
        let dirs = Dirs::demo();
        let mut s = Settings::default();
        assert_eq!(s.portable_dir(&dirs), dirs.downloads);
        s.download_dir = Some("D:\\Tools".into());
        assert_eq!(s.portable_dir(&dirs), PathBuf::from("D:\\Tools"));
    }
}
