//! Apple Music -> Discord Rich Presence, as a Windows system tray app with a
//! small "now playing" readout in the tray menu.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod discord;
mod icon;
mod itunes;
mod media;
mod platform;
mod update;
mod worker;

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Icon as MenuIcon, IconMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};

const APP_NAME: &str = "Apple Music RPC";
const PORTAL: &str = "https://discord.com/developers/applications";

pub const MODE_APP: u8 = 0;
pub const MODE_ARTIST: u8 = 1;
pub const MODE_SONG: u8 = 2;
pub const MODE_BOTH: u8 = 3;
/// (mode, config.json value, menu label)
const MODES: [(u8, &str, &str); 4] = [
    (MODE_ARTIST, "artist", "Artist"),
    (MODE_SONG, "song", "Song"),
    (MODE_BOTH, "both", "Artist - Song"),
    (MODE_APP, "app", "Apple Music"),
];

/// Snapshot of what Apple Music is doing, published by the worker for the tray.
#[derive(Clone)]
pub struct View {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Seconds into the song at `as_of`, when known.
    pub position: Option<f64>,
    /// Song length in seconds, when known.
    pub duration: Option<f64>,
    pub as_of: Instant,
}

/// State shared between the tray (main thread) and the worker thread.
pub struct Shared {
    pub enabled: AtomicBool,
    pub quit: AtomicBool,
    pub done: AtomicBool,
    pub discord: AtomicBool,
    pub mode: AtomicU8,
    /// `None` = Apple Music isn't open.
    pub view: Mutex<Option<View>>,
    pub client_id: String,
    pub storefront: String,
    pub show_button: bool,
}

impl Shared {
    pub fn set_view(&self, v: Option<View>) {
        *self.view.lock().unwrap() = v;
    }
}

// ---------- log ----------
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn log(msg: &str) {
    let Some(path) = LOG_PATH.get() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", platform::timestamp(), msg);
    }
}

// ---------- config ----------
struct Config {
    client_id: String,
    storefront: String,
    show_button: bool,
    status_line: String,
}

fn valid_id(s: &str) -> bool {
    (17..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

fn load_config(path: &Path) -> Option<Config> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let client_id = match &v["clientId"] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if !valid_id(&client_id) {
        return None;
    }
    let status_line = v["statusLine"].as_str().unwrap_or("artist").to_string();
    Some(Config {
        client_id,
        storefront: v["storefront"].as_str().unwrap_or("us").to_string(),
        show_button: v["showButton"].as_bool().unwrap_or(true),
        status_line: if MODES.iter().any(|m| m.1 == status_line) { status_line } else { "artist".into() },
    })
}

fn save_config(path: &Path, c: &Config) {
    let v = json!({
        "clientId": c.client_id,
        "storefront": c.storefront,
        "showButton": c.show_button,
        "statusLine": c.status_line,
    });
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(path, text);
    }
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
}

/// Starts the Apple Music app, or brings it to the front when it is already open.
fn open_apple_music() {
    let _ = std::process::Command::new("explorer.exe")
        .arg(r"shell:AppsFolder\AppleInc.AppleMusicWin_nzyj5cx40ttqa!App")
        .spawn();
}

fn first_run_setup() -> Option<String> {
    open_url(PORTAL);
    let mut prompt = String::from(
        "First-time setup\n\n\
         1. In the Discord Developer Portal (opening in your browser), click New Application and name it \"Apple Music\".\n\
         2. Copy its Application ID from the General Information page.\n\
         3. Click OK here - the ID is read from your clipboard.",
    );
    loop {
        if !platform::confirm(APP_NAME, &prompt) {
            return None;
        }
        let clip = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
        let id = clip.trim().to_string();
        if valid_id(&id) {
            return Some(id);
        }
        prompt = String::from(
            "The clipboard doesn't hold an Application ID (a 17-20 digit number).\n\n\
             Copy the Application ID from the Developer Portal, then click OK to try again.",
        );
    }
}

// ---------- start with Windows ----------
#[cfg(windows)]
mod autostart {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "AppleMusicDiscordRPC";

    fn key() -> Option<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ | KEY_WRITE).ok()
    }
    pub fn is_on() -> bool {
        key().and_then(|k| k.get_value::<String, _>(NAME).ok()).is_some()
    }
    pub fn set(on: bool) {
        let Some(k) = key() else { return };
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let _ = k.set_value(NAME, &format!("\"{}\"", exe.display()));
            }
        } else {
            let _ = k.delete_value(NAME);
        }
    }
}
#[cfg(not(windows))]
mod autostart {
    pub fn is_on() -> bool {
        false
    }
    pub fn set(_on: bool) {}
}

