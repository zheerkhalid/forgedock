//! What ForgeDock does on the user's behalf: download a release file, check
//! it, start an installer, an uninstaller or an app, open a page.

use crate::github::Asset;
use crate::net::{self, Downloaded};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

/// A program to start through the shell (so elevation prompts work).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub file: String,
    pub params: String,
}

/// Checks a finished download against what the API promised.
pub fn verify(got: &Downloaded, asset: &Asset) -> Result<(), String> {
    if got.size != asset.size {
        return Err(format!(
            "The download is incomplete or altered: {} bytes arrived, {} were expected.",
            got.size, asset.size
        ));
    }
    if let Some(want) = &asset.sha256 {
        if !want.eq_ignore_ascii_case(&got.sha256) {
            return Err(
                "The download does not match the checksum published with the release. It was deleted."
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Downloads `asset` of `repo` into `dir` and verifies size and checksum.
/// A file that fails the check is deleted.
pub fn download_asset(
    asset: &Asset,
    repo: &str,
    dir: &Path,
    progress: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    if asset.name.contains(['\\', '/', ':']) || asset.name.starts_with('.') {
        return Err("The release file has an unexpected name.".to_string());
    }
    let dest = dir.join(&asset.name);
    // A little slack over the announced size; the exact size is checked after.
    let cap = asset.size.saturating_add(1024);
    let got =
        net::download(&asset.url, repo, &dest, cap, progress, cancel).map_err(|e| e.to_string())?;
    if let Err(e) = verify(&got, asset) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(dest)
}

/// The command that starts a downloaded installer. The user completes the
/// installer in its own window; nothing is run silently.
pub fn installer_launch(path: &Path) -> Result<Launch, String> {
    let name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !name.ends_with("-setup.exe") || !path.is_absolute() {
        return Err(format!("Not an installer: {}", path.display()));
    }
    Ok(Launch {
        file: path.display().to_string(),
        params: String::new(),
    })
}

/// Pages ForgeDock opens in the browser: the publisher's GitHub and site.
pub fn is_allowed_page(url: &str) -> bool {
    let Some((host, path)) = net::parse_https(url) else {
        return false;
    };
    match host.as_str() {
        "github.com" => {
            path == format!("/{}", crate::catalog::OWNER)
                || path.starts_with(&format!("/{}/", crate::catalog::OWNER))
        }
        "zheer.dev" => true,
        _ => false,
    }
}

fn shell_execute(file: &str, params: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let file_w = HSTRING::from(file);
    let params_w = HSTRING::from(params);
    // SAFETY: the strings outlive the call.
    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("open"),
            &file_w,
            if params.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params_w.as_ptr())
            },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Windows could not start {file}."))
    }
}

/// Starts a program.
pub fn run(launch: &Launch) -> Result<(), String> {
    if !Path::new(&launch.file).is_file() {
        return Err(format!("{} was not found.", launch.file));
    }
    shell_execute(&launch.file, &launch.params)
}

/// Opens one of the publisher's pages in the default browser.
pub fn open_page(url: &str) -> Result<(), String> {
    if !is_allowed_page(url) {
        return Err("That address is not one ForgeDock opens.".to_string());
    }
    shell_execute(url, "")
}

/// Opens a folder in Explorer.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} does not exist yet.", dir.display()));
    }
    shell_execute(&dir.display().to_string(), "")
}

/// `6133367` -> `5.8 MB`.
pub fn human_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(size: u64, sha: Option<&str>) -> Asset {
        Asset {
            name: "X-Setup.exe".into(),
            size,
            url: "https://github.com/zheerkhalid/x/releases/download/v1/X-Setup.exe".into(),
            sha256: sha.map(str::to_string),
        }
    }

    #[test]
    fn verifies_size_and_checksum() {
        let got = Downloaded {
            size: 10,
            sha256: "ab".repeat(32),
        };
        assert!(verify(&got, &asset(10, None)).is_ok());
        assert!(verify(&got, &asset(10, Some(&"AB".repeat(32)))).is_ok());
        assert!(verify(&got, &asset(11, None)).is_err());
        assert!(verify(&got, &asset(10, Some(&"cd".repeat(32)))).is_err());
    }

    #[test]
    fn installer_command_construction() {
        let ok = installer_launch(Path::new("C:\\Temp\\dl\\SnapForge-Setup.exe")).unwrap();
        assert_eq!(ok.file, "C:\\Temp\\dl\\SnapForge-Setup.exe");
        assert_eq!(
            ok.params, "",
            "never silent: the user completes the installer"
        );
        assert!(installer_launch(Path::new("C:\\Temp\\dl\\SnapForge.exe")).is_err());
        assert!(installer_launch(Path::new("C:\\Temp\\dl\\notes.zip")).is_err());
        assert!(installer_launch(Path::new("SnapForge-Setup.exe")).is_err());
    }

    #[test]
    fn odd_asset_names_are_refused_before_any_request() {
        let mut a = asset(1, None);
        a.name = "..\\evil.exe".into();
        let r = download_asset(
            &a,
            "x",
            &std::env::temp_dir(),
            &AtomicU64::new(0),
            &AtomicBool::new(false),
        );
        assert!(r.is_err());
    }

    #[test]
    fn pages() {
        assert!(is_allowed_page("https://github.com/zheerkhalid/snapforge"));
        assert!(is_allowed_page(
            "https://github.com/zheerkhalid/snapforge/releases/tag/v0.1.2"
        ));
        assert!(is_allowed_page("https://github.com/zheerkhalid"));
        assert!(is_allowed_page("https://zheer.dev"));
        assert!(!is_allowed_page("https://github.com/zheerkhalidx/a"));
        assert!(!is_allowed_page("https://example.com/zheerkhalid/"));
        assert!(!is_allowed_page("http://zheer.dev"));
        assert!(!is_allowed_page("file:///C:/x.exe"));
        assert!(!is_allowed_page("C:\\Windows\\System32\\calc.exe"));
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(6133367), "5.8 MB");
        assert_eq!(human_size(2048), "2 KB");
    }
}
