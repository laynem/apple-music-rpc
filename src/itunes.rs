//! Looks a song up in Apple's public iTunes Search API for album art, length and a link.

use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Meta {
    pub art: Option<String>,
    pub duration: Option<f64>,
    pub url: Option<String>,
}

pub fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn related(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
}

/// HTTPS client using Windows' own TLS.
pub fn agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    Ok(ureq::AgentBuilder::new().tls_connector(std::sync::Arc::new(tls)).timeout(Duration::from_secs(6)).build())
}

/// `Err` means the lookup itself failed (worth retrying later); a song that
/// simply isn't found returns an empty `Meta`.
pub fn find_song(title: &str, artist: &str, album: &str, storefront: &str) -> Result<Meta, String> {
    let body = agent()?
        .get("https://itunes.apple.com/search")
        .query("media", "music")
        .query("entity", "song")
        .query("limit", "15")
        .query("country", storefront)
        .query("term", &format!("{title} {artist}"))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(pick(&json, title, artist, album))
}

pub fn pick(json: &Value, title: &str, artist: &str, album: &str) -> Meta {
    let (nt, na, nal) = (normalize(title), normalize(artist), normalize(album));
    let mut best: Option<&Value> = None;
    let mut best_score = 0;
    for r in json["results"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        let rt = normalize(r["trackName"].as_str().unwrap_or(""));
        let mut score = if rt.is_empty() {
            continue;
        } else if rt == nt {
            3
        } else if related(&rt, &nt) {
            2
        } else {
            continue;
        };
        let ra = normalize(r["artistName"].as_str().unwrap_or(""));
        if !ra.is_empty() && !na.is_empty() && (ra.contains(&na) || na.contains(&ra)) {
            score += 2;
        }
        if related(&normalize(r["collectionName"].as_str().unwrap_or("")), &nal) {
            score += 2;
        }
        if score > best_score {
            best = Some(r);
            best_score = score;
        }
    }
    let mut meta = Meta::default();
    if let (Some(r), true) = (best, best_score >= 4) {
        meta.art = r["artworkUrl100"].as_str().map(|u| u.replace("100x100bb", "512x512bb"));
        meta.duration = r["trackTimeMillis"].as_f64().map(|ms| ms / 1000.0);
        meta.url = r["trackViewUrl"].as_str().map(str::to_string);
    }
    meta
}