// ---------- mini player text ----------
fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max - 3).chain("...".chars()).collect()
    } else {
        s.to_string()
    }
}

/// The three readout lines: song, "artist — album", and progress.
/// The last two are `None` when Apple Music isn't open.
fn player_lines(view: &Option<View>, now: Instant) -> (String, Option<String>, Option<String>) {
    let Some(v) = view else {
        return ("Apple Music is not open".into(), None, None);
    };
    let by = match (v.artist.is_empty(), v.album.is_empty()) {
        (false, false) => format!("{} \u{2014} {}", v.artist, v.album),
        (false, true) => v.artist.clone(),
        (true, false) => v.album.clone(),
        (true, true) => "Unknown artist".into(),
    };
    let pos = v.position.map(|p| {
        let p = if v.playing { p + now.saturating_duration_since(v.as_of).as_secs_f64() } else { p };
        v.duration.map_or(p, |d| p.min(d))
    });
    let mut time = match (pos, v.duration) {
        (Some(p), Some(d)) if d > 0.0 => {
            const CELLS: usize = 12;
            let filled = ((p / d) * CELLS as f64).round() as usize;
            let bar: String = (0..CELLS).map(|i| if i < filled { '\u{25A0}' } else { '\u{25A1}' }).collect();
            format!("{bar}  {} / {}  (-{})", clock(p), clock(d), clock(d - p))
        }
        (Some(p), _) => clock(p),
        (None, Some(d)) => format!("--:-- / {}", clock(d)),
        (None, None) => String::new(),
    };
    if !v.playing {
        time = if time.is_empty() { "Paused".into() } else { format!("{time}  paused") };
    } else if time.is_empty() {
        time = "Playing".into();
    }
    (v.title.clone(), Some(by), Some(time))
}

fn make_icon(color: (u8, u8, u8)) -> Icon {
    Icon::from_rgba(icon::rgba(color), icon::SIZE, icon::SIZE).expect("icon")
}

fn make_dot(color: (u8, u8, u8)) -> MenuIcon {
    MenuIcon::from_rgba(icon::dot(color), icon::DOT, icon::DOT).expect("dot")
}

fn menu_text(s: &str) -> String {
    shorten(s, 64).replace('&', "&&")
}

