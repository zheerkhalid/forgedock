//! A minimal HTTPS client on top of WinHTTP (the TLS stack that ships with
//! Windows), plus the rules for which addresses ForgeDock may contact.
//!
//! TLS only, certificate validation left at the system defaults (never
//! weakened), no cookies, no automatic redirects: release downloads follow
//! redirects by hand so every hop can be checked against the allow-list.
//! The only thing sent besides the request itself is the fixed User-Agent.

use crate::catalog::OWNER;
use sha2::{Digest, Sha256};
use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

pub const USER_AGENT: &str = "ForgeDock";

/// Hosts GitHub serves release files from after the redirect.
pub const DOWNLOAD_HOSTS: [&str; 3] = [
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

// ---------------------------------------------------------------------------
// Address rules (pure)
// ---------------------------------------------------------------------------

/// Splits `https://host/path?query` into host (lower case) and the rest.
/// Anything that is not plain HTTPS on the default port is refused.
pub fn parse_https(url: &str) -> Option<(String, String)> {
    let rest = url
        .get(..8)
        .filter(|p| p.eq_ignore_ascii_case("https://"))?;
    let rest = &url[rest.len()..];
    let (host, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = host.to_ascii_lowercase();
    let ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    if !ok || !path.starts_with('/') || url.chars().any(|c| c.is_control() || c == ' ') {
        return None;
    }
    Some((host, path.to_string()))
}

fn plain_segments(path: &str) -> Option<Vec<&str>> {
    if path.contains(['?', '#', '%', '\\']) {
        return None;
    }
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    segs.iter()
        .all(|s| !s.is_empty() && *s != "." && *s != "..")
        .then_some(segs)
}

/// True for `https://github.com/zheerkhalid/<repo>/releases/download/<tag>/<file>`.
/// With `repo` given, the repository must be that one.
pub fn is_release_asset_url(url: &str, repo: Option<&str>) -> bool {
    let Some((host, path)) = parse_https(url) else {
        return false;
    };
    if host != "github.com" {
        return false;
    }
    let Some(segs) = plain_segments(&path) else {
        return false;
    };
    segs.len() == 6
        && segs[0].eq_ignore_ascii_case(OWNER)
        && repo.is_none_or(|r| segs[1].eq_ignore_ascii_case(r))
        && segs[2] == "releases"
        && segs[3] == "download"
}

/// Where a release download may be redirected to: GitHub's own download
/// hosts, or another release file of the same publisher.
pub fn is_allowed_redirect(url: &str) -> bool {
    match parse_https(url) {
        Some((host, _)) => {
            DOWNLOAD_HOSTS.contains(&host.as_str()) || is_release_asset_url(url, None)
        }
        None => false,
    }
}

/// What plain GET requests may fetch: the publisher's releases on the GitHub
/// API, and the publisher's files (catalogue, icons, screenshots) on
/// raw.githubusercontent.com.
pub fn is_allowed_fetch(url: &str) -> bool {
    let Some((host, path)) = parse_https(url) else {
        return false;
    };
    let Some(segs) = plain_segments(&path) else {
        return false;
    };
    match host.as_str() {
        "api.github.com" => {
            segs.len() == 5
                && segs[0] == "repos"
                && segs[1] == OWNER
                && segs[3] == "releases"
                && segs[4] == "latest"
        }
        "raw.githubusercontent.com" => segs.len() >= 4 && segs[0] == OWNER,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// WinHTTP
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u32,
    pub body: Vec<u8>,
    /// `X-RateLimit-Remaining` and `X-RateLimit-Reset`, when present.
    pub rate_remaining: Option<u64>,
    pub rate_reset: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// Could not connect, TLS failed, or the request timed out.
    Unreachable(String),
    /// The response was larger than the caller allows.
    TooLarge,
    /// The address is not one ForgeDock is allowed to contact.
    Blocked(String),
    /// The server answered with an unexpected status.
    Status(u32),
    Cancelled,
    Io(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Unreachable(e) => {
                write!(
                    f,
                    "Could not reach GitHub. Check your internet connection. ({e})"
                )
            }
            NetError::TooLarge => f.write_str("The server sent an unexpectedly large answer."),
            NetError::Blocked(host) => write!(
                f,
                "Refused: the download pointed outside GitHub's download servers ({host})."
            ),
            NetError::Status(s) => write!(f, "The server answered with status {s}."),
            NetError::Cancelled => f.write_str("Cancelled."),
            NetError::Io(e) => write!(f, "Could not write the file: {e}"),
        }
    }
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from WinHTTP and is closed once.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn last_error(what: &str) -> NetError {
    NetError::Unreachable(format!(
        "{what}: {}",
        windows::core::Error::from_win32().message()
    ))
}

/// An open request whose response headers have arrived. Fields drop in
/// order: request, then connection, then session.
struct Conn {
    request: Handle,
    _connect: Handle,
    _session: Handle,
}

fn open(host: &str, path: &str, headers: &[&str], receive_ms: i32) -> Result<Conn, NetError> {
    let host = HSTRING::from(host);
    let path = HSTRING::from(path);
    let agent = HSTRING::from(USER_AGENT);
    // SAFETY: every pointer passed below outlives the call it is passed to,
    // and every handle is closed by its guard.
    unsafe {
        let session = Handle(WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(last_error("open"));
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, receive_ms);
        let protocols = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3)
            .to_le_bytes();
        if WinHttpSetOption(
            Some(session.0),
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            Some(&protocols),
        )
        .is_err()
        {
            // Older Windows without TLS 1.3: ask for TLS 1.2 only.
            let tls12 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_le_bytes();
            let _ = WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                Some(&tls12),
            );
        }
        let connect = Handle(WinHttpConnect(session.0, &host, 443, 0));
        if connect.0.is_null() {
            return Err(last_error("connect"));
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(last_error("request"));
        }
        let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_le_bytes();
        let _ = WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&never),
        );
        let no_cookies = WINHTTP_DISABLE_COOKIES.to_le_bytes();
        let _ = WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&no_cookies),
        );
        let header_text: Vec<u16> = headers.join("\r\n").encode_utf16().collect();
        let header_arg = (!header_text.is_empty()).then_some(&header_text[..]);
        WinHttpSendRequest(request.0, header_arg, None, 0, 0, 0)
            .map_err(|e| NetError::Unreachable(format!("send: {}", e.message())))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|e| NetError::Unreachable(format!("receive: {}", e.message())))?;
        Ok(Conn {
            request,
            _connect: connect,
            _session: session,
        })
    }
}

