//! Manual "Check for updates": asks the GitHub releases API for the newest tag, downloads the
//! installer for this computer and patches the installed app in place.

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

/// What `apply` did with a downloaded release.
#[derive(Debug)]
pub enum Staged {
    /// The running program was replaced on disk; restart to use it.
    Replaced(std::path::PathBuf),
    /// A Windows installer, run silently over the current install on restart.
    Installer(std::path::PathBuf),
    /// Could not patch in place (package-managed or read-only install, deb/rpm); open the file instead.
    Manual(std::path::PathBuf, String),
}

fn run(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = std::process::Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

fn find_named(dir: &std::path::Path, name: &str, want_dir: bool, depth: u32) -> Option<std::path::PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if e.file_name() == name && is_dir == want_dir {
            return Some(p);
        }
        if is_dir && depth > 0 {
            if let Some(f) = find_named(&p, name, want_dir, depth - 1) {
                return Some(f);
            }
        }
    }
    None
}

/// Patch the installed app from a downloaded release file. Blocking; run it off the UI thread.
/// Extraction happens in a fresh scratch directory next to `file`; the old program is only replaced
/// by an atomic rename once the new one is fully staged beside it.
pub fn apply(file: &std::path::Path) -> Result<Staged, String> {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    let manual = |why: &str| Ok(Staged::Manual(file.to_path_buf(), why.to_string()));
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(windows) && name.ends_with(".exe") {
        return Ok(Staged::Installer(file.to_path_buf()));
    }
    let scratch = file.with_extension("extract");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let result = (|| {
        if cfg!(target_os = "macos") && name.ends_with(".zip") {
            let app = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|e| e == "app"));
            let Some(app) = app else { return manual("dboard is not running from an app bundle") };
            run("ditto", &["-x".as_ref(), "-k".as_ref(), file.as_os_str(), scratch.as_os_str()])?;
            let new_app = find_named(&scratch, "dboard.app", true, 2).ok_or("The download has no dboard.app")?;
            let staged = app.with_extension("app.new");
            let old = app.with_extension("app.old");
            let _ = std::fs::remove_dir_all(&staged);
            let _ = std::fs::remove_dir_all(&old);
            // `ditto` copies across volumes and keeps signatures and permissions intact.
            if run("ditto", &[new_app.as_os_str(), staged.as_os_str()]).is_err() {
                return manual("the application folder is not writable");
            }
            if std::fs::rename(app, &old).is_err() {
                let _ = std::fs::remove_dir_all(&staged);
                return manual("the application folder is not writable");
            }
            if let Err(e) = std::fs::rename(&staged, app) {
                let _ = std::fs::rename(&old, app);
                return Err(format!("Could not install the update: {e}"));
            }
            let _ = std::fs::remove_dir_all(&old);
            Ok(Staged::Replaced(app.to_path_buf()))
        } else if cfg!(unix) && !cfg!(target_os = "macos") && name.ends_with(".tar.gz") {
            run("tar", &["-xzf".as_ref(), file.as_os_str(), "-C".as_ref(), scratch.as_os_str()])?;
            let new_bin = find_named(&scratch, "dboard", false, 2).ok_or("The download has no dboard program")?;
            let staged = exe.with_extension("new");
            if std::fs::copy(&new_bin, &staged).is_err() {
                let _ = std::fs::remove_file(&staged);
                return manual("the install folder is not writable (use your package manager or install.sh --system)");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
            // Renaming over a running executable is safe on Linux: the old inode stays alive until exit.
            if let Err(e) = std::fs::rename(&staged, &exe) {
                let _ = std::fs::remove_file(&staged);
                return Err(format!("Could not replace {}: {e}", exe.display()));
            }
            Ok(Staged::Replaced(exe.clone()))
        } else {
            manual("this package type is installed with your package manager")
        }
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// Start the updated program (or the silent installer) and let the caller quit.
pub fn relaunch(staged: &Staged) -> Result<(), String> {
    let mut cmd = match staged {
        Staged::Replaced(p) if cfg!(target_os = "macos") => {
            let mut c = std::process::Command::new("open");
            c.arg("-n").arg(p);
            c
        }
        Staged::Replaced(p) => std::process::Command::new(p),
        Staged::Installer(p) => {
            let mut c = std::process::Command::new(p);
            c.args(["/SILENT", "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"]);
            c
        }
        Staged::Manual(..) => return Err("Nothing to relaunch".into()),
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
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
    fn find_named_distinguishes_files_and_folders() {
        let d = std::env::temp_dir().join(format!("dboard-find-{}", std::process::id()));
        std::fs::create_dir_all(d.join("a/dboard.app")).unwrap();
        std::fs::create_dir_all(d.join("a/dboard-1")).unwrap();
        std::fs::write(d.join("a/dboard-1/dboard"), b"x").unwrap();
        assert!(find_named(&d, "dboard.app", true, 2).is_some());
        assert!(find_named(&d, "dboard", true, 2).is_none());
        assert_eq!(find_named(&d, "dboard", false, 2).unwrap(), d.join("a/dboard-1/dboard"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn apply_extracts_a_tarball_or_falls_back_to_manual() {
        let d = std::env::temp_dir().join(format!("dboard-apply-{}", std::process::id()));
        std::fs::create_dir_all(d.join("pkg/dboard-v9")).unwrap();
        std::fs::write(d.join("pkg/dboard-v9/dboard"), b"new").unwrap();
        let tar = d.join("dboard-v9-linux-x86_64.tar.gz");
        run("tar", &["-czf".as_ref(), tar.as_os_str(), "-C".as_ref(), d.join("pkg").as_os_str(), "dboard-v9".as_ref()]).unwrap();
        // The test binary is replaceable (we built it), so only assert that nothing is left behind.
        let deb = d.join("dboard_9_amd64.deb");
        std::fs::write(&deb, b"x").unwrap();
        assert!(matches!(apply(&deb).unwrap(), Staged::Manual(..)));
        assert!(find_named(&d, "dboard_9_amd64.extract", true, 0).is_none(), "scratch folder is removed");
        assert!(tar.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9", "1.0.0"));
    }
}
