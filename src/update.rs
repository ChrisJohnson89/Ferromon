use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use flate2::read::GzDecoder;
use serde::Deserialize;
use tar::Archive;

use crate::types::UpdateState;

pub const VERSION: &str = env!("FERRO_VERSION");
const REPO_OWNER: &str = "ChrisJohnson89";
const REPO_NAME: &str = "Ferromon";
const UPDATE_CHECK_TTL_SEC: u64 = 6 * 60 * 60;

#[derive(Debug, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn target_triple() -> &'static str {
    "x86_64-unknown-linux-musl"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn target_triple() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn target_triple() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
)))]
fn target_triple() -> &'static str {
    "unknown"
}

fn update_api_url() -> String {
    format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    )
}

fn should_check_update(st: &UpdateState) -> bool {
    match st.last_checked_at {
        None => true,
        Some(t) => t
            .elapsed()
            .map(|d| d.as_secs() >= UPDATE_CHECK_TTL_SEC)
            .unwrap_or(true),
    }
}

fn update_cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("ferromon")
            .join("update.json"),
    )
}

pub fn load_update_cache() -> UpdateState {
    let path = match update_cache_path() {
        Some(p) => p,
        None => return UpdateState::default(),
    };

    let Ok(bytes) = fs::read(&path) else {
        return UpdateState::default();
    };

    #[derive(Deserialize)]
    struct Cache {
        last_checked_unix: u64,
        latest_tag: Option<String>,
        available: bool,
    }

    let Ok(c) = serde_json::from_slice::<Cache>(&bytes) else {
        return UpdateState::default();
    };

    UpdateState {
        last_checked_at: Some(std::time::UNIX_EPOCH + Duration::from_secs(c.last_checked_unix)),
        latest_tag: c.latest_tag,
        available: c.available,
        error: None,
    }
}

fn save_update_cache(st: &UpdateState) {
    let Some(path) = update_cache_path() else {
        return;
    };

    let Some(dir) = path.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);

    let last_checked_unix = st
        .last_checked_at
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    #[derive(serde::Serialize)]
    struct Cache<'a> {
        last_checked_unix: u64,
        latest_tag: &'a Option<String>,
        available: bool,
    }

    let bytes = serde_json::to_vec_pretty(&Cache {
        last_checked_unix,
        latest_tag: &st.latest_tag,
        available: st.available,
    });

    if let Ok(bytes) = bytes {
        let _ = fs::write(&path, bytes);
    }
}

pub fn check_update(mut st: UpdateState) -> UpdateState {
    if std::env::var_os("FERRO_NO_UPDATE_CHECK").is_some() {
        return st;
    }

    if !should_check_update(&st) {
        return st;
    }

    let url = update_api_url();
    let req = ureq::get(&url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(3));

    match req.call() {
        Ok(resp) => {
            let Ok(body) = resp.into_string() else {
                st.error = Some("failed to read response".to_string());
                st.last_checked_at = Some(std::time::SystemTime::now());
                save_update_cache(&st);
                return st;
            };
            let parsed = serde_json::from_str::<GhRelease>(&body);
            match parsed {
                Ok(r) => {
                    st.latest_tag = Some(r.tag_name.clone());
                    st.available = r.tag_name.trim_start_matches('v') != VERSION;
                    st.error = None;
                }
                Err(e) => {
                    st.error = Some(format!("bad json: {e}"));
                }
            }
        }
        Err(e) => {
            st.error = Some(format!("update check failed: {e}"));
        }
    }

    st.last_checked_at = Some(std::time::SystemTime::now());
    save_update_cache(&st);
    st
}

fn install_path_user() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local").join("bin").join("ferro"))
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    let mut reader = resp.into_reader();
    let mut buf: Vec<u8> = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    Ok(buf)
}

pub fn perform_self_update(latest_tag: &str) -> Result<String, String> {
    let target = target_triple();
    if target == "unknown" {
        return Err("unknown target".to_string());
    }

    let asset_name = format!("ferromon-{}-{}.tar.gz", latest_tag, target);
    let sha_name = format!("{}.sha256", asset_name);

    let rel_url = update_api_url();
    let resp = ureq::get(&rel_url)
        .set("User-Agent", "ferromon")
        .timeout(Duration::from_secs(3))
        .call()
        .map_err(|e| format!("update metadata fetch failed: {e}"))?;

    let body = resp
        .into_string()
        .map_err(|e| format!("read release json failed: {e}"))?;
    let r = serde_json::from_str::<GhRelease>(&body).map_err(|e| format!("bad json: {e}"))?;

    let mut asset_url: Option<String> = None;
    let mut sha_url: Option<String> = None;
    for a in r.assets {
        if a.name == asset_name {
            asset_url = Some(a.browser_download_url);
        } else if a.name == sha_name {
            sha_url = Some(a.browser_download_url);
        }
    }

    let asset_url = asset_url.ok_or_else(|| format!("missing asset {asset_name}"))?;
    let sha_url = sha_url.ok_or_else(|| format!("missing asset {sha_name}"))?;

    // Download both
    let tar_gz = download_bytes(&asset_url)?;
    let sha_txt =
        String::from_utf8(download_bytes(&sha_url)?).map_err(|e| format!("bad sha: {e}"))?;

    // Parse sha file: "<hex>  <filename>"
    let expected = sha_txt
        .split_whitespace()
        .next()
        .ok_or_else(|| "bad sha file".to_string())
        .map(|s| s.to_string())?;

    if expected.len() < 16 {
        return Err("checksum looked wrong".to_string());
    }

    // Extract `ferro` from tar.gz
    let mut ar = Archive::new(GzDecoder::new(&tar_gz[..]));
    let mut ferro_bytes: Option<Vec<u8>> = None;
    for entry in ar.entries().map_err(|e| format!("tar read failed: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry failed: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("tar path failed: {e}"))?
            .to_string_lossy()
            .to_string();
        if path == "ferro" {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("tar read ferro failed: {e}"))?;
            ferro_bytes = Some(buf);
            break;
        }
    }

    let ferro_bytes = ferro_bytes.ok_or_else(|| "missing ferro in archive".to_string())?;

    let dst = install_path_user().ok_or_else(|| "HOME not set".to_string())?;
    let dir = dst
        .parent()
        .ok_or_else(|| "invalid install destination".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("mkdir failed: {e}"))?;

    let tmp = dst.with_extension("new");
    fs::write(&tmp, &ferro_bytes).map_err(|e| format!("write failed: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&tmp)
            .map_err(|e| format!("metadata failed: {e}"))?
            .permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&tmp, perm).map_err(|e| format!("chmod failed: {e}"))?;
    }

    fs::rename(&tmp, &dst).map_err(|e| format!("install failed: {e}"))?;

    Ok(format!(
        "Updated to {} (installed to {})
If this isn't on PATH, add ~/.local/bin to PATH.",
        latest_tag,
        dst.display()
    ))
}
