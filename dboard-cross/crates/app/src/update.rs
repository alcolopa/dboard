//! Manual "Check for updates": asks the GitHub releases API for the newest tag.

const LATEST: &str = "https://api.github.com/repos/alcolopa/dboard/releases/latest";

pub struct Release {
    pub tag: String,
    pub url: String,
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
    Ok(Release { tag, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9", "1.0.0"));
    }
}