impl Conn {
    fn status(&self) -> Result<u32, NetError> {
        let mut status: u32 = 0;
        let mut size = 4u32;
        // SAFETY: `status` and `size` live across the call.
        unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut c_void),
                &mut size,
                std::ptr::null_mut(),
            )
            .map_err(|e| NetError::Unreachable(format!("status: {}", e.message())))?;
        }
        Ok(status)
    }

    /// A response header by name, if present and of a sane length.
    fn header(&self, name: &str) -> Option<String> {
        let name = HSTRING::from(name);
        let mut buf = vec![0u16; 8192];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: the buffer is `size` bytes long and outlives the call.
        unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_CUSTOM,
                &name,
                Some(buf.as_mut_ptr() as *mut c_void),
                &mut size,
                std::ptr::null_mut(),
            )
            .ok()?;
        }
        let len = (size as usize / 2).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]).trim().to_string())
    }

    /// Reads the next chunk of the body; 0 means the end.
    fn read(&self, chunk: &mut [u8]) -> Result<usize, NetError> {
        let mut read = 0u32;
        // SAFETY: `chunk` is valid for `chunk.len()` bytes.
        unsafe {
            WinHttpReadData(
                self.request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
            .map_err(|e| NetError::Unreachable(format!("read: {}", e.message())))?;
        }
        Ok(read as usize)
    }
}