fn main() {
    if !platform::single_instance() {
        return;
    }

    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let config_path = dir.join("config.json");
    let log_path = dir.join("log.txt");
    if std::fs::metadata(&log_path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&log_path);
    }
    let _ = LOG_PATH.set(log_path.clone());

    let mut config = match load_config(&config_path) {
        Some(c) => c,
        None => {
            let Some(client_id) = first_run_setup() else { return };
            let c = Config { client_id, storefront: "us".into(), show_button: true, status_line: "artist".into() };
            save_config(&config_path, &c);
            autostart::set(true); // on by default; untick in the tray menu
            c
        }
    };
    if autostart::is_on() {
        autostart::set(true); // keep the entry pointing at wherever the exe lives now
    }

    let mode = MODES.iter().find(|m| m.1 == config.status_line).map_or(MODE_ARTIST, |m| m.0);
    let shared = Arc::new(Shared {
        enabled: AtomicBool::new(true),
        quit: AtomicBool::new(false),
        done: AtomicBool::new(false),
        discord: AtomicBool::new(false),
        mode: AtomicU8::new(mode),
        view: Mutex::new(None),
        client_id: config.client_id.clone(),
        storefront: config.storefront.clone(),
        show_button: config.show_button,
    });
    {
        let shared = shared.clone();
        std::thread::spawn(move || worker::run(shared));
    }

    // ---------- tray icon and menu ----------
    let icon_on = make_icon((250, 45, 72));
    let icon_off = make_icon((120, 120, 120));

    // Row 0: whether Apple Music is open, with a status light. Clicking it opens Apple Music.
    const MUSIC_ON: &str = "Apple Music: connected";
    const MUSIC_OFF: &str = "Apple Music: not connected (click to open)";
    let dot_on = make_dot((52, 199, 89));
    let dot_off = make_dot((140, 140, 140));
    let mi_music = IconMenuItem::new(MUSIC_OFF, true, Some(dot_off.clone()), None);
    // Mini player readout (rows 1-3, only present while Apple Music is open).
    let mi_title = MenuItem::new("", true, None);
    let mi_by = MenuItem::new("", true, None);
    let mi_time = MenuItem::new("", true, None);
    let mi_discord = MenuItem::new("Discord: starting...", false, None);
    let mi_start = MenuItem::new("Start", false, None);
    let mi_stop = MenuItem::new("Stop", true, None);
    let mode_items: Vec<(u8, &str, CheckMenuItem)> = MODES
        .iter()
        .map(|&(m, key, label)| (m, key, CheckMenuItem::new(label, true, m == mode, None)))
        .collect();
    let sub_line = Submenu::new("Show under my name", true);
    for (_, _, item) in &mode_items {
        let _ = sub_line.append(item);
    }
    let mi_auto = CheckMenuItem::new("Start with Windows", true, autostart::is_on(), None);

    // About: version, maker, where to get it, updates.
    let mi_version = MenuItem::new(format!("{APP_NAME} {}", update::VERSION), false, None);
    let mi_author = MenuItem::new(format!("Made by {}", update::AUTHOR), false, None);
    let mi_home = MenuItem::new("Get it on GitHub", true, None);
    let mi_changes = MenuItem::new("What's new", true, None);
    let mi_update = MenuItem::new("Check for updates", true, None);
    let sub_about = Submenu::new("About", true);
    let _ = sub_about.append_items(&[
        &mi_version,
        &mi_author,
        &PredefinedMenuItem::separator(),
        &mi_home,
        &mi_changes,
        &mi_update,
    ]);
    // Result of an update check, handed over by the thread that ran it.
    let update_result: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let check_for_update = {
        let slot = update_result.clone();
        move || {
            let slot = slot.clone();
            std::thread::spawn(move || {
                let result = update::latest();
                *slot.lock().unwrap() = Some(result);
            });
        }
    };
    check_for_update(); // quiet check at startup
    let mut update_tag: Option<String> = None;
    let mut announce_update = false;
    let mi_log = MenuItem::new("Open log", true, None);
    let mi_exit = MenuItem::new("Exit", true, None);

    let menu = Menu::new();
    let _ = menu.append_items(&[
        &mi_music,
        &PredefinedMenuItem::separator(),
        &mi_discord,
        &mi_start,
        &mi_stop,
        &PredefinedMenuItem::separator(),
        &sub_line,
        &mi_auto,
        &mi_log,
        &sub_about,
        &PredefinedMenuItem::separator(),
        &mi_exit,
    ]);

    let tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(true)
        .with_tooltip(APP_NAME)
        .with_icon(icon_on.clone())
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            log(&format!("Could not create the tray icon: {e}"));
            platform::error_box(APP_NAME, &format!("Could not create the tray icon:\n{e}"));
            return;
        }
    };

    let mut shown: (String, Option<String>, Option<String>) = (String::new(), None, None);
    let mut shown_discord = String::new();
    let mut shown_on: Option<bool> = None;
    let mut details_visible = false;

    platform::run_loop(|| {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == *mi_start.id() {
                shared.enabled.store(true, Relaxed);
            } else if ev.id == *mi_stop.id() {
                shared.enabled.store(false, Relaxed);
            } else if ev.id == *mi_auto.id() {
                autostart::set(!autostart::is_on());
                mi_auto.set_checked(autostart::is_on());
            } else if ev.id == *mi_log.id() {
                if !log_path.exists() {
                    let _ = std::fs::write(&log_path, "");
                }
                let _ = std::process::Command::new("notepad.exe").arg(&log_path).spawn();
            } else if ev.id == *mi_music.id() {
                open_apple_music();
            } else if ev.id == *mi_home.id() {
                open_url(update::HOME_URL);
            } else if ev.id == *mi_changes.id() {
                open_url(update::CHANGELOG_URL);
            } else if ev.id == *mi_update.id() {
                if update_tag.is_some() {
                    open_url(update::RELEASES_URL);
                } else {
                    announce_update = true;
                    mi_update.set_enabled(false);
                    check_for_update();
                }
            } else if ev.id == *mi_exit.id() {
                shared.quit.store(true, Relaxed);
            } else if let Some((m, key, _)) = mode_items.iter().find(|(_, _, item)| ev.id == *item.id()) {
                shared.mode.store(*m, Relaxed);
                for (other, _, item) in &mode_items {
                    item.set_checked(other == m);
                }
                config.status_line = key.to_string();
                save_config(&config_path, &config);
            }
        }
        if shared.quit.load(Relaxed) {
            return false;
        }

        let checked = update_result.lock().unwrap().take();
        if let Some(result) = checked {
            let announce = std::mem::take(&mut announce_update);
            mi_update.set_enabled(true);
            match result {
                Ok(tag) if update::newer(&tag, update::VERSION) => {
                    log(&format!("Update available: {tag} (running {}).", update::VERSION));
                    mi_update.set_text(format!("Update available: {tag}"));
                    let text = format!(
                        "Version {} is available (you have {}).\n\nOpen the download page?",
                        tag.trim_start_matches('v'),
                        update::VERSION
                    );
                    update_tag = Some(tag);
                    if announce && platform::confirm(APP_NAME, &text) {
                        open_url(update::RELEASES_URL);
                    }
                }
                Ok(tag) => {
                    log(&format!("Update check: latest is {tag}, running {}.", update::VERSION));
                    if announce {
                        platform::info_box(APP_NAME, &format!("You're up to date (version {}).", update::VERSION));
                    }
                }
                Err(e) => {
                    log(&format!("Update check failed: {e}"));
                    if announce {
                        platform::error_box(APP_NAME, &format!("Couldn't check for updates:\n{e}"));
                    }
                }
            }
        }

        // Mini player: song, artist/album and progress. Only touch the menu when text changes.
        let view = shared.view.lock().unwrap().clone();
        let lines = player_lines(&view, Instant::now());
        if lines != shown {
            let open = lines.1.is_some();
            if open != details_visible {
                if open {
                    let _ = menu.insert_items(&[&mi_title, &mi_by, &mi_time], 1);
                } else {
                    let _ = menu.remove(&mi_title);
                    let _ = menu.remove(&mi_by);
                    let _ = menu.remove(&mi_time);
                }
                mi_music.set_text(if open { MUSIC_ON } else { MUSIC_OFF });
                mi_music.set_icon(Some(if open { dot_on.clone() } else { dot_off.clone() }));
                details_visible = open;
            }
            if let (Some(by), Some(time)) = (&lines.1, &lines.2) {
                if lines.0 != shown.0 {
                    mi_title.set_text(menu_text(&lines.0));
                }
                if lines.1 != shown.1 {
                    mi_by.set_text(menu_text(by));
                }
                mi_time.set_text(menu_text(time));
            }
            // Hover text: "Song - Artist" plus the clock part of the progress line.
            let tip = match &view {
                Some(v) => {
                    let clock_part = lines.2.as_deref().unwrap_or("").trim_start_matches(['\u{25A0}', '\u{25A1}', ' ']);
                    format!("{}\n{}", shorten(&format!("{} - {}", v.title, v.artist), 60), clock_part)
                }
                None => format!("{APP_NAME}\n{}", lines.0),
            };
            let _ = tray.set_tooltip(Some(shorten(&tip, 120)));
            shown = lines;
        }

        let on = shared.enabled.load(Relaxed);
        let discord = if !on {
            "Discord: stopped"
        } else if shared.discord.load(Relaxed) {
            if view.as_ref().is_some_and(|v| v.playing) { "Discord: showing your song" } else { "Discord: connected" }
        } else {
            "Discord: waiting for Discord..."
        };
        if discord != shown_discord {
            mi_discord.set_text(discord);
            shown_discord = discord.to_string();
        }
        if shown_on != Some(on) {
            mi_start.set_enabled(!on);
            mi_stop.set_enabled(on);
            let _ = tray.set_icon(Some(if on { icon_on.clone() } else { icon_off.clone() }));
            shown_on = Some(on);
        }
        true
    });

    // Give the worker a moment to clear the Discord status before exiting.
    shared.quit.store(true, Relaxed);
    let _ = tray.set_visible(false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !shared.done.load(Relaxed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(playing: bool, position: Option<f64>, duration: Option<f64>) -> Option<View> {
        Some(View {
            title: "Halo".into(),
            artist: "Beyonce".into(),
            album: "I Am".into(),
            playing,
            position,
            duration,
            as_of: Instant::now(),
        })
    }

    #[test]
    fn readout_when_closed() {
        let (a, b, c) = player_lines(&None, Instant::now());
        assert_eq!(a, "Apple Music is not open");
        assert!(b.is_none() && c.is_none());
    }

    #[test]
    fn readout_shows_progress_length_and_time_left() {
        let v = view(true, Some(60.0), Some(240.0));
        let now = v.as_ref().unwrap().as_of + Duration::from_secs(23);
        let (title, by, time) = player_lines(&v, now);
        assert_eq!(title, "Halo");
        assert_eq!(by.unwrap(), "Beyonce \u{2014} I Am");
        let time = time.unwrap();
        assert!(time.ends_with("1:23 / 4:00  (-2:37)"), "{time}");
        assert_eq!(time.chars().filter(|c| *c == '\u{25A0}').count(), 4);
    }

    #[test]
    fn readout_when_paused_or_position_unknown() {
        let v = view(false, Some(60.0), Some(240.0));
        let later = v.as_ref().unwrap().as_of + Duration::from_secs(30);
        assert!(player_lines(&v, later).2.unwrap().ends_with("1:00 / 4:00  (-3:00)  paused"));
        assert_eq!(player_lines(&view(true, None, Some(240.0)), Instant::now()).2.unwrap(), "--:-- / 4:00");
        assert_eq!(player_lines(&view(true, None, None), Instant::now()).2.unwrap(), "Playing");
    }
}
