//! Background loop: watch Apple Music, keep the Discord status in sync.

use crate::discord::Discord;
use crate::itunes::{self, Meta};
use crate::media::{Media, NowPlaying};
use crate::{log, Shared, View, MODE_ARTIST, MODE_BOTH, MODE_SONG};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Track {
    id: String,
    /// Seconds we have watched this track play (used when Windows gives no position).
    elapsed: f64,
    /// False for a song that was already mid-way when we first saw it.
    elapsed_known: bool,
    meta: Meta,
}

/// What Discord is (or should be) showing: track id, start time in epoch ms, display mode.
type Shown = (String, Option<i64>, u8);

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

/// Discord wants 2..=128 characters per text field.
pub fn fit(s: &str) -> String {
    let mut out: String = if s.chars().count() > 128 {
        s.chars().take(127).chain(std::iter::once('\u{2026}')).collect()
    } else {
        s.to_string()
    };
    if out.chars().count() < 2 {
        out.push(' ');
        out.push('\u{200B}');
    }
    out
}

pub fn build_activity(
    np: &NowPlaying,
    meta: &Meta,
    start_ms: Option<i64>,
    duration: Option<f64>,
    mode: u8,
    show_button: bool,
) -> Value {
    // type 2 = "Listening to"
    let mut a = json!({ "type": 2, "details": fit(&np.title) });
    if !np.artist.is_empty() {
        a["state"] = json!(fit(&np.artist));
    }
    // status_display_type picks the text shown under your name:
    // absent = app name, 1 = state, 2 = details.
    match mode {
        MODE_ARTIST if !np.artist.is_empty() => a["status_display_type"] = json!(1),
        MODE_SONG => a["status_display_type"] = json!(2),
        MODE_BOTH => {
            if !np.artist.is_empty() {
                a["details"] = json!(fit(&format!("{} - {}", np.artist, np.title)));
            }
            if np.album.is_empty() {
                a.as_object_mut().unwrap().remove("state");
            } else {
                a["state"] = json!(fit(&np.album));
            }
            a["status_display_type"] = json!(2);
        }
        _ => {}
    }
    if let Some(art) = &meta.art {
        a["assets"] = json!({ "large_image": art });
        if !np.album.is_empty() {
            a["assets"]["large_text"] = json!(fit(&np.album));
        }
    }
    if let Some(start) = start_ms {
        a["timestamps"] = json!({ "start": start });
        if let Some(d) = duration {
            a["timestamps"]["end"] = json!(start + (d * 1000.0) as i64);
        }
    }
    if let (true, Some(url)) = (show_button, &meta.url) {
        if url.len() <= 512 {
            a["buttons"] = json!([{ "label": "Play on Apple Music", "url": url }]);
        }
    }
    a
}

fn needs_update(want: &Option<Shown>, sent: &Option<Shown>) -> bool {
    match (want, sent) {
        (None, None) => false,
        (Some(w), Some(s)) => {
            w.0 != s.0
                || w.2 != s.2
                || match (w.1, s.1) {
                    (Some(a), Some(b)) => (a - b).abs() > 3000,
                    (None, None) => false,
                    _ => true,
                }
        }
        _ => true,
    }
}

