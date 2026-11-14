//! Version info and the "is there a newer release?" check against GitHub.

use serde_json::Value;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHOR: &str = "Teckem";
pub const HOME_URL: &str = "https://github.com/laynem/apple-music-rpc";
pub const RELEASES_URL: &str = "https://github.com/laynem/apple-music-rpc/releases/latest";
pub const CHANGELOG_URL: &str = "https://github.com/laynem/apple-music-rpc/blob/main/CHANGELOG.md";
const LATEST_API: &str = "https://api.github.com/repos/laynem/apple-music-rpc/releases/latest";

fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

/// True when `latest` (a release tag such as "v1.2.0") is newer than `current`.
pub fn newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// Tag name of the newest published release.
pub fn latest() -> Result<String, String> {
    let body = crate::itunes::agent()?
        .get(LATEST_API)
        .set("User-Agent", "apple-music-rpc")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    json["tag_name"].as_str().map(str::to_string).ok_or_else(|| "no release found".to_string())
}
