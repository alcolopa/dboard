//! Manual "Check for updates": asks the GitHub releases API for the newest tag.

const LATEST: &str = "https://api.github.com/repos/alcolopa/dboard/releases/latest";

pub struct Release {
    pub tag: String,
    pub url: String,
    /// (file name, download url, size in bytes)
    pub assets: Vec<(String, String, u64)>,
}

/// The installer for this computer, chosen by OS and CPU from the names the release workflow
/// uses (`dboard-v1.0.0-macos-arm64.zip`, `...-linux-x86_64.tar.gz`, ...).
pub fn pick_asset<'a>(assets: &'a [(String, String, u64)], os: &str, arch: &str) -> Option<&'a (String, String, u64)> {
    let os_tokens: &[&str] = match os {
        "macos" => &["macos", "darwin", "mac"],
        "windows" => &["windows", "win"],
        _ => &["linux"],
    };
    let arch_tokens: &[&str] = match arch {
        "aarch64" | "arm64" => &["arm64", "aarch64"],
        _ => &["x86_64", "amd64", "x64"],
    };
    let ext_rank = |n: &str| ["zip", "tar.gz", "msi", "exe", "dmg", "deb", "rpm"].iter().position(|e| n.to_lowercase().ends_with(e)).unwrap_or(99);
    assets
        .iter()
        .filter(|(n, _, _)| {
            let l = n.to_lowercase();
            os_tokens.iter().any(|t| l.contains(t)) && arch_tokens.iter().any(|t| l.contains(t)) && ext_rank(&l) < 99
        })
        .min_by_key(|(n, _, _)| ext_rank(n))
}

/// Blocking download into `dir`; returns the saved path.
pub fn download(url: &str, name: &str, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let resp = ureq::get(url).set("User-Agent", "dboard-update-check").timeout(std::time::Duration::from_secs(300)).call().map_err(|e| e.to_string())?;
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    let path = dir.join(safe);
    let part = path.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| e.to_string())?;
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn parse_version(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(0)).collect()
}

/// True when `candidate` is a higher version than `current` (`v1.2.0` vs `1.1.9`).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_version(candidate) > parse_version(current)
}

/// Blocking; run it off the UI thread.
pub fn latest_release() -> Result<Release, String> {
    let body = ureq::get(LATEST)
        .set("User-Agent", "dboard-update-check")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("No release found.")?.to_string();
    let url = v["html_url"].as_str().unwrap_or("https://github.com/alcolopa/dboard/releases").to_string();
    let assets = v["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["browser_download_url"].as_str()?.to_string(), x["size"].as_u64().unwrap_or(0)))).collect())
        .unwrap_or_default();
    Ok(Release { tag, url, assets })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Vec<(String, String, u64)> {
        ["dboard-v1.2.0-macos-arm64.zip", "dboard-v1.2.0-macos-x86_64.zip", "dboard-v1.2.0-linux-x86_64.tar.gz", "dboard_1.2.0_amd64.deb", "dboard-v1.2.0-windows-x86_64.zip", "dboard-v1.2.0-windows-arm64.zip", "SHA256SUMS.txt"]
            .iter()
            .map(|n| (n.to_string(), format!("https://example.com/{n}"), 1))
            .collect()
    }

    #[test]
    fn picks_the_installer_for_this_computer() {
        let a = assets();
        assert_eq!(pick_asset(&a, "macos", "aarch64").unwrap().0, "dboard-v1.2.0-macos-arm64.zip");
        assert_eq!(pick_asset(&a, "macos", "x86_64").unwrap().0, "dboard-v1.2.0-macos-x86_64.zip");
        assert_eq!(pick_asset(&a, "linux", "x86_64").unwrap().0, "dboard-v1.2.0-linux-x86_64.tar.gz");
        assert_eq!(pick_asset(&a, "windows", "aarch64").unwrap().0, "dboard-v1.2.0-windows-arm64.zip");
        assert!(pick_asset(&a, "linux", "aarch64").is_none());
    }

    #[test]
    fn downloads_a_file_and_never_leaves_a_partial_one() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 1024];
                let n = s.read(&mut buf).unwrap();
                let ok = String::from_utf8_lossy(&buf[..n]).contains("GET /ok");
                let _ = if ok {
                    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                } else {
                    s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                };
            }
        });
        let dir = std::env::temp_dir().join(format!("dboard-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = download(&format!("http://127.0.0.1:{port}/ok"), "dboard v1/../x.zip", &dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(path.file_name().unwrap(), "dboardv1..x.zip", "path separators are stripped from asset names");
        assert!(download(&format!("http://127.0.0.1:{port}/missing"), "nope.zip", &dir).is_err());
        assert!(!dir.join("nope.part").exists() && !dir.join("nope.zip").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9", "1.0.0"));
    }
}
