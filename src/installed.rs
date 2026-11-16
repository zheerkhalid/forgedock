//! Finds installed copies of the catalogue's apps in the uninstall registry
//! keys. Every app in the family is installed by Inno Setup, whose key is
//! named `<AppId>_is1`.

use crate::actions::Launch;
use crate::catalog::Detect;
use std::path::PathBuf;

const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

/// One uninstall registry entry, as far as ForgeDock cares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegEntry {
    pub key_name: String,
    pub display_name: String,
    pub display_version: String,
    pub install_location: String,
    pub uninstall_string: String,
    pub per_user: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub location: String,
    pub uninstall_string: String,
    pub per_user: bool,
}

fn is_inno_key(key: &str) -> bool {
    key.to_ascii_lowercase().ends_with("_is1")
}

/// Exact AppId match (the reliable one).
fn matches_app_id(entry: &RegEntry, detect: &Detect) -> bool {
    !detect.inno_app_id.is_empty()
        && entry
            .key_name
            .eq_ignore_ascii_case(&format!("{}_is1", detect.inno_app_id))
}

/// DisplayName match for an Inno Setup entry: the name alone, or the name
/// followed by a version (`UnForge 1.0.0`, `UnForge version 1.0.0`).
fn matches_name(entry: &RegEntry, detect: &Detect) -> bool {
    let want = detect.display_name.trim().to_lowercase();
    if want.is_empty() || !is_inno_key(&entry.key_name) {
        return false;
    }
    let have = entry.display_name.trim().to_lowercase();
    match have.strip_prefix(&want) {
        Some("") => true,
        Some(rest) => {
            let rest = match rest.strip_prefix(' ') {
                Some(r) => r,
                None => return false,
            };
            let rest = rest.strip_prefix("version ").unwrap_or(rest);
            rest.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

pub fn matches(entry: &RegEntry, detect: &Detect) -> bool {
    matches_app_id(entry, detect) || matches_name(entry, detect)
}

/// The installed copy described by `detect`, preferring an AppId match.
pub fn find(entries: &[RegEntry], detect: &Detect) -> Option<Installed> {
    let entry = entries
        .iter()
        .find(|e| matches_app_id(e, detect))
        .or_else(|| entries.iter().find(|e| matches_name(e, detect)))?;
    Some(Installed {
        version: entry.display_version.clone(),
        location: entry.install_location.trim_end_matches('\\').to_string(),
        uninstall_string: entry.uninstall_string.clone(),
        per_user: entry.per_user,
    })
}

/// Reads the Inno Setup entries from HKCU and both views of HKLM.
pub fn scan() -> Vec<RegEntry> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut out = Vec::new();
    let roots = [
        (HKEY_CURRENT_USER, KEY_READ, true),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY, false),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY, false),
    ];
    for (root, access, per_user) in roots {
        let Ok(key) = RegKey::predef(root).open_subkey_with_flags(UNINSTALL, access) else {
            continue;
        };
        for name in key.enum_keys().flatten() {
            if !is_inno_key(&name) {
                continue;
            }
            let Ok(sub) = key.open_subkey_with_flags(&name, access) else {
                continue;
            };
            let get = |v: &str| sub.get_value::<String, _>(v).unwrap_or_default();
            let entry = RegEntry {
                display_name: get("DisplayName"),
                display_version: get("DisplayVersion"),
                install_location: get("InstallLocation"),
                uninstall_string: get("UninstallString"),
                key_name: name,
                per_user,
            };
            if !entry.display_name.is_empty() && !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

/// Splits a registry command line into the program and its arguments.
/// Handles `"C:\dir\unins000.exe" /x` and an unquoted path without arguments.
pub fn split_command(command: &str) -> Option<(String, String)> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"')?;
        let program = rest[..end].trim();
        (!program.is_empty()).then(|| (program.to_string(), rest[end + 1..].trim().to_string()))
    } else if c.is_empty() {
        None
    } else {
        match c.to_lowercase().find(".exe ") {
            Some(i) => Some((c[..i + 4].to_string(), c[i + 5..].trim().to_string())),
            None => Some((c.to_string(), String::new())),
        }
    }
}

/// The command that starts the registered uninstaller. Only an Inno Setup
/// uninstaller (`unins###.exe`) with an absolute path is accepted, and the
/// user completes it in its own window.
pub fn uninstall_launch(installed: &Installed) -> Result<Launch, String> {
    let (program, params) = split_command(&installed.uninstall_string)
        .ok_or_else(|| "No uninstaller is registered for this app.".to_string())?;
    let path = PathBuf::from(&program);
    let file = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or("");
    let numbered = stem
        .strip_prefix("unins")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if !numbered || !path.is_absolute() {
        return Err(format!(
            "The registered uninstaller is not the expected one: {program}"
        ));
    }
    Ok(Launch {
        file: program,
        params,
    })
}

/// The command that starts the installed app: `<install folder>\<Name>.exe`.
pub fn open_launch(installed: &Installed, exe_name: &str) -> Result<Launch, String> {
    let simple = !exe_name.is_empty()
        && exe_name.to_ascii_lowercase().ends_with(".exe")
        && !exe_name.contains(['\\', '/', ':', '*', '?', '"']);
    if installed.location.is_empty() || !simple {
        return Err("The install folder of this app is not known.".to_string());
    }
    let path = PathBuf::from(&installed.location).join(exe_name);
    Ok(Launch {
        file: path.display().to_string(),
        params: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect() -> Detect {
        Detect {
            inno_app_id: "{B3D1F6A2-4E57-4C0B-9A31-7D2E8C5F1A64}".into(),
            display_name: "UnForge".into(),
        }
    }

    fn entry(key: &str, name: &str, version: &str) -> RegEntry {
        RegEntry {
            key_name: key.into(),
            display_name: name.into(),
            display_version: version.into(),
            install_location: "C:\\Program Files\\UnForge\\".into(),
            uninstall_string: "\"C:\\Program Files\\UnForge\\unins000.exe\"".into(),
            per_user: false,
        }
    }

    #[test]
    fn matches_by_app_id() {
        let e = entry(
            "{b3d1f6a2-4e57-4c0b-9a31-7d2e8c5f1a64}_is1",
            "Renamed",
            "1.0.0",
        );
        assert!(matches(&e, &detect()));
    }

    #[test]
    fn matches_by_display_name_on_inno_keys_only() {
        let d = detect();
        assert!(matches(&entry("{OTHER}_is1", "UnForge 1.0.0", "1.0.0"), &d));
        assert!(matches(&entry("{OTHER}_is1", "UnForge", "1.0.0"), &d));
        assert!(matches(
            &entry("{OTHER}_is1", "unforge version 1.2", "1.2"),
            &d
        ));
        // Not an Inno Setup key.
        assert!(!matches(&entry("UnForge", "UnForge 1.0.0", "1.0.0"), &d));
        // Different products that merely share a prefix.
        assert!(!matches(&entry("{OTHER}_is1", "UnForgettable 2", "2"), &d));
        assert!(!matches(&entry("{OTHER}_is1", "UnForge Helper", "1"), &d));
        assert!(!matches(&entry("{OTHER}_is1", "My UnForge 1.0", "1"), &d));
        let empty = Detect::default();
        assert!(!matches(&entry("_is1", "", ""), &empty));
    }

    #[test]
    fn find_prefers_the_app_id() {
        let entries = vec![
            entry("{OTHER}_is1", "UnForge 0.9.0", "0.9.0"),
            entry(
                "{B3D1F6A2-4E57-4C0B-9A31-7D2E8C5F1A64}_is1",
                "UnForge 1.0.0",
                "1.0.0",
            ),
        ];
        let found = find(&entries, &detect()).unwrap();
        assert_eq!(found.version, "1.0.0");
        assert_eq!(found.location, "C:\\Program Files\\UnForge");
        assert!(find(&entries[..0], &detect()).is_none());
    }

    #[test]
    fn splits_commands() {
        assert_eq!(
            split_command("\"C:\\Program Files\\UnForge\\unins000.exe\" /SILENT"),
            Some((
                "C:\\Program Files\\UnForge\\unins000.exe".into(),
                "/SILENT".into()
            ))
        );
        assert_eq!(
            split_command("C:\\Apps\\X\\unins000.exe"),
            Some(("C:\\Apps\\X\\unins000.exe".into(), String::new()))
        );
        assert_eq!(
            split_command("C:\\Apps\\X\\unins000.exe /x"),
            Some(("C:\\Apps\\X\\unins000.exe".into(), "/x".into()))
        );
        assert_eq!(split_command("  "), None);
        assert_eq!(split_command("\"unterminated"), None);
    }

    #[test]
    fn uninstall_command_construction() {
        let mut inst = find(&[entry("{OTHER}_is1", "UnForge 1.0.0", "1.0.0")], &detect()).unwrap();
        assert_eq!(
            uninstall_launch(&inst),
            Ok(Launch {
                file: "C:\\Program Files\\UnForge\\unins000.exe".into(),
                params: String::new()
            })
        );
        // Anything that is not an Inno Setup uninstaller is refused.
        for bad in [
            "",
            "cmd.exe /c del x",
            "\"C:\\Windows\\System32\\cmd.exe\" /c rd /s /q C:\\",
            "unins000.exe",
            "\"C:\\Apps\\X\\uninstall.exe\"",
            "\"C:\\Apps\\X\\unins.exe\"",
            "MsiExec.exe /X{0000}",
        ] {
            inst.uninstall_string = bad.into();
            assert!(uninstall_launch(&inst).is_err(), "{bad}");
        }
    }

    #[test]
    fn open_command_construction() {
        let inst = find(&[entry("{OTHER}_is1", "UnForge 1.0.0", "1.0.0")], &detect()).unwrap();
        assert_eq!(
            open_launch(&inst, "UnForge.exe").unwrap().file,
            "C:\\Program Files\\UnForge\\UnForge.exe"
        );
        assert!(open_launch(&inst, "..\\..\\evil.exe").is_err());
        assert!(open_launch(&inst, "readme.txt").is_err());
        assert!(open_launch(&inst, "").is_err());
    }
}