pub fn run(shared: Arc<Shared>) {
    let mut media = Media::default();
    let mut cache: HashMap<String, Meta> = HashMap::new();
    let mut discord: Option<Discord> = None;
    let mut track: Option<Track> = None;
    let mut first_poll = true;
    let mut last_tick = Instant::now();
    let mut sent: Option<Shown> = None;
    let mut next_connect = Instant::now();
    let mut last_error = String::new();
    let mut log_once = |msg: String| {
        if msg != last_error {
            log(&msg);
            last_error = msg;
        }
    };
    log("Started.");

    while !shared.quit.load(Relaxed) {
        let enabled = shared.enabled.load(Relaxed);
        if !enabled {
            // "Stop" in the tray menu: clear the Discord status. The mini player keeps updating.
            if let Some(mut d) = discord.take() {
                let _ = d.set_activity(Value::Null);
            }
            sent = None;
            next_connect = Instant::now();
        }

        let np = match media.now_playing() {
            Ok(v) => v,
            Err(e) => {
                log_once(format!("Could not read Apple Music: {e}"));
                None
            }
        };
        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;

        let mut duration = None;
        match &np {
            Some(np) => {
                let id = format!("{}|{}|{}", np.title, np.artist, np.album);
                if track.as_ref().map_or(true, |t| t.id != id) {
                    let meta = match cache.get(&id) {
                        Some(m) => m.clone(),
                        None => match itunes::find_song(&np.title, &np.artist, &np.album, &shared.storefront) {
                            Ok(m) => {
                                cache.insert(id.clone(), m.clone());
                                m
                            }
                            Err(e) => {
                                log_once(format!("Album art lookup failed: {e}"));
                                Meta::default()
                            }
                        },
                    };
                    track = Some(Track { id, elapsed: 0.0, elapsed_known: !first_poll, meta });
                } else if np.playing {
                    track.as_mut().unwrap().elapsed += dt;
                }
                let t = track.as_mut().unwrap();
                duration = np.duration.or(t.meta.duration);
                if let Some(d) = duration {
                    if t.elapsed > d + 3.0 {
                        // Same song still going past its length: it repeated.
                        t.elapsed = 0.0;
                        t.elapsed_known = true;
                    }
                }
                shared.set_view(Some(View {
                    title: np.title.clone(),
                    artist: np.artist.clone(),
                    album: np.album.clone(),
                    playing: np.playing,
                    position: np.position.or(if t.elapsed_known { Some(t.elapsed) } else { None }),
                    duration,
                    as_of: now,
                }));
            }
            None => {
                track = None;
                shared.set_view(None);
            }
        }
        first_poll = false;

        let mode = shared.mode.load(Relaxed);
        let want: Option<Shown> = match (&np, &track) {
            (Some(np), Some(t)) if enabled && np.playing => {
                let pos = np.position.or(if t.elapsed_known { Some(t.elapsed) } else { None });
                Some((t.id.clone(), pos.map(|p| now_ms() - (p * 1000.0) as i64), mode))
            }
            _ => None,
        };

        if enabled && discord.is_none() && now >= next_connect {
            match Discord::connect(&shared.client_id) {
                Ok(Some(d)) => {
                    discord = Some(d);
                    sent = None;
                    log("Connected to Discord.");
                }
                Ok(None) => next_connect = now + Duration::from_secs(10),
                Err(e) => {
                    log_once(e);
                    next_connect = now + Duration::from_secs(10);
                }
            }
        }

        if let Some(d) = discord.as_mut() {
            if needs_update(&want, &sent) {
                let activity = match (&want, &np, &track) {
                    (Some(w), Some(np), Some(t)) => {
                        build_activity(np, &t.meta, w.1, duration, mode, shared.show_button)
                    }
                    _ => Value::Null, // paused or stopped: hide the status
                };
                match d.set_activity(activity) {
                    Ok(refused) => {
                        if let Some(msg) = refused {
                            log_once(format!("Discord refused the update: {msg}"));
                        }
                        sent = want;
                    }
                    Err(e) => {
                        log(&format!("Lost Discord: {e}"));
                        discord = None;
                        sent = None;
                        next_connect = now + Duration::from_secs(5);
                    }
                }
            }
        }
        shared.discord.store(discord.is_some(), Relaxed);

        for _ in 0..8 {
            if shared.quit.load(Relaxed) || shared.enabled.load(Relaxed) != enabled {
                break;
            }
            sleep(Duration::from_millis(250));
        }
    }

    if let Some(mut d) = discord.take() {
        let _ = d.set_activity(Value::Null);
    }
    log("Stopped.");
    shared.done.store(true, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn np(title: &str, artist: &str, album: &str) -> NowPlaying {
        NowPlaying {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            playing: true,
            position: None,
            duration: None,
        }
    }

    #[test]
    fn artist_mode_shows_state_under_name() {
        let meta = Meta { art: Some("https://a/512x512bb.jpg".into()), duration: Some(200.0), url: Some("https://music.apple.com/x".into()) };
        let a = build_activity(&np("Halo", "Beyonc\u{e9}", "I Am"), &meta, Some(1000), Some(200.0), MODE_ARTIST, true);
        assert_eq!(a["type"], 2);
        assert_eq!(a["details"], "Halo");
        assert_eq!(a["state"], "Beyonc\u{e9}");
        assert_eq!(a["status_display_type"], 1);
        assert_eq!(a["timestamps"]["end"], 201000);
        assert_eq!(a["assets"]["large_text"], "I Am");
        assert_eq!(a["buttons"][0]["url"], "https://music.apple.com/x");
    }

    #[test]
    fn both_mode_combines_artist_and_song() {
        let a = build_activity(&np("Halo", "Beyonc\u{e9}", "I Am"), &Meta::default(), None, None, MODE_BOTH, true);
        assert_eq!(a["details"], "Beyonc\u{e9} - Halo");
        assert_eq!(a["state"], "I Am");
        assert_eq!(a["status_display_type"], 2);
        assert!(a.get("timestamps").is_none() && a.get("assets").is_none() && a.get("buttons").is_none());
    }

    #[test]
    fn short_and_long_text_fit_discord_limits() {
        assert_eq!(fit("7").chars().count(), 3);
        assert_eq!(fit(&"x".repeat(300)).chars().count(), 128);
    }

    #[test]
    fn update_only_when_something_changed() {
        let a = Some(("t".to_string(), Some(10_000), 1));
        assert!(!needs_update(&a, &a));
        assert!(!needs_update(&a, &Some(("t".to_string(), Some(11_000), 1))));
        assert!(needs_update(&a, &Some(("t".to_string(), Some(20_000), 1))));
        assert!(needs_update(&a, &Some(("t".to_string(), Some(10_000), 2))));
        assert!(needs_update(&a, &None));
        assert!(!needs_update(&None, &None));
    }

    #[test]
    fn itunes_pick_matches_title_and_artist() {
        let json = json!({ "results": [
            { "trackName": "Halo", "artistName": "Someone Else", "collectionName": "Other", "artworkUrl100": "https://x/100x100bb.jpg" },
            { "trackName": "Halo", "artistName": "Beyonc\u{e9}", "collectionName": "I Am... Sasha Fierce", "artworkUrl100": "https://y/100x100bb.jpg", "trackTimeMillis": 261000, "trackViewUrl": "https://music.apple.com/y" }
        ]});
        let m = itunes::pick(&json, "Halo", "Beyonc\u{e9}", "I Am... Sasha Fierce");
        assert_eq!(m.art.as_deref(), Some("https://y/512x512bb.jpg"));
        assert_eq!(m.duration, Some(261.0));
        let none = itunes::pick(&json, "Different Song", "Nobody", "");
        assert!(none.art.is_none());
    }
}