/// `GET` of an allowed address (see [`is_allowed_fetch`]). No redirects.
pub fn get(url: &str, headers: &[&str], max_body: usize) -> Result<Response, NetError> {
    if !is_allowed_fetch(url) {
        return Err(NetError::Blocked(url.chars().take(80).collect()));
    }
    let (host, path) = parse_https(url).ok_or_else(|| NetError::Blocked(url.into()))?;
    let conn = open(&host, &path, headers, 15_000)?;
    let status = conn.status()?;
    let number = |name: &str| conn.header(name).and_then(|v| v.parse::<u64>().ok());
    let rate_remaining = number("X-RateLimit-Remaining");
    let rate_reset = number("X-RateLimit-Reset");
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 32 * 1024];
    loop {
        let n = conn.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if body.len() + n > max_body {
            return Err(NetError::TooLarge);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(Response {
        status,
        body,
        rate_remaining,
        rate_reset,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub size: u64,
    /// Lower-case hex SHA-256 of the bytes written.
    pub sha256: String,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads a release file of `repo` to `dest`. The first address must be a
/// release asset of the publisher on github.com; every redirect must stay on
/// GitHub's download hosts. `max_size` caps what is written.
pub fn download(
    url: &str,
    repo: &str,
    dest: &Path,
    max_size: u64,
    progress: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<Downloaded, NetError> {
    let mut current = url.to_string();
    for hop in 0..6 {
        let allowed = if hop == 0 {
            is_release_asset_url(&current, Some(repo))
        } else {
            is_allowed_redirect(&current)
        };
        let parsed = parse_https(&current).filter(|_| allowed);
        let Some((host, path)) = parsed else {
            return Err(NetError::Blocked(
                current
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect(),
            ));
        };
        let conn = open(&host, &path, &["Accept: application/octet-stream"], 30_000)?;
        match conn.status()? {
            200 => return save_body(&conn, dest, max_size, progress, cancel),
            301 | 302 | 303 | 307 | 308 => {
                current = conn
                    .header("Location")
                    .ok_or_else(|| NetError::Unreachable("redirect without an address".into()))?;
            }
            other => return Err(NetError::Status(other)),
        }
    }
    Err(NetError::Unreachable("too many redirects".into()))
}

fn save_body(
    conn: &Conn,
    dest: &Path,
    max_size: u64,
    progress: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<Downloaded, NetError> {
    let io = |e: std::io::Error| NetError::Io(e.to_string());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    let part = std::path::PathBuf::from(name);
    let result = (|| {
        let mut file = std::fs::File::create(&part).map_err(io)?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(NetError::Cancelled);
            }
            let n = conn.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > max_size {
                return Err(NetError::TooLarge);
            }
            hasher.update(&chunk[..n]);
            file.write_all(&chunk[..n]).map_err(io)?;
            progress.store(total, Ordering::Relaxed);
        }
        file.flush().map_err(io)?;
        Ok(Downloaded {
            size: total,
            sha256: hex(&hasher.finalize()),
        })
    })();
    match &result {
        Ok(_) => {
            let _ = std::fs::remove_file(dest);
            std::fs::rename(&part, dest).map_err(io)?;
        }
        Err(_) => {
            let _ = std::fs::remove_file(&part);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_only() {
        assert_eq!(
            parse_https("https://GitHub.com/a/b?c=1"),
            Some(("github.com".into(), "/a/b?c=1".into()))
        );
        assert_eq!(
            parse_https("https://github.com"),
            Some(("github.com".into(), "/".into()))
        );
        assert!(parse_https("http://github.com/a").is_none());
        assert!(parse_https("ftp://github.com/a").is_none());
        assert!(parse_https("https://user@github.com/a").is_none());
        assert!(parse_https("https://github.com:8443/a").is_none());
        assert!(parse_https("https://github.com@evil.example/a").is_none());
        assert!(parse_https("https:///a").is_none());
        assert!(parse_https("https://github.com/a b").is_none());
        assert!(parse_https("").is_none());
    }

    #[test]
    fn release_assets_only_from_the_publisher() {
        let good =
            "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge-Setup.exe";
        assert!(is_release_asset_url(good, Some("snapforge")));
        assert!(is_release_asset_url(good, None));
        // Another repository than the one asked for.
        assert!(!is_release_asset_url(good, Some("unforge")));
        for bad in [
            "http://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge-Setup.exe",
            "https://github.com/someoneelse/snapforge/releases/download/v0.1.2/SnapForge-Setup.exe",
            "https://github.com.evil.example/zheerkhalid/snapforge/releases/download/v1/a.exe",
            "https://evil.example/zheerkhalid/snapforge/releases/download/v1/a.exe",
            "https://github.com/zheerkhalid/snapforge/releases/download/v1/../../../x/a.exe",
            "https://github.com/zheerkhalid/snapforge/releases/download/v1/a.exe?x=1",
            "https://github.com/zheerkhalid/snapforge/releases/download/v1/%2e%2e/a.exe",
            "https://github.com/zheerkhalid/snapforge/archive/main.zip",
            "https://github.com/zheerkhalid/snapforge/releases/download/v1",
            "https://raw.githubusercontent.com/zheerkhalid/snapforge/main/a.exe",
        ] {
            assert!(!is_release_asset_url(bad, None), "{bad}");
        }
    }

    #[test]
    fn redirects_stay_on_github_download_hosts() {
        assert!(is_allowed_redirect(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=abc&x=%2F"
        ));
        assert!(is_allowed_redirect(
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2?X-Amz=1"
        ));
        assert!(is_allowed_redirect(
            "https://github.com/zheerkhalid/snapforge/releases/download/v0.1.2/SnapForge-Setup.exe"
        ));
        for bad in [
            "http://objects.githubusercontent.com/x",
            "https://objects.githubusercontent.com.evil.example/x",
            "https://evil.example/objects.githubusercontent.com/x",
            "https://evilgithubusercontent.com/x",
            "https://gist.githubusercontent.com/x",
            "https://github.com/someoneelse/tool/releases/download/v1/a.exe",
            "https://github.com/login",
            "https://objects.githubusercontent.com@evil.example/x",
            "file:///C:/Windows/System32/cmd.exe",
            "",
        ] {
            assert!(!is_allowed_redirect(bad), "{bad}");
        }
    }

    #[test]
    fn plain_fetches_only_from_the_publisher() {
        assert!(is_allowed_fetch(crate::CATALOG_URL));
        assert!(is_allowed_fetch(
            "https://api.github.com/repos/zheerkhalid/snapforge/releases/latest"
        ));
        assert!(is_allowed_fetch(
            "https://raw.githubusercontent.com/zheerkhalid/unforge/main/docs/screenshots/programs.png"
        ));
        for bad in [
            "https://api.github.com/repos/someoneelse/snapforge/releases/latest",
            "https://api.github.com/user",
            "https://api.github.com/repos/zheerkhalid/snapforge/releases/latest?per_page=1",
            "https://raw.githubusercontent.com/someoneelse/x/main/a.png",
            "https://raw.githubusercontent.com/zheerkhalid/../someoneelse/x/a.png",
            "https://example.com/zheerkhalid/x/main/a.png",
            "http://raw.githubusercontent.com/zheerkhalid/x/main/a.png",
        ] {
            assert!(!is_allowed_fetch(bad), "{bad}");
        }
    }

    #[test]
    fn blocked_addresses_never_reach_the_network() {
        let progress = AtomicU64::new(0);
        let cancel = AtomicBool::new(false);
        let dest = std::env::temp_dir().join("forgedock-test-never-written.bin");
        let r = download(
            "https://example.com/zheerkhalid/snapforge/releases/download/v1/a.exe",
            "snapforge",
            &dest,
            1 << 20,
            &progress,
            &cancel,
        );
        assert!(matches!(r, Err(NetError::Blocked(_))));
        assert!(!dest.exists());
        assert!(matches!(
            get("https://example.com/", &[], 1024),
            Err(NetError::Blocked(_))
        ));
    }

    #[test]
    fn hex_encoding() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }
}
